const statusBadge = document.getElementById("status-badge");
const memoryCount = document.getElementById("memory-count");
const behaviorCount = document.getElementById("behavior-count");
const syncBtn = document.getElementById("sync-btn");
const lastSync = document.getElementById("last-sync");

// Teams 相关 DOM 元素
const teamsStatusBadge = document.getElementById("teams-status-badge");
const teamsMessageCount = document.getElementById("teams-message-count");
const teamsCaptureLevel = document.getElementById("teams-capture-level");

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

// --- Teams 状态更新 ---

function updateTeamsStatus() {
  chrome.storage.local.get(
    [
      "teams_page_active",
      "teams_today_count",
      "teams_today_date",
      "teams_send_content_summary",
    ],
    (result) => {
      // 检测 Teams 页面是否活跃
      const isActive = result.teams_page_active === true;
      if (isActive) {
        teamsStatusBadge.textContent = "已连接";
        teamsStatusBadge.className = "badge badge--connected";
      } else {
        // 进一步检查当前 active tab 是否是 Teams
        chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
          const tab = tabs[0];
          const isTeamsTab =
            tab?.url?.includes("teams.microsoft.com") ||
            tab?.url?.includes("teams.live.com");

          if (isTeamsTab) {
            teamsStatusBadge.textContent = "检测中…";
            teamsStatusBadge.className = "badge badge--checking";
          } else {
            teamsStatusBadge.textContent = "未检测到";
            teamsStatusBadge.className = "badge badge--disconnected";
          }
        });
      }

      // 更新今日消息计数（如果是今天的数据）
      const today = new Date().toDateString();
      if (result.teams_today_date === today) {
        teamsMessageCount.textContent = result.teams_today_count ?? 0;
      } else {
        teamsMessageCount.textContent = 0;
      }

      // 恢复捕获级别选择
      const sendSummary = result.teams_send_content_summary ?? false;
      teamsCaptureLevel.value = sendSummary ? "summary" : "metadata";
    }
  );
}

// --- Teams 捕获级别设置 ---

teamsCaptureLevel.addEventListener("change", () => {
  const sendSummary = teamsCaptureLevel.value === "summary";
  chrome.storage.local.set({ teams_send_content_summary: sendSummary });
});

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

// --- 初始化 Teams 状态 ---
updateTeamsStatus();
