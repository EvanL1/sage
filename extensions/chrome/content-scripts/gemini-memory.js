/**
 * gemini-memory.js — Google Gemini 记忆提取脚本
 * 运行页面: https://gemini.google.com/app/settings 及相关设置页面
 *
 * 策略:
 * 1. Hook fetch 拦截 Gemini 内存/设置相关 API
 * 2. 降级: MutationObserver 扫描 DOM 中的记忆/偏好列表
 * 注意: Gemini 无独立"记忆"功能（截至 2026），主要捕获 Activity/Preferences
 */

(function () {
  "use strict";

  const PLATFORM = "gemini";
  const MEMORY_API_PATTERNS = [
    "/memories",
    "/personalization",
    "/preferences",
    "/_/BardChatUi/data/assistant.lamda",
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
          console.log(`[Sage] Gemini memory sync: imported ${unique.length} memories`);
        }
      }
    );
  }

  // ── 递归从任意 JSON 结构中提取字符串记忆 ──────────────────────────────────

  function _extractStringsFromJson(obj, depth = 0) {
    if (depth > 6 || !obj) return [];
    const results = [];

    if (typeof obj === "string" && obj.trim().length > 10 && obj.trim().length < 500) {
      results.push(obj.trim());
    } else if (Array.isArray(obj)) {
      for (const item of obj) {
        results.push(..._extractStringsFromJson(item, depth + 1));
      }
    } else if (typeof obj === "object") {
      // 优先取 text/content/value 字段
      const priorityKeys = ["text", "content", "memory", "value", "description", "preference"];
      for (const key of priorityKeys) {
        if (typeof obj[key] === "string" && obj[key].trim().length > 10) {
          results.push(obj[key].trim());
        }
      }
      // 递归其他字段（但跳过 id/timestamp 等无关字段）
      const skipKeys = new Set(["id", "timestamp", "created_at", "updated_at", "type", "status"]);
      for (const [k, v] of Object.entries(obj)) {
        if (!skipKeys.has(k) && !priorityKeys.includes(k)) {
          results.push(..._extractStringsFromJson(v, depth + 1));
        }
      }
    }
    return results;
  }

  function _extractFromJson(json) {
    // 只在设置/记忆相关响应中提取
    const texts = _extractStringsFromJson(json);
    return [...new Set(texts)].map((t) => ({
      category: "preference",
      content: t,
      confidence: 0.7,
    }));
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
    // Gemini 设置/偏好页面通用选择器
    "mat-list-item",
    "[class*='setting'] [class*='description']",
    "[class*='preference'] span",
    "[class*='memory'] li",
    // Material Design 列表
    ".mat-list-item-content span",
    "li[class*='item'] > span",
    // 通用段落
    "section p",
  ];

  // 过滤短标签和 UI 元素文字
  const SKIP_WORDS = new Set([
    "on", "off", "enabled", "disabled", "save", "cancel", "delete",
    "edit", "settings", "preferences", "ok", "yes", "no", "submit",
  ]);

  function _scanDom() {
    const texts = new Set();

    for (const sel of MEMORY_SELECTORS) {
      try {
        document.querySelectorAll(sel).forEach((el) => {
          const t = el.innerText?.trim();
          if (
            t && t.length > 15 && t.length < 500 &&
            !SKIP_WORDS.has(t.toLowerCase())
          ) {
            texts.add(t);
          }
        });
      } catch (_) {}
    }

    if (texts.size === 0) return;

    const memories = [...texts].map((t) => ({
      category: "preference",
      content: t,
      confidence: 0.7,
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
