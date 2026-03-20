/**
 * chatgpt-memory.js — ChatGPT 记忆提取脚本
 * 运行页面: https://chatgpt.com/settings (Personalization → Memory 页面)
 *
 * 策略:
 * 1. Hook fetch 拦截 /backend-api/memories 等接口
 * 2. 降级: MutationObserver 扫描 DOM 中的记忆列表项
 */

(function () {
  "use strict";

  const PLATFORM = "chatgpt";
  const MEMORY_API_PATTERNS = [
    "/backend-api/memories",
    "/api/memories",
    "/backend-api/user/memories",
  ];

  const _sentHashes = new Set();

  function _hash(text) {
    let h = 0;
    for (let i = 0; i < Math.min(text.length, 200); i++) {
      h = ((h << 5) - h + text.charCodeAt(i)) | 0;
    }
    return "h_" + Math.abs(h).toString(36);
  }

  function _dedup(memories) {
    const out = [];
    for (const m of memories) {
      const key = _hash(m.content);
      if (!_sentHashes.has(key)) {
        _sentHashes.add(key);
        out.push(m);
      }
    }
    return out;
  }

  function _sendToBackground(memories) {
    if (!memories || memories.length === 0) return;
    const unique = _dedup(memories);
    if (unique.length === 0) return;

    chrome.runtime.sendMessage(
      { type: "IMPORT_MEMORIES", payload: { source: PLATFORM, memories: unique } },
      (resp) => {
        if (chrome.runtime.lastError) return;
        if (resp?.ok) {
          console.log(`[Sage] ChatGPT memory sync: imported ${unique.length} memories`);
        }
      }
    );
  }

  // ── 从 API 响应 JSON 中提取记忆条目 ────────────────────────────────────────

  function _extractFromJson(json) {
    const results = [];

    // ChatGPT /backend-api/memories 返回格式：
    // { memories: [{ memory_id, text, ... }] }
    // 或 { items: [...] }
    const candidates = [
      json?.memories,
      json?.items,
      json?.data,
      json?.results,
    ].filter(Array.isArray);

    for (const list of candidates) {
      for (const item of list) {
        const content =
          item?.text || item?.content || item?.memory || item?.value ||
          (typeof item === "string" ? item : null);
        if (content && typeof content === "string" && content.trim().length > 3) {
          results.push({
            category: "behavior",
            content: content.trim(),
            confidence: 0.85,
          });
        }
      }
    }
    return results;
  }

  // ── fetch 钩子 ─────────────────────────────────────────────────────────────

  const _origFetch = window.fetch.bind(window);
  window.fetch = async function (input, init) {
    const url = typeof input === "string" ? input : input?.url || "";
    const resp = await _origFetch(input, init);

    if (MEMORY_API_PATTERNS.some((p) => url.includes(p))) {
      try {
        const clone = resp.clone();
        clone.json().then((json) => {
          const memories = _extractFromJson(json);
          if (memories.length > 0) {
            _sendToBackground(memories);
          }
        }).catch(() => {});
      } catch (_) {}
    }

    return resp;
  };

  // ── DOM 降级扫描 ────────────────────────────────────────────────────────────

  const MEMORY_SELECTORS = [
    // ChatGPT 设置页面中的记忆列表
    '[data-testid*="memory"]',
    '[class*="memory"]',
    // 通用模式
    "div[class*='Memory'] span",
    "li[class*='memory']",
    // Settings 弹窗中的列表
    "div[role='dialog'] ul li",
    // 按钮旁的文字（记忆条目通常带 delete 按钮）
    "div[class*='item'] > span:first-child",
  ];

  function _scanDom() {
    const texts = new Set();

    for (const sel of MEMORY_SELECTORS) {
      try {
        document.querySelectorAll(sel).forEach((el) => {
          const t = el.innerText?.trim();
          // 排除按钮文字（Delete / Forget / X 等）
          if (
            t && t.length > 10 && t.length < 500 &&
            !["delete", "remove", "forget", "×", "x"].includes(t.toLowerCase())
          ) {
            texts.add(t);
          }
        });
      } catch (_) {}
    }

    if (texts.size === 0) return;

    const memories = [...texts].map((t) => ({
      category: "behavior",
      content: t,
      confidence: 0.8,
    }));
    _sendToBackground(memories);
  }

  function _scheduleScan() {
    setTimeout(_scanDom, 2000);
    setTimeout(_scanDom, 5000);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", _scheduleScan);
  } else {
    _scheduleScan();
  }

  const _observer = new MutationObserver(() => {
    clearTimeout(_observer._timer);
    _observer._timer = setTimeout(_scanDom, 800);
  });

  _observer.observe(document.body, { childList: true, subtree: true });

  window.__sageSyncMemories = function () {
    _scanDom();
    return true;
  };
})();
