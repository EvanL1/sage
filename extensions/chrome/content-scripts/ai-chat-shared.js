// AI Chat 共用逻辑 — 被 claude.js / chatgpt.js / gemini.js 引用
// 依赖：调用前必须已定义全局变量 AI_SOURCE（平台名）

const MSG_TYPE = "SAGE_AI_CHAT";

// --- 去重 ---
let _processedHashes = new Set();

function _hashMsg(content, timestamp) {
  const str = `${(content || "").slice(0, 100)}|${timestamp}`;
  let hash = 0;
  for (let i = 0; i < str.length; i++) {
    hash = ((hash << 5) - hash + str.charCodeAt(i)) | 0;
  }
  return "h_" + Math.abs(hash).toString(36);
}

// --- 敏感信息过滤 ---
const _SENSITIVE = [
  /password\s*[:=：]/i, /密码\s*[:=：是]/, /token\s*[:=：]/i,
  /secret\s*[:=：]/i, /api[_-]?key\s*[:=：]/i,
];

// --- 注入 fetch hook ---
function _injectHook() {
  const script = document.createElement("script");
  script.src = chrome.runtime.getURL("content-scripts/ai-chat-hook.js");
  script.onload = () => script.remove();
  (document.head || document.documentElement).appendChild(script);
}

// --- 接收 hook 消息 ---
window.addEventListener("message", (event) => {
  if (event.source !== window) return;
  if (!event.data || event.data.type !== MSG_TYPE) return;
  if (event.data.platform !== AI_SOURCE) return;

  const { eventType, data } = event.data;
  if (!data?.content || data.content.length < 3) return;
  if (_SENSITIVE.some((p) => p.test(data.content))) return;

  const key = _hashMsg(data.content, data.timestamp);
  if (_processedHashes.has(key)) return;
  _processedHashes.add(key);

  if (_processedHashes.size > 500) {
    const arr = [..._processedHashes];
    _processedHashes = new Set(arr.slice(-300));
  }

  chrome.runtime.sendMessage({
    type: "BEHAVIOR_EVENT",
    payload: {
      source: AI_SOURCE,
      event_type: eventType,
      metadata: {
        content: data.content,
        timestamp: data.timestamp,
        conversation_url: data.url || location.href,
      },
    },
  });
});

// --- 入口 ---
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", _injectHook);
} else {
  _injectHook();
}

// Session beacon
chrome.runtime.sendMessage({
  type: "BEHAVIOR_EVENT",
  payload: { source: AI_SOURCE, event_type: "session_active", metadata: { url: location.href } },
});
