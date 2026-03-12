// Content script for chatgpt.com / chat.openai.com
// Reports session activity and imports memory items from
// Settings > Personalization > Memory.

const SOURCE = "chatgpt";

// --- Session active beacon ---
chrome.runtime.sendMessage({
  type: "BEHAVIOR_EVENT",
  payload: { source: SOURCE, event: "session_active", url: location.href },
});

// --- Memory extraction helpers ---

/**
 * Extracts memory item texts from ChatGPT's Personalization > Memory panel.
 * ChatGPT renders each saved memory as a list item with a short description.
 */
function extractMemoryItems() {
  // ChatGPT memory items appear in a modal/dialog list when the user opens
  // Settings > Personalization > Manage Memories.
  const candidates = document.querySelectorAll(
    '[data-testid*="memory"], .memory-item, [class*="Memory"] li, [role="listitem"]'
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
    document.querySelector('[aria-label*="emory"], [aria-label*="Personalization"]') !== null
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

// Also watch for the memory modal being opened dynamically
const modalObserver = new MutationObserver(() => {
  if (isMemoryPage()) {
    setTimeout(tryImport, 500);
  }
});
modalObserver.observe(document.body, { childList: true, subtree: false });
