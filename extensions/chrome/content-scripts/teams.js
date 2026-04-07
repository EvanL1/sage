// Teams Web 消息捕获 — 多通道并行
// 三种捕获通道同时启动，共享去重，谁先捕到用谁：
//   1. Fetch Hook — 拦截 REST API 响应（历史消息、初始加载）
//   2. MutationObserver — 监听 DOM 变化（实时新消息，兼容 WebSocket/SignalR）
//   3. 定时扫描 — 兜底，防止前两种都漏掉
// 消息通过统一的 processMessage() 去重后发送

(function () {
  "use strict";

  var SOURCE = "teams";
  var LOG_PREFIX = "[Sage Teams]";
  var DEBOUNCE_MS = 500;
  var SCAN_INTERVAL = 30000; // 定时扫描间隔（兜底，前三通道是事件驱动）

  // --- 去重（持久化到 chrome.storage） ---

  var processedIds = new Set();
  var MAX_IDS = 2000;
  var todayMessageCount = 0;
  chrome.storage.local.get(
    ["teams_processed_ids", "teams_today_count", "teams_today_date"],
    function (result) {
      processedIds = new Set(result.teams_processed_ids || []);

      var today = new Date().toDateString();
      if (result.teams_today_date === today) {
        todayMessageCount = result.teams_today_count || 0;
      } else {
        todayMessageCount = 0;
        chrome.storage.local.set({ teams_today_count: 0, teams_today_date: today });
      }
    }
  );

  var persistTimer = null;
  function persistIds() {
    clearTimeout(persistTimer);
    persistTimer = setTimeout(function () {
      chrome.storage.local.set({
        teams_processed_ids: Array.from(processedIds).slice(-MAX_IDS),
      });
    }, 2000);
  }

  // --- 敏感信息过滤 ---

  var SENSITIVE_PATTERNS = [
    /password\s*[:=：is]/i,
    /密码\s*[:=：是]/,
    /token\s*[:=：]/i,
    /secret\s*[:=：]/i,
    /api[_-]?key\s*[:=：]/i,
    /credential/i,
    /ssh[_-]?key/i,
    /private[_-]?key/i,
  ];

  function isSensitive(text) {
    return SENSITIVE_PATTERNS.some(function (p) { return p.test(text); });
  }

  // --- 统一发送 ---

  // --- 当前用户名检测 ---
  var currentUserName = "";
  // 从 storage 恢复上次检测到的用户名（跨页面刷新保持）
  chrome.storage.local.get(["teams_current_user"], function (r) {
    if (r.teams_current_user) currentUserName = r.teams_current_user;
  });

  function detectCurrentUser() {
    // Teams 页面头像/个人信息区域 — 多种选择器覆盖不同版本
    var sels = [
      '#personButton span[class*="displayName"]',
      'button[data-tid="me-control"] span',
      '[data-tid="app-header-profile"] span',
      '#meInitials',
      // Fluent UI / 新版 Teams
      '[data-tid="app-header-profile-button"] span',
      'button[id="mectrl_headerPicture"]',
      '[class*="me-control"] span[class*="displayName"]',
      '[class*="ProfileCard"] span',
      // 新版 Teams 2.0 / Copilot era 选择器
      '[data-tid="me-control-button"] span',
      'button[id*="meControl"] span',
      '#mectrl_currentAccount_primary',
      // aria-label 兜底
      'button[aria-label*="profile" i]',
      'button[aria-label*="账户" i]',
      'button[aria-label*="个人资料" i]',
      'button[aria-label*="account" i]',
    ];
    for (var i = 0; i < sels.length; i++) {
      try {
        var el = document.querySelector(sels[i]);
        if (el) {
          // 优先 aria-label（更稳定），再 innerText
          var t = el.getAttribute("aria-label") || (el.innerText || el.textContent || "").trim();
          // aria-label 可能是 "打开个人资料, Evan Li" 或 "Open profile, Evan Li"
          if (t && (t.includes(",") || t.includes("，"))) {
            t = t.split(/[,，]/).pop().trim();
          }
          if (t && t.length > 1 && t.length < 40) { setCurrentUser(t); return; }
        }
      } catch (e) {}
    }
    // 兜底：从 .fui-ChatMyMessage 的 author 元素拿自己的名字
    try {
      var myMsg = document.querySelector('.fui-ChatMyMessage [class*="author"], [class*="ChatMyMessage"] [class*="author"]');
      if (myMsg) {
        var t = (myMsg.innerText || myMsg.textContent || "").trim();
        if (t && t.length > 1) { setCurrentUser(t); return; }
      }
    } catch (e) {}
  }

  function setCurrentUser(name) {
    if (name === currentUserName) return;
    currentUserName = name;
    chrome.storage.local.set({ teams_current_user: name });
    console.log(LOG_PREFIX + " 检测到当前用户: " + name);
  }

  // 启动时检测，多次重试（Teams SPA 加载慢）
  setTimeout(detectCurrentUser, 3000);
  setTimeout(detectCurrentUser, 8000);
  setTimeout(detectCurrentUser, 15000);
  // 额外晚一次重试，覆盖超慢网络/首次登录
  setTimeout(detectCurrentUser, 30000);

  function isFromMe(sender) {
    if (!sender || !currentUserName) return false;
    return sender === currentUserName || sender.toLowerCase() === currentUserName.toLowerCase();
  }

  function sendMessage(sender, content, channel, timestamp, messageType, chatType, direction) {
    var key = sender + "|" + (content || "").slice(0, 80) + "|" + timestamp;
    var hash = 0;
    for (var i = 0; i < key.length; i++) {
      hash = ((hash << 5) - hash + key.charCodeAt(i)) | 0;
    }
    var dedupeKey = "u_" + Math.abs(hash).toString(36);
    if (processedIds.has(dedupeKey)) return;
    processedIds.add(dedupeKey);

    if (!content || content.length < 2) return;
    if (isSensitive(content)) return;

    todayMessageCount++;
    chrome.storage.local.set({
      teams_today_count: todayMessageCount,
      teams_today_date: new Date().toDateString(),
    });
    persistIds();

    // 判断方向：显式传入 > 用户名匹配 > 默认 received
    var dir = direction || (isFromMe(sender) ? "sent" : "received");

    var metadata = {
      sender: sender || "Unknown",
      channel: channel || getCurrentChannel(),
      timestamp: timestamp || new Date().toISOString(),
      message_type: messageType || "text",
      chat_type: chatType || "unknown",
      content: content.length > 5000 ? content.slice(0, 5000) + "…" : content,
      direction: dir,
    };

    chrome.runtime.sendMessage({
      type: "BEHAVIOR_EVENT",
      payload: { source: SOURCE, event_type: "message_received", metadata: metadata },
    });
  }

  // ========================================================================
  // 通道 1: Fetch Hook — 通过注入 <script> 拦截 Teams REST API
  // ========================================================================

  var MSG_TYPE = "SAGE_TEAMS_FETCH";

  function injectFetchHook() {
    // 构造内联脚本（避免 web_accessible_resources 依赖）
    var code = '(' + (function () {
      var MSG = "SAGE_TEAMS_FETCH";
      var pending = [];
      var timer = null;

      function looksLike(obj) {
        if (!obj || typeof obj !== "object") return false;
        var hasContent = !!(obj.content || obj.body);
        var hasSender = !!(obj.from || obj.imdisplayname || obj.creator || obj.displayName);
        var hasTime = !!(obj.composetime || obj.originalarrivaltime || obj.createdTime);
        return hasContent && hasSender && hasTime;
      }

      function extract(obj) {
        var sender = obj.imdisplayname || obj.displayName || obj.creator ||
          (typeof obj.from === "string" ? obj.from : null) ||
          (obj.from && obj.from.user && obj.from.user.displayName) || "Unknown";
        var content = obj.content || obj.body || "";
        if (typeof content === "string") content = content.replace(/<[^>]+>/g, "").trim();
        var ts = obj.composetime || obj.originalarrivaltime || obj.createdTime || "";
        var ch = obj.conversationId || obj.threadId || "";
        var topic = (obj.threadProperties && obj.threadProperties.topic) || "";
        // threadtype: "group"=群聊, "topic"=频道, "p2p"/"chat"=私聊
        var tt = obj.threadtype || obj.threadType ||
          (obj.threadProperties && obj.threadProperties.threadType) || "";
        var chatType = "unknown";
        if (tt === "group" || tt === "space") chatType = "group";
        else if (tt === "topic" || tt === "channel") chatType = "channel";
        else if (tt === "p2p" || tt === "chat" || tt === "one_on_one") chatType = "p2p";
        else if (topic) chatType = "group"; // 有 topic 说明是群/频道
        // isFromMe 检测：多字段 fallback
        var fromMe = obj.isFromMe ||
          (obj.properties && (obj.properties.isFromMe === true || obj.properties.isFromMe === "true")) ||
          (obj.imDisplayName === "" && obj.imdisplayname === "") || // 自己的消息有时 displayname 为空
          false;
        // 对比 clientmessageid：本地发送的消息有 clientmessageid，收到的消息通常没有
        if (!fromMe && obj.clientmessageid && !obj.isread) {
          // clientmessageid 存在且 isread 不存在，可能是自己发送的
          // 这只是一个弱信号，配合 content script 端的 isFromMe(sender) 使用
        }
        return { sender: sender, content: content, timestamp: ts, channel: topic || ch, chatType: chatType, direction: fromMe ? "sent" : "" };
      }

      function scan(data, depth) {
        if (depth > 6 || !data) return;
        if (Array.isArray(data)) { data.forEach(function (d) { scan(d, depth + 1); }); return; }
        if (typeof data !== "object") return;
        if (looksLike(data)) {
          var m = extract(data);
          if (m.content && m.content.length > 0) pending.push(m);
          return;
        }
        Object.keys(data).forEach(function (k) { scan(data[k], depth + 1); });
      }

      function flush() {
        if (pending.length === 0) return;
        var batch = pending.splice(0);
        window.postMessage({ type: MSG, messages: batch }, "*");
      }

      // Hook fetch
      var origFetch = window.fetch;
      window.fetch = function () {
        var p = origFetch.apply(this, arguments);
        p.then(function (r) {
          if (!r.ok) return;
          var ct = r.headers.get("content-type") || "";
          if (!ct.includes("json")) return;
          r.clone().json().then(function (j) {
            scan(j, 0);
            if (pending.length > 0) {
              clearTimeout(timer);
              timer = setTimeout(flush, 500);
            }
          }).catch(function () {});
        }).catch(function () {});
        return p;
      };

      // Hook XHR
      var xhrOpen = XMLHttpRequest.prototype.open;
      var xhrSend = XMLHttpRequest.prototype.send;
      XMLHttpRequest.prototype.open = function (m, u) {
        this._sage_url = u;
        return xhrOpen.apply(this, arguments);
      };
      XMLHttpRequest.prototype.send = function () {
        this.addEventListener("load", function () {
          try {
            var ct = this.getResponseHeader("content-type") || "";
            if (!ct.includes("json")) return;
            scan(JSON.parse(this.responseText), 0);
            if (pending.length > 0) { clearTimeout(timer); timer = setTimeout(flush, 500); }
          } catch (e) {}
        });
        return xhrSend.apply(this, arguments);
      };

      // Hook WebSocket — 拦截 SignalR 实时消息
      var OrigWS = window.WebSocket;
      window.WebSocket = function (url, protocols) {
        var ws = protocols ? new OrigWS(url, protocols) : new OrigWS(url);

        ws.addEventListener("message", function (event) {
          try {
            var raw = event.data;
            if (typeof raw !== "string") return;
            // SignalR 消息以 JSON 结尾加 \x1e 分隔符
            var parts = raw.split("\x1e");
            for (var pi = 0; pi < parts.length; pi++) {
              var part = parts[pi].trim();
              if (!part || part[0] !== "{") continue;
              var json = JSON.parse(part);
              // SignalR invoke: { type: 1, target: "...", arguments: [...] }
              if (json.arguments && Array.isArray(json.arguments)) {
                for (var ai = 0; ai < json.arguments.length; ai++) {
                  scan(json.arguments[ai], 0);
                }
              }
              // 也扫描顶层（可能直接包含消息）
              scan(json, 0);
            }
            if (pending.length > 0) {
              clearTimeout(timer);
              timer = setTimeout(flush, 300);
            }
          } catch (e) {}
        });

        return ws;
      };
      window.WebSocket.prototype = OrigWS.prototype;
      window.WebSocket.CONNECTING = OrigWS.CONNECTING;
      window.WebSocket.OPEN = OrigWS.OPEN;
      window.WebSocket.CLOSING = OrigWS.CLOSING;
      window.WebSocket.CLOSED = OrigWS.CLOSED;
    }).toString() + ')()';

    var script = document.createElement("script");
    script.textContent = code;
    (document.head || document.documentElement).appendChild(script);
    script.remove();
    console.log(LOG_PREFIX + " 通道1: Fetch/XHR/WebSocket Hook 已注入");
  }

  // 接收 fetch hook 发来的消息
  window.addEventListener("message", function (event) {
    if (event.source !== window) return;
    if (!event.data || event.data.type !== MSG_TYPE) return;
    var messages = event.data.messages;
    if (!Array.isArray(messages)) return;

    var count = 0;
    messages.forEach(function (msg) {
      var before = todayMessageCount;
      sendMessage(msg.sender, msg.content, msg.channel, msg.timestamp, "text", msg.chatType || "unknown", msg.direction || "");
      if (todayMessageCount > before) count++;
    });
    if (count > 0) {
      console.log(LOG_PREFIX + " 通道1(Fetch): 捕获 " + count + " 条");
    }
  });

  // ========================================================================
  // 通道 2: MutationObserver — 监听 DOM 变化
  // ========================================================================

  // Fluent UI (fui-) 选择器 — Teams 当前组件库
  var FUI_ITEMS = ".fui-ChatMessage, .fui-ChatMyMessage, .fui-unstable-ChatItem";
  var FUI_CONTAINER = ".fui-Chat";

  // 传统选择器 fallback
  var LEGACY_ITEMS = [
    '[role="listitem"]',
    '[data-tid="message-pane-list-item"]',
    '[data-tid="chat-pane-item"]',
    ".ui-chat__item",
  ].join(",");

  // 合并选择器
  var ALL_ITEM_SELECTORS = FUI_ITEMS + ", " + LEGACY_ITEMS;

  // 发送者 / 内容 / 时间戳选择器
  var SENDER_SELS = [
    ".fui-ChatMessage__author",
    '[class*="ChatMyMessage_author"]',
    '[class*="ChatMessage_author"]',
    '[data-tid="message-header-name"]',
    '[class*="authorName"]',
    '[class*="senderName"]',
  ];

  var BODY_SELS = [
    ".fui-ChatMessage__body",
    '[class*="ChatMyMessage__body"]',
    '[class*="ChatMessage__body"]',
    '[data-tid="chat-pane-message"]',
    '[data-tid="message-body"]',
    '[class*="messageBody"]',
    '[class*="messageContent"]',
  ];

  var TS_SELS = [
    '[class*="ChatMessage_timestamp"]',
    '[class*="ChatMessage__timestamp"]',
    '[class*="ChatMyMessage_timestamp"]',
    "time[datetime]",
    '[data-tid*="timestamp"]',
  ];

  var CHANNEL_SELS = [
    '[data-tid="chat-title"]',
    '[data-tid="team-channel-header"]',
    'h1[class*="header"]',
    'div[role="banner"] h1',
    'div[role="banner"] span[title]',
  ];

  function queryText(root, sels) {
    for (var i = 0; i < sels.length; i++) {
      try {
        var el = root.querySelector(sels[i]);
        if (el) {
          var t = (el.innerText || el.textContent || "").trim();
          if (t) return t;
        }
      } catch (e) {}
    }
    return null;
  }

  function getCurrentChannel() {
    return queryText(document, CHANNEL_SELS) || "unknown";
  }

  function detectChatType() {
    // 检测当前页面是群聊还是私聊
    var header = queryText(document, CHANNEL_SELS) || "";
    // Teams 群聊/频道的标题栏通常有成员数显示
    var membersEl = document.querySelector('[data-tid="chat-header-member-count"], [class*="memberCount"], [class*="participantCount"]');
    if (membersEl) return "group";
    // 频道页面 URL 含 /channel/
    if (location.href.includes("/channel/")) return "channel";
    // 群聊页面标题通常含逗号分隔的多人名或自定义群名
    if (header && header.includes(",")) return "group";
    return "unknown";
  }

  function processDomMessage(el) {
    var text = (el.innerText || "").trim();
    if (!text || text.length < 2) return;

    // 去重用 DOM id
    var domId = el.getAttribute("data-message-id") ||
      el.getAttribute("data-mid") ||
      el.getAttribute("id") ||
      el.getAttribute("data-item-key");
    var dedupeKey = domId || ("d_" + hashStr(text.slice(0, 200)));
    if (processedIds.has(dedupeKey)) return;
    processedIds.add(dedupeKey);

    var sender = queryText(el, SENDER_SELS) || "Unknown";
    var body = queryText(el, BODY_SELS);
    if (!body) {
      // fallback: 用整个 innerText 减去 sender
      body = text;
      if (sender !== "Unknown" && body.startsWith(sender)) {
        body = body.slice(sender.length).trim();
      }
    }
    // 截断超长消息但不丢弃
    if (body.length > 5000) body = body.slice(0, 5000) + "…";

    var timestamp = null;
    var timeEl = el.querySelector("time[datetime]");
    if (timeEl) {
      timestamp = timeEl.getAttribute("datetime");
    }
    if (!timestamp) {
      var rawTs = queryText(el, TS_SELS);
      // 验证 fallback 时间戳是否可解析，防止把"星期四 10:26"等日期分隔行当成消息
      if (rawTs && !isNaN(new Date(rawTs).getTime())) {
        timestamp = rawTs;
      }
    }

    // 跳过没有有效 sender 且没有有效 timestamp 的元素（日期分隔行、系统消息等）
    if (sender === "Unknown" && !timestamp) return;

    // fui-ChatMyMessage = 自己发的消息
    var isMine = el.matches && (el.matches(".fui-ChatMyMessage") || el.matches('[class*="ChatMyMessage"]'));
    sendMessage(sender, body, null, timestamp, "text", detectChatType(), isMine ? "sent" : "");
  }

  function hashStr(s) {
    var h = 0;
    for (var i = 0; i < s.length; i++) {
      h = ((h << 5) - h + s.charCodeAt(i)) | 0;
    }
    return Math.abs(h).toString(36);
  }

  // 扫描所有消息元素
  function scanDom() {
    var items = document.querySelectorAll(ALL_ITEM_SELECTORS);
    var count = 0;
    items.forEach(function (el) {
      var before = todayMessageCount;
      processDomMessage(el);
      if (todayMessageCount > before) count++;
    });
    if (count > 0) {
      console.log(LOG_PREFIX + " 通道2(DOM): 捕获 " + count + " 条");
    }
  }

  // MutationObserver: 监听整个 body，过滤出消息相关变化
  var debounceTimer = null;

  function startMutationObserver() {
    var obs = new MutationObserver(function (mutations) {
      var dominated = false;
      for (var i = 0; i < mutations.length; i++) {
        var m = mutations[i];
        if (m.type !== "childList") continue;
        for (var j = 0; j < m.addedNodes.length; j++) {
          var node = m.addedNodes[j];
          if (node.nodeType !== Node.ELEMENT_NODE) continue;
          // 检查新增节点是否包含消息
          if (node.matches && node.matches(ALL_ITEM_SELECTORS)) {
            dominated = true; break;
          }
          if (node.querySelector && node.querySelector(ALL_ITEM_SELECTORS)) {
            dominated = true; break;
          }
          // 广义检查：任何包含 fui-Chat 相关 class 的节点
          var cls = (node.className || "").toString();
          if (/chat|message/i.test(cls)) {
            dominated = true; break;
          }
        }
        if (dominated) break;
      }
      if (!dominated) return;

      clearTimeout(debounceTimer);
      debounceTimer = setTimeout(scanDom, DEBOUNCE_MS);
    });

    obs.observe(document.body, { childList: true, subtree: true });
    console.log(LOG_PREFIX + " 通道2: MutationObserver 已启动");
  }

  // ========================================================================
  // 通道 3: 定时扫描 — 兜底
  // ========================================================================

  function startPeriodicScan() {
    setInterval(scanDom, SCAN_INTERVAL);
    console.log(LOG_PREFIX + " 通道3: 定时扫描已启动 (" + (SCAN_INTERVAL / 1000) + "s)");
  }

  // ========================================================================
  // SPA 路由变化检测
  // ========================================================================

  var lastHref = location.href;
  new MutationObserver(function () {
    if (location.href !== lastHref) {
      lastHref = location.href;
      console.log(LOG_PREFIX + " URL 变化，执行扫描");
      detectCurrentUser();
      setTimeout(scanDom, 1500);
    }
  }).observe(document.body, { childList: true, subtree: true });

  // ========================================================================
  // 入口：三通道并行启动
  // ========================================================================

  function start() {
    console.log(LOG_PREFIX + " 启动四通道并行捕获");

    // 通道 1: Fetch Hook（立即注入）
    injectFetchHook();

    // 通道 2: MutationObserver（立即启动）
    startMutationObserver();

    // 通道 3: 定时扫描（立即启动）
    startPeriodicScan();

    // 初次扫描（等 DOM 稳定）
    setTimeout(scanDom, 2000);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", start);
  } else {
    start();
  }

  // 通知 popup
  chrome.storage.local.set({ teams_page_active: true });
  window.addEventListener("beforeunload", function () {
    chrome.storage.local.set({ teams_page_active: false });
  });
})();
