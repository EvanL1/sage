/**
 * claude-memory.js — Claude.ai 记忆提取脚本
 * 运行页面: https://claude.ai/settings (记忆管理页面)
 *
 * 策略:
 * 1. Hook fetch 拦截 /api/account/settings 或 /memories 等接口
 * 2. 降级: MutationObserver 扫描 DOM 中的记忆列表项
 */

(function () {
  "use strict";

  const PLATFORM = "claude";
  const MEMORY_API_PATTERNS = ["/api/account/settings", "/api/memories", "/api/user/memories"];

  // 已发送内容 hash 集合，避免重复发送
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
          console.log(`[Sage] Claude memory sync: imported ${unique.length} memories`);
        }
      }
    );
  }

  // ── 从 API 响应 JSON 中提取记忆条目 ────────────────────────────────────────

  function _extractFromJson(json) {
    const results = [];

    // Claude 可能的数据结构探测
    const candidates = [
      json?.memories,
      json?.data?.memories,
      json?.account?.memories,
      json?.settings?.memories,
      json?.user?.memories,
    ].filter(Array.isArray);

    for (const list of candidates) {
      for (const item of list) {
        const content =
          item?.text || item?.content || item?.memory || item?.value ||
          (typeof item === "string" ? item : null);
        if (content && typeof content === "string" && content.trim().length > 3) {
          results.push({
            category: item?.category || item?.type || "behavior",
            content: content.trim(),
            confidence: 0.8,
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
    // 通用列表项
    '[data-testid*="memory"]',
    '[class*="memory-item"]',
    '[class*="memoryItem"]',
    '[aria-label*="memory"]',
    // 通用 li > span/p 结构（设置页面常见布局）
    "ul[class*='memory'] li",
    "div[class*='memory-list'] > div",
  ];

  function _scanDom() {
    const texts = [];
    for (const sel of MEMORY_SELECTORS) {
      try {
        document.querySelectorAll(sel).forEach((el) => {
          const t = el.innerText?.trim();
          if (t && t.length > 5 && t.length < 500) texts.push(t);
        });
      } catch (_) {}
    }

    if (texts.length === 0) return;

    const memories = texts.map((t) => ({
      category: "behavior",
      content: t,
      confidence: 0.75,
    }));
    _sendToBackground(memories);
  }

  // 页面加载完成后延迟扫描（等 React 渲染完成）
  function _scheduleScan() {
    setTimeout(_scanDom, 2000);
    setTimeout(_scanDom, 5000);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", _scheduleScan);
  } else {
    _scheduleScan();
  }

  // MutationObserver: 动态加载的记忆列表
  const _observer = new MutationObserver(() => {
    clearTimeout(_observer._timer);
    _observer._timer = setTimeout(_scanDom, 800);
  });

  _observer.observe(document.body, { childList: true, subtree: true });

  // 暴露手动触发接口，供 popup "Sync Memories" 按钮调用
  window.__sageSyncMemories = function () {
    _scanDom();
    return true;
  };
})();
