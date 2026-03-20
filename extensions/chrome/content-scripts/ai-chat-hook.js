// AI Chat Fetch Hook — 注入到 AI 平台页面上下文
// 只捕获用户提问（行为元数据），不抓 AI 回复（各平台自己记）
// 支持：claude.ai / chatgpt.com / gemini.google.com
// 通过 window.postMessage 发回 content script

(function () {
  "use strict";

  const MSG_TYPE = "SAGE_AI_CHAT";
  const host = location.hostname;

  const PLATFORM = host.includes("claude.ai")
    ? "claude"
    : host.includes("chatgpt.com") || host.includes("chat.openai.com")
      ? "chatgpt"
      : host.includes("gemini.google.com")
        ? "gemini"
        : null;

  if (!PLATFORM) return;

  // --- 平台适配：识别"发送消息"的 API 请求 ---

  function isCompletionRequest(url, method) {
    if (method !== "POST") return false;
    if (PLATFORM === "claude") {
      return url.includes("/completion") || url.includes("/chat_conversations");
    }
    if (PLATFORM === "chatgpt") {
      return url.includes("/backend-api/conversation");
    }
    if (PLATFORM === "gemini") {
      return (
        url.includes("BardChatUi") ||
        url.includes("StreamGenerate") ||
        url.includes("assistant")
      );
    }
    return false;
  }

  // --- 从请求体提取用户消息 ---

  function extractUserMessage(body) {
    if (!body || typeof body !== "string") return null;
    try {
      const json = JSON.parse(body);
      return extractFromJson(json);
    } catch (_) {
      return extractFromText(body);
    }
  }

  function extractFromJson(json) {
    if (PLATFORM === "claude") {
      if (json.prompt) return json.prompt;
      if (json.messages && Array.isArray(json.messages)) {
        const last = json.messages.filter((m) => m.role === "user").pop();
        if (last) return typeof last.content === "string" ? last.content : JSON.stringify(last.content);
      }
    }

    if (PLATFORM === "chatgpt") {
      if (json.messages && Array.isArray(json.messages)) {
        const userMsgs = json.messages.filter(
          (m) => m.author?.role === "user" || m.role === "user"
        );
        const last = userMsgs.pop();
        if (!last) return null;
        const content = last.content;
        if (typeof content === "string") return content;
        if (content?.parts) return content.parts.join("\n");
        return JSON.stringify(content);
      }
    }

    if (PLATFORM === "gemini") {
      return findTextField(json);
    }

    return null;
  }

  function extractFromText(body) {
    if (PLATFORM !== "gemini") return null;
    const match = body.match(/"([^"]{10,})"/);
    if (match) return match[1];
    return null;
  }

  function findTextField(obj, depth) {
    if (!obj || depth > 5) return null;
    if (typeof obj === "string" && obj.length > 5) return obj;
    if (Array.isArray(obj)) {
      for (const item of obj) {
        const r = findTextField(item, (depth || 0) + 1);
        if (r) return r;
      }
    }
    if (typeof obj === "object") {
      for (const key of ["text", "content", "prompt", "query"]) {
        if (obj[key] && typeof obj[key] === "string" && obj[key].length > 3) {
          return obj[key];
        }
      }
      for (const key of Object.keys(obj)) {
        const r = findTextField(obj[key], (depth || 0) + 1);
        if (r) return r;
      }
    }
    return null;
  }

  // --- 发送到 content script ---

  function send(data) {
    window.postMessage(
      { type: MSG_TYPE, platform: PLATFORM, eventType: "user_message", data },
      "*"
    );
  }

  // --- 用户消息截短为摘要 ---

  function summarize(text) {
    if (!text) return null;
    // 截取前 200 字符作为主题摘要
    const trimmed = text.trim();
    if (trimmed.length <= 200) return trimmed;
    return trimmed.slice(0, 200) + "…";
  }

  // --- Fetch Hook ---

  const originalFetch = window.fetch;
  window.fetch = function (...args) {
    const [input, init] = args;
    const url = typeof input === "string" ? input : input?.url || "";
    const method = (init?.method || "GET").toUpperCase();

    if (isCompletionRequest(url, method)) {
      const userMsg = extractUserMessage(init?.body);
      if (userMsg) {
        send({
          content: summarize(userMsg),
          timestamp: new Date().toISOString(),
          url: location.href,
        });
      }
    }

    return originalFetch.apply(this, args);
  };

  // --- XMLHttpRequest Hook（Gemini fallback）---

  const XHR = XMLHttpRequest.prototype;
  const originalOpen = XHR.open;
  const originalSend = XHR.send;

  XHR.open = function (method, url, ...rest) {
    this._sage_method = method;
    this._sage_url = url;
    return originalOpen.call(this, method, url, ...rest);
  };

  XHR.send = function (body) {
    if (isCompletionRequest(this._sage_url || "", (this._sage_method || "GET").toUpperCase())) {
      const userMsg = extractUserMessage(body);
      if (userMsg) {
        send({
          content: summarize(userMsg),
          timestamp: new Date().toISOString(),
          url: location.href,
        });
      }
    }
    return originalSend.call(this, body);
  };
})();
