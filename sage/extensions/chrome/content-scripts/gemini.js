// Content script for gemini.google.com
// Reports session activity and imports memory items if Gemini exposes them.

const SOURCE = "gemini";

// --- Session active beacon ---
chrome.runtime.sendMessage({
  type: "BEHAVIOR_EVENT",
  payload: { source: SOURCE, event: "session_active", url: location.href },
});

// --- Memory extraction helpers ---

/**
 * Attempts to extract memory / personalization items from Gemini's UI.
 * Gemini's memory surface (if present) lives under Settings > Extensions
 * or a dedicated memory panel. Selectors are best-effort.
 */
function extractMemoryItems() {
  const candidates = document.querySelectorAll(
    '[data-testid*="memory"], [aria-label*="memory"], [aria-label*="Memory"], ' +
      ".memory-item, [class*='memory'] li, [class*='Memory'] li"
  );
  const items = [];
  candidates.forEach((el) => {
    const text = el.innerText?.trim();
    if (text && text.length > 3) {
      items.push(text);
    }
  });
  return items;
}

function importMemories(items) {
  if (items.length === 0) return;
  chrome.runtime.sendMessage({
    type: "IMPORT_MEMORIES",
    payload: { source: SOURCE, memories: items },
  });
}

// --- MutationObserver ---

let lastImportedCount = 0;

function tryImport() {
  const items = extractMemoryItems();
  if (items.length > 0 && items.length !== lastImportedCount) {
    lastImportedCount = items.length;
    importMemories(items);
  }
}

function isMemoryPage() {
  return (
    location.href.includes("settings") ||
    location.href.includes("memory") ||
    document.querySelector('[aria-label*="emory"]') !== null
  );
}

if (isMemoryPage()) {
  tryImport();
  const observer = new MutationObserver(() => tryImport());
  observer.observe(document.body, { childList: true, subtree: true });
}

// SPA navigation watcher
let lastHref = location.href;
const navObserver = new MutationObserver(() => {
  if (location.href !== lastHref) {
    lastHref = location.href;
    lastImportedCount = 0;
    setTimeout(tryImport, 800);
  }
});
navObserver.observe(document.body, { childList: true, subtree: true });
