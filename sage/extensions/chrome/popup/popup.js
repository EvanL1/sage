const statusBadge = document.getElementById("status-badge");
const memoryCount = document.getElementById("memory-count");
const behaviorCount = document.getElementById("behavior-count");
const syncBtn = document.getElementById("sync-btn");
const lastSync = document.getElementById("last-sync");

// --- Helpers ---

function setConnected(data) {
  statusBadge.textContent = "Connected";
  statusBadge.className = "badge badge--connected";
  syncBtn.disabled = false;

  if (data) {
    memoryCount.textContent = data.memory_count ?? "—";
    behaviorCount.textContent = data.behavior_count ?? "—";
  }
}

function setDisconnected(reason) {
  statusBadge.textContent = "Disconnected";
  statusBadge.className = "badge badge--disconnected";
  syncBtn.disabled = true;
  memoryCount.textContent = "—";
  behaviorCount.textContent = "—";
  if (reason) console.warn("Sage Bridge disconnected:", reason);
}

function formatTime(iso) {
  if (!iso) return "never";
  try {
    return new Date(iso).toLocaleTimeString();
  } catch {
    return iso;
  }
}

// --- Check connection on popup open ---

chrome.runtime.sendMessage({ type: "CHECK_STATUS" }, (response) => {
  if (chrome.runtime.lastError) {
    setDisconnected(chrome.runtime.lastError.message);
    return;
  }
  if (response?.ok) {
    setConnected(response.data);
    lastSync.textContent = `Last sync: ${formatTime(response.data?.last_sync)}`;
  } else {
    setDisconnected(response?.error);
  }
});

// --- Sync button ---

syncBtn.addEventListener("click", () => {
  syncBtn.disabled = true;
  syncBtn.textContent = "Syncing…";

  // Ask the active tab's content script to re-scan and push memories
  chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
    const tab = tabs[0];
    if (!tab?.id) {
      syncBtn.disabled = false;
      syncBtn.textContent = "Sync Now";
      return;
    }

    // Re-check status after a brief delay to refresh counts
    setTimeout(() => {
      chrome.runtime.sendMessage({ type: "CHECK_STATUS" }, (res) => {
        syncBtn.textContent = "Sync Now";
        if (res?.ok) {
          setConnected(res.data);
          lastSync.textContent = `Last sync: ${formatTime(new Date().toISOString())}`;
          chrome.storage.local.set({ lastSync: new Date().toISOString() });
        } else {
          setDisconnected(res?.error);
          syncBtn.disabled = false;
        }
      });
    }, 1200);
  });
});

// --- Restore last sync time from storage ---

chrome.storage.local.get(["lastSync"], (result) => {
  if (result.lastSync) {
    lastSync.textContent = `Last sync: ${formatTime(result.lastSync)}`;
  }
});
