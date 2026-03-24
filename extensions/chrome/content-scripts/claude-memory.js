/**
 * claude-memory.js — Claude.ai 记忆提取脚本
 * 运行页面: https://claude.ai/settings/capabilities?modal=memory
 *
 * 策略: DOM 扫描 — Claude 记忆页面的结构化文本提取
 */

(function () {
  "use strict";

  const PLATFORM = "claude";
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
        if (resp?.ok) console.log(`[Sage] Claude memory sync: imported ${unique.length} memories`);
      }
    );
  }

  function _scanDom() {
    const memories = [];

    // 策略 1：找 "Manage memory" 对话框/区域内的所有段落
    // Claude 记忆页面有 h3 标题（Work context / Personal context / Top of mind）+ p 段落
    const headings = document.querySelectorAll("h3, h4, [role='heading']");
    const memoryHeadings = [];
    for (const h of headings) {
      const text = h.textContent?.trim().toLowerCase() || "";
      if (text.includes("context") || text.includes("mind") || text.includes("memory") ||
          text.includes("remember") || text.includes("preference") || text.includes("style")) {
        memoryHeadings.push(h);
      }
    }

    // 从每个匹配的标题出发，收集其后的段落文本
    for (const heading of memoryHeadings) {
      const section = heading.closest("div, section, article") || heading.parentElement;
      if (!section) continue;
      const category = heading.textContent?.trim() || "memory";
      const paragraphs = section.querySelectorAll("p");
      for (const p of paragraphs) {
        const t = p.textContent?.trim();
        if (t && t.length > 10 && t.length < 2000) {
          memories.push({ category, content: t, confidence: 0.8 });
        }
      }
    }

    // 策略 2：如果策略 1 没找到，广搜模态框内容
    if (memories.length === 0) {
      // 查找 modal/dialog 容器
      const modals = document.querySelectorAll("[role='dialog'], [data-state='open'], [class*='modal'], [class*='Modal']");
      for (const modal of modals) {
        const paragraphs = modal.querySelectorAll("p");
        for (const p of paragraphs) {
          const t = p.textContent?.trim();
          if (t && t.length > 20 && t.length < 2000) {
            memories.push({ category: "memory", content: t, confidence: 0.75 });
          }
        }
      }
    }

    // 策略 3：最后降级——找页面所有 p 标签中的长文本（排除导航等短文本）
    if (memories.length === 0) {
      const allP = document.querySelectorAll("main p, [class*='content'] p");
      for (const p of allP) {
        const t = p.textContent?.trim();
        if (t && t.length > 50 && t.length < 2000) {
          memories.push({ category: "memory", content: t, confidence: 0.7 });
        }
      }
    }

    console.log(`[Sage] Claude DOM scan: found ${memories.length} memory segments`);
    if (memories.length > 0) {
      _sendToBackground(memories);
      _synced = true;
      _observer.disconnect(); // 成功后停止监听
    }
  }

  let _synced = false;

  // 延迟扫描（等 SPA 渲染完成）
  function _scheduleScan() {
    setTimeout(_scanDom, 3000);
    setTimeout(() => { if (!_synced) _scanDom(); }, 8000);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", _scheduleScan);
  } else {
    _scheduleScan();
  }

  // MutationObserver: 等待动态内容加载，成功后自动停止
  const _observer = new MutationObserver(() => {
    if (_synced) return;
    clearTimeout(_observer._timer);
    _observer._timer = setTimeout(_scanDom, 1500);
  });
  _observer.observe(document.body || document.documentElement, { childList: true, subtree: true });

  // 手动触发接口
  window.__sageSyncMemories = function () {
    _scanDom();
    return true;
  };
})();
