// Teams Web 消息捕获内容脚本
// 监听 Microsoft Teams 网页版聊天消息，提取元数据发送到 Sage Bridge
// 兼容 Edge 浏览器

const SOURCE = "teams";

// --- 常量 ---

// 消息去重集合（从 storage 恢复，实现跨刷新增量）
let processedIds = new Set();
const MAX_STORED_IDS = 500;

let todayMessageCount = 0;
let debounceTimer = null;
const DEBOUNCE_MS = 500;

// 持久化已处理 ID（限制上限，FIFO 淘汰）
function persistProcessedIds() {
  const ids = [...processedIds];
  // 只保留最近 MAX_STORED_IDS 个
  const toStore = ids.slice(-MAX_STORED_IDS);
  chrome.storage.local.set({ teams_processed_ids: toStore });
}

// --- 初始化：读取配置 + 恢复已处理 ID ---

chrome.storage.local.get(
  [
    "teams_today_count",
    "teams_today_date",
    "teams_processed_ids",
  ],
  (result) => {
    // 恢复已处理 ID（一次性清空旧缓存，重新捕获带内容的消息）
    // TODO: 下个版本恢复持久化：processedIds = new Set(result.teams_processed_ids || []);
    processedIds = new Set();
    chrome.storage.local.remove("teams_processed_ids");

    // 如果是新的一天，重置计数（但不清空 ID，避免重复捕获跨天消息）
    const today = new Date().toDateString();
    if (result.teams_today_date === today) {
      todayMessageCount = result.teams_today_count ?? 0;
    } else {
      todayMessageCount = 0;
      chrome.storage.local.set({
        teams_today_count: 0,
        teams_today_date: today,
      });
    }
  }
);

// --- 消息选择器（新版 Teams 优先，旧版 fallback）---

// 新版 Teams (teams.cloud.microsoft) 使用 data-testid
// 旧版 Teams (teams.microsoft.com) 使用 data-tid / .ui-chat__*
// 分两组：chat list sidebar + 对话区域
const SIDEBAR_SELECTORS = [
  '[data-testid="comfy-message-wrapper"]',
];
const CONVERSATION_SELECTORS = [
  '[data-testid="message-pane-list-item"]',
  '[data-testid="message-wrapper"]',
  '[data-tid="message-pane-list-item"]',
  '[class*="message-list-item"]',
  ".ui-chat__item",
  '[data-scope="message"]',
];
const MESSAGE_SELECTORS = [...SIDEBAR_SELECTORS, ...CONVERSATION_SELECTORS].join(",");

// --- DOM 选择器工具函数 ---

function queryText(root, selectors) {
  for (const sel of selectors) {
    try {
      const el = root.querySelector(sel);
      if (el) {
        const text = el.innerText?.trim() || el.textContent?.trim();
        if (text) return text;
      }
    } catch (_) {}
  }
  return null;
}

/**
 * 新版 Teams chat list：从 comfy-message-wrapper 的兄弟 DIV 提取对话名
 * DOM 结构：parent > [0] 对话名+日期 | [1] comfy-message-wrapper
 * 例如："Jiong Li3/11" → "Jiong Li"，"EMS—Monarch Hub开发项目3/11" → "EMS—Monarch Hub开发项目"
 */
function extractConversationName(el) {
  const parent = el.parentElement;
  if (!parent || parent.children.length < 2) return null;

  // comfy-message-wrapper 通常是第二个子元素，第一个是对话名
  const sibling = parent.children[0];
  if (sibling === el || !sibling.textContent) return null;

  let name = sibling.textContent.trim();
  // 去掉末尾日期/时间标记（直接拼接在名字后面，无空格）
  // "3/11" "12/25" "10:30" "昨天" "今天" "前天" "上午 10:30" "星期一"
  name = name
    .replace(
      /(\d{1,2}\/\d{1,2}|昨天|前天|今天|\d{1,2}:\d{2}|[上下]午\s*\d{1,2}:\d{2}|星期[一二三四五六日天])$/,
      ""
    )
    .trim();
  return name || null;
}

/**
 * 检测文本是否包含敏感信息（密码、token、密钥等）
 * 匹配到则跳过记忆写入，防止凭据泄漏
 */
function containsSensitiveInfo(text) {
  const patterns = [
    /password\s*[:=：is]/i,
    /密码\s*[:=：是]/,
    /the password/i,
    /token\s*[:=：]/i,
    /secret\s*[:=：]/i,
    /api[_-]?key\s*[:=：]/i,
    /credential/i,
    /ssh[_-]?key/i,
    /private[_-]?key/i,
  ];
  return patterns.some((p) => p.test(text));
}

/**
 * 简单字符串 hash（用于生成 fallback 消息 ID）
 */
function simpleHash(str) {
  let hash = 0;
  for (let i = 0; i < str.length; i++) {
    hash = ((hash << 5) - hash + str.charCodeAt(i)) | 0;
  }
  return "hash_" + Math.abs(hash).toString(36);
}

function getMessageId(el) {
  // 新版 Teams：id 属性包含 message ID
  // 旧版 Teams：data-message-id 或 data-mid
  const id = el.getAttribute("id");
  if (id && id.includes("message")) return id;

  // 尝试子元素的 id（comfy-message 在 wrapper 内部）
  const inner = el.querySelector('[data-testid="comfy-message"][id]');
  if (inner) return inner.getAttribute("id");

  const explicitId =
    el.getAttribute("data-message-id") ||
    el.getAttribute("data-mid") ||
    el.getAttribute("data-item-key");
  if (explicitId) return explicitId;

  // 对话区域 fallback：任意带 id 的子元素
  const anyId = el.querySelector("[id]");
  if (anyId?.id) return anyId.id;

  // 最终 fallback：基于内容 hash 生成 ID（确保可去重）
  const text = (el.textContent || "").trim().slice(0, 200);
  if (text.length > 3) return simpleHash(text);

  return id || null;
}

function detectMessageType(el) {
  // 附件检测：只用结构性选择器，避免 aria-label 误匹配普通文本
  if (
    el.querySelector(
      '[data-testid*="attachment"], [data-testid*="file"], [data-tid="attachment-card"], .ui-attachment'
    )
  ) {
    return "file";
  }
  // 会议/通话检测：只用结构性选择器（aria-label*="meeting" 会误匹配含 "meeting" 的普通消息）
  if (
    el.querySelector(
      '[data-testid*="meeting"], [data-testid*="call"], [data-tid="meeting-card"], .ui-meeting'
    )
  ) {
    return "meeting";
  }
  return "text";
}

function extractSender(el) {
  // 策略 1：新版 Teams chat list — 消息内容前缀（"你：xxx" / "张三：xxx"）
  const comfy =
    el.querySelector('[data-testid="comfy-message"]') ||
    (el.matches?.('[data-testid="comfy-message-wrapper"]') ? el : null);
  if (comfy) {
    const text = comfy.textContent?.trim() || "";
    const prefixMatch = text.match(/^(.{1,20})[：:]\s*/);
    if (prefixMatch) return prefixMatch[1].trim();
  }

  // 策略 2：新版 Teams chat list — 兄弟 DIV 对话名（1:1 聊天时即联系人）
  const convName = extractConversationName(el);
  if (convName) return convName;

  // 策略 3：新版 Teams 对话区域 — fui-ChatMessage__author / fui-ChatMyMessage__author
  const myAuthor = el.querySelector('[class*="ChatMyMessage__author"]');
  if (myAuthor) {
    const name = myAuthor.textContent?.trim();
    if (name) return name;
  }
  const otherAuthor = el.querySelector('[class*="ChatMessage__author"]');
  if (otherAuthor) {
    const name = otherAuthor.textContent?.trim();
    if (name) return name;
  }

  // 策略 4：新版 Teams data-testid 选择器（fallback）
  const sender = queryText(el, [
    '[data-testid="message-author-name"]',
    '[data-testid="message-header-name"]',
    '[data-testid*="author"]',
    '[data-testid*="sender"]',
    '[data-testid*="display-name"]',
  ]);
  if (sender) return sender;

  // 策略 5：旧版 Teams 选择器
  return queryText(el, [
    '[data-tid="message-header-name"]',
    ".ui-chat__message__author",
    '[class*="authorName"]',
    '[class*="senderName"]',
    "[data-scope='message-author']",
    'span[title][class*="author"]',
  ]);
}

function extractContent(el) {
  // 策略 1：chat list sidebar — comfy-message（去掉 "发送者：" 前缀）
  const comfy = el.querySelector('[data-testid="comfy-message"]');
  if (comfy) {
    let text = comfy.textContent?.trim() || "";
    text = text.replace(/^.{1,20}[：:]\s*/, "").trim();
    if (text) return text;
  }

  // 策略 2：对话区域 — 新版 Teams 消息体 + 通用选择器
  const convText = queryText(el, [
    '[class*="Message__body"]',
    '[data-testid="message-body"]',
    '[data-testid="chat-pane-message"]',
    '[data-testid*="message-text"]',
    '[data-testid*="message-content"]',
    ".message-body-content",
    'div[class*="markdown"]',
    "p",
  ]);
  if (convText) return convText;

  // 策略 3：旧版 fallback
  const text = queryText(el, [
    '[data-tid="chat-pane-message"]',
    ".ui-chat__message__content",
    '[class*="messageBody"]',
    "[data-scope='message-content']",
  ]);
  if (text) return text;

  // 策略 4（最后兜底）：从 heading 提取（截断摘要，仅当其他策略全失败时）
  const heading = el.querySelector('[role="heading"]');
  if (heading) {
    let h = heading.textContent?.trim() || "";
    h = h.replace(/\s+x\s+\S+(\s+\S+)?$/, "").trim();
    if (h) return h;
  }
  return null;
}

/**
 * 从消息 DOM 提取实际发送时间
 * Teams 在消息中嵌入 <time datetime="ISO8601"> 元素
 * 回退到扫描时间
 */
function extractTimestamp(el) {
  // 策略 1：<time datetime="..."> 元素（新版 Teams 标准方式）
  const timeEl = el.querySelector("time[datetime]");
  if (timeEl) {
    const dt = timeEl.getAttribute("datetime");
    if (dt) return dt;
  }

  // 策略 2：data-testid 含 timestamp 的元素
  const tsEl = el.querySelector('[data-testid*="timestamp"]');
  if (tsEl) {
    const dt = tsEl.getAttribute("datetime") || tsEl.getAttribute("title");
    if (dt) return dt;
  }

  // 策略 3：aria-label 含时间模式的元素（"10:30 AM" / "下午 3:45"）
  const labelEl = el.querySelector("[aria-label]");
  if (labelEl) {
    const label = labelEl.getAttribute("aria-label") || "";
    // 匹配 ISO 日期或常见时间格式
    const isoMatch = label.match(/\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/);
    if (isoMatch) return isoMatch[0];
  }

  // 策略 4：chat list sidebar — 兄弟 DIV 中的日期文本（"3/11" "今天" "10:30"）
  // 这些不够精确，不做解析，直接回退

  return new Date().toISOString();
}

function getCurrentChannel() {
  // 新版 Teams：尝试 data-testid 选择器
  const channel = queryText(document, [
    '[data-testid="chat-header-title"]',
    '[data-testid="channel-header-title"]',
    '[data-testid*="header-title"]',
    '[data-testid*="thread-header"]',
    ".channel-header-title",
    '[data-tid="team-channel-header"]',
    'h1[class*="header"]',
    'div[role="banner"] h1',
    'div[role="banner"] span[title]',
  ]);
  if (channel) return channel;

  // 新版 Teams 对话区域：从非自己的 author 推断对话方（1:1 聊天）
  const partner = getConversationPartner();
  if (partner) return partner;

  // 从 URL hash 提取（新版 Teams 用 hash 路由）
  return extractChannelFromUrl();
}

/**
 * 从对话区域推断对话方名字（1:1 聊天场景）
 * fui-ChatMessage__author = 对方，fui-ChatMyMessage__author = 自己
 */
function getConversationPartner() {
  // 先获取"自己"的名字
  const myEls = document.querySelectorAll('[class*="ChatMyMessage__author"]');
  let myName = null;
  for (const el of myEls) {
    const n = el.textContent?.trim();
    if (n) { myName = n; break; }
  }
  // 找到非自己的 author
  const allAuthors = document.querySelectorAll('[class*="ChatMessage__author"]');
  for (const el of allAuthors) {
    // 跳过 ChatMyMessage__author（它也包含 "ChatMessage__author" 子串）
    if (el.className.includes("ChatMyMessage")) continue;
    const name = el.textContent?.trim();
    if (name && name !== myName) return name;
  }
  return null;
}

function extractChannelFromUrl() {
  try {
    const url = new URL(location.href);
    const threadId = url.searchParams.get("threadId") || "";
    if (threadId) {
      return `channel_${threadId.slice(0, 8)}`;
    }
    // 新版 Teams hash 路由：#/conversations/xxx
    const hash = url.hash || "";
    const convMatch = hash.match(/conversations\/([^/?]+)/);
    if (convMatch) return `conv_${convMatch[1].slice(0, 12)}`;
  } catch (_) {}
  return "unknown";
}

// --- 消息处理 ---

/**
 * 处理单条消息 DOM 元素
 */
function processMessage(el) {
  const msgId = getMessageId(el);

  // 去重检查（持久化，跨刷新有效）
  if (msgId) {
    if (processedIds.has(msgId)) return;
    processedIds.add(msgId);
  } else {
    // 没有 ID 的消息无法去重，跳过避免重复
    return;
  }

  const sender = extractSender(el) || "Unknown";
  // 优先从兄弟 DIV 提取对话名作为 channel，fallback 到页面级选择器
  const channel = extractConversationName(el) || getCurrentChannel();
  const timestamp = extractTimestamp(el);
  const messageType = detectMessageType(el);

  // 更新计数
  todayMessageCount++;
  chrome.storage.local.set({
    teams_today_count: todayMessageCount,
    teams_today_date: new Date().toDateString(),
  });

  // 提取内容（文字消息才提取，附件/会议只记元数据）
  let content = null;
  if (messageType === "text") {
    const raw = extractContent(el);
    if (raw && raw.length > 5 && !containsSensitiveInfo(raw)) {
      content = raw;
    }
  }

  // 发送行为事件到 browser_behaviors（含内容），不直接写 memories
  chrome.runtime.sendMessage({
    type: "BEHAVIOR_EVENT",
    payload: {
      source: SOURCE,
      event_type: "message_received",
      metadata: {
        sender,
        channel,
        timestamp,
        message_type: messageType,
        ...(content && { content }),
      },
    },
  });

  // 批量持久化已处理 ID（防抖，避免频繁写 storage）
  persistProcessedIds();
}

function scanMessages() {
  const messageEls = document.querySelectorAll(MESSAGE_SELECTORS);
  messageEls.forEach((el) => processMessage(el));
}

// --- MutationObserver：监听新消息 DOM 节点 ---

function handleMutations(mutations) {
  let hasNewNodes = false;

  for (const mutation of mutations) {
    if (mutation.type !== "childList") continue;
    for (const node of mutation.addedNodes) {
      if (node.nodeType !== Node.ELEMENT_NODE) continue;

      const isMessageItem = node.matches?.(MESSAGE_SELECTORS) || false;
      const containsMessages =
        node.querySelector?.(MESSAGE_SELECTORS) !== null;

      if (isMessageItem || containsMessages) {
        hasNewNodes = true;
        break;
      }
    }
    if (hasNewNodes) break;
  }

  if (!hasNewNodes) return;

  // 防抖：500ms 内合并多条消息
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(scanMessages, DEBOUNCE_MS);
}

// 启动 MutationObserver，等待 body 就绪
function startObserver() {
  const observer = new MutationObserver(handleMutations);
  observer.observe(document.body, {
    childList: true,
    subtree: true,
  });
  return observer;
}

// --- SPA 路由变化检测 ---

let lastHref = location.href;

function checkUrlChange() {
  if (location.href !== lastHref) {
    lastHref = location.href;
    // 导航到新频道：不清空 processedIds（已持久化，增量捕获）
    // 延迟等待 React 渲染完成
    setTimeout(scanMessages, 1000);
  }
}

const urlObserver = new MutationObserver(checkUrlChange);

// --- 滚动监听（捕获虚拟滚动渲染的消息）---

let scrollDebounce = null;
function handleScroll() {
  clearTimeout(scrollDebounce);
  scrollDebounce = setTimeout(scanMessages, 300);
}

// 监听对话区域滚动（Teams 虚拟列表只渲染可视区域，滚动时补充渲染）
function attachScrollListener() {
  // 新版 Teams 对话滚动容器通常有 role="main" 或 class 含 "scroll"
  const scrollTargets = [
    document.querySelector('[data-testid="message-pane-list"]'),
    document.querySelector('[role="main"]'),
    document.querySelector('[class*="scrollable"]'),
    document.querySelector('[class*="message-list"]'),
  ].filter(Boolean);

  if (scrollTargets.length > 0) {
    scrollTargets.forEach((el) => el.addEventListener("scroll", handleScroll, { passive: true }));
  } else {
    // fallback：监听 document 级滚动
    document.addEventListener("scroll", handleScroll, { passive: true, capture: true });
  }
}

// --- 入口 ---

// 确保 DOM 就绪后再启动
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", () => {
    startObserver();
    urlObserver.observe(document.body, { childList: true, subtree: true });
    attachScrollListener();
    // 初次扫描（页面已有消息）
    setTimeout(scanMessages, 1500);
    // 定期补扫（捕获虚拟滚动延迟渲染的消息，5秒一次）
    setInterval(scanMessages, 5000);
  });
} else {
  startObserver();
  urlObserver.observe(document.body, { childList: true, subtree: true });
  attachScrollListener();
  setTimeout(scanMessages, 1500);
  setInterval(scanMessages, 5000);
}

// 通知 popup：当前页面是 Teams
chrome.storage.local.set({ teams_page_active: true });

// 页面卸载时清除标记
window.addEventListener("beforeunload", () => {
  chrome.storage.local.set({ teams_page_active: false });
});
