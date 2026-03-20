// Teams Fetch Hook — 注入到页面上下文，拦截 Teams 自身 API 的 JSON 响应
// 通过 window.postMessage 将消息数据发回 content script
// 此文件由 teams.js 通过 <script> 标签注入

(function () {
  "use strict";

  const MSG_TYPE = "SAGE_TEAMS_MSG";
  const BATCH_INTERVAL = 800; // 批量发送间隔（ms）
  let pendingMessages = [];
  let batchTimer = null;

  // --- 消息检测 ---

  /**
   * 判断对象是否像 Teams 消息
   * Teams API 消息结构：
   *   { content/body, from/imdisplayname, composetime/originalarrivaltime, messagetype, ... }
   */
  function looksLikeMessage(obj) {
    if (!obj || typeof obj !== "object") return false;
    // 必须有内容字段
    const hasContent = !!(obj.content || obj.body);
    // 必须有发送者字段
    const hasSender = !!(
      obj.from ||
      obj.imdisplayname ||
      obj.creator ||
      obj.displayName
    );
    // 必须有时间字段
    const hasTime = !!(
      obj.composetime ||
      obj.originalarrivaltime ||
      obj.createdTime ||
      obj.lastModifiedTime
    );
    return hasContent && hasSender && hasTime;
  }

  /**
   * 从对象中提取标准化消息字段
   */
  function extractMessage(obj) {
    // 发送者：多种字段名 fallback
    let sender =
      obj.imdisplayname ||
      obj.displayName ||
      obj.creator ||
      (typeof obj.from === "string" ? obj.from : null) ||
      (obj.from && obj.from.user && obj.from.user.displayName) ||
      "Unknown";

    // 内容：content 优先，body 次之
    let content = obj.content || obj.body || "";
    // 去除 HTML 标签（Teams RichText 消息含 HTML）
    if (typeof content === "string") {
      content = content.replace(/<[^>]+>/g, "").trim();
    }

    // 时间戳
    const timestamp =
      obj.composetime ||
      obj.originalarrivaltime ||
      obj.createdTime ||
      obj.lastModifiedTime ||
      new Date().toISOString();

    // 消息 ID
    const id =
      obj.id ||
      obj.messageId ||
      obj.clientmessageid ||
      obj.version ||
      null;

    // 消息类型
    const messageType = obj.messagetype || obj.messageType || "text";

    // 对话/频道 ID
    const channel =
      obj.conversationId ||
      obj.threadId ||
      obj.to ||
      obj.conversationLink ||
      null;

    // 对话显示名
    const threadName = obj.threadProperties?.topic || null;

    return { id, sender, content, timestamp, messageType, channel, threadName };
  }

  /**
   * 递归扫描 JSON 数据，找出所有像消息的对象
   */
  function findMessages(data, depth) {
    if (depth > 6 || !data) return;

    if (Array.isArray(data)) {
      for (const item of data) {
        findMessages(item, depth + 1);
      }
      return;
    }

    if (typeof data !== "object") return;

    if (looksLikeMessage(data)) {
      const msg = extractMessage(data);
      // 过滤空内容和系统消息
      if (
        msg.content &&
        msg.content.length > 0 &&
        !msg.messageType.includes("Event") &&
        !msg.messageType.includes("ThreadActivity")
      ) {
        pendingMessages.push(msg);
      }
      return; // 不再递归子对象（消息本身已提取）
    }

    // 继续扫描子字段
    for (const key of Object.keys(data)) {
      findMessages(data[key], depth + 1);
    }
  }

  /**
   * 批量发送收集到的消息
   */
  function flushMessages() {
    if (pendingMessages.length === 0) return;
    const batch = pendingMessages.splice(0);
    window.postMessage({ type: MSG_TYPE, messages: batch }, "*");
  }

  function scheduleFlush() {
    if (batchTimer) return;
    batchTimer = setTimeout(() => {
      batchTimer = null;
      flushMessages();
    }, BATCH_INTERVAL);
  }

  /**
   * 处理拦截到的 JSON 响应
   */
  function processResponse(json) {
    try {
      findMessages(json, 0);
      if (pendingMessages.length > 0) {
        scheduleFlush();
      }
    } catch (_) {
      // 静默忽略解析错误
    }
  }

  // --- Fetch Hook ---

  const originalFetch = window.fetch;
  window.fetch = function (...args) {
    const promise = originalFetch.apply(this, args);
    promise
      .then((response) => {
        // 只处理成功的 JSON 响应
        if (!response.ok) return;
        const ct = response.headers.get("content-type") || "";
        if (!ct.includes("json")) return;

        // clone() 避免消耗原始 body
        response
          .clone()
          .json()
          .then(processResponse)
          .catch(() => {});
      })
      .catch(() => {});
    return promise;
  };

  // --- XMLHttpRequest Hook ---

  const XHR = XMLHttpRequest.prototype;
  const originalOpen = XHR.open;
  const originalSend = XHR.send;

  XHR.open = function (method, url, ...rest) {
    this._sage_url = url;
    return originalOpen.call(this, method, url, ...rest);
  };

  XHR.send = function (...args) {
    this.addEventListener("load", function () {
      try {
        const ct = this.getResponseHeader("content-type") || "";
        if (!ct.includes("json")) return;
        const json = JSON.parse(this.responseText);
        processResponse(json);
      } catch (_) {}
    });
    return originalSend.apply(this, args);
  };
})();
