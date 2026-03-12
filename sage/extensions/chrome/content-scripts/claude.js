// Content script for claude.ai
// Reports session activity and imports memory items from the memory settings page.

const SOURCE = "claude";

// --- Session active beacon ---
chrome.runtime.sendMessage({
  type: "BEHAVIOR_EVENT",
  payload: { source: SOURCE, event: "session_active", url: location.href },
});

// --- Memory extraction helpers ---

/**
 * Extracts visible memory item texts from the current DOM.
 * Claude renders memories as list items inside the memory settings panel.
 */
function extractMemoryItems() {
  // Claude.ai memory page renders items in a scrollable list.
  // Selectors are best-effort and may need updating if the UI changes.
  const candidates = document.querySelectorAll(
    '[data-testid="memory-item"], .memory-item, ul[aria-label*="emor"] li, ol li'
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

// --- MutationObserver to watch for memory items ---

let lastImportedCount = 0;

function tryImport() {
  const items = extractMemoryItems();
  if (items.length > 0 && items.length !== lastImportedCount) {
    lastImportedCount = items.length;
    importMemories(items);
  }
}

// Only activate the observer on settings / memory pages to avoid unnecessary work
function isMemoryPage() {
  return (
    location.href.includes("/settings") ||
    location.href.includes("/memory") ||
    document.title.toLowerCase().includes("memory")
  );
}

if (isMemoryPage()) {
  // Initial pass
  tryImport();

  const observer = new MutationObserver(() => tryImport());
  observer.observe(document.body, { childList: true, subtree: true });
}

// Re-check when the user navigates within the SPA
let lastHref = location.href;
const navObserver = new MutationObserver(() => {
  if (location.href !== lastHref) {
    lastHref = location.href;
    if (isMemoryPage()) {
      lastImportedCount = 0;
      setTimeout(tryImport, 800); // let the SPA render
    }
  }
});
navObserver.observe(document.body, { childList: true, subtree: true });
