const statusBadge = document.getElementById("status-badge");
const memoryCount = document.getElementById("memory-count");
const behaviorCount = document.getElementById("behavior-count");
const syncBtn = document.getElementById("sync-btn");
const lastSync = document.getElementById("last-sync");

// 追踪相关元素
const trackingToggle = document.getElementById("tracking-toggle");
const todayPageCount = document.getElementById("today-page-count");
const todayTopDomain = document.getElementById("today-top-domain");
const todayActiveTime = document.getElementById("today-active-time");

// Teams 相关 DOM 元素
const teamsStatusBadge = document.getElementById("teams-status-badge");
const teamsMessageCount = document.getElementById("teams-message-count");
const teamsCaptureLevel = document.getElementById("teams-capture-level");

// --- 工具函数 ---

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

function formatDuration(seconds) {
  if (!seconds || seconds < 60) return `${seconds || 0}s`;
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
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
      const isActive = result.teams_page_active === true;
      if (isActive) {
        teamsStatusBadge.textContent = "已连接";
        teamsStatusBadge.className = "badge badge--connected";
      } else {
        chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
          const tab = tabs[0];
          const isTeamsTab =
            tab?.url?.includes("teams.microsoft.com") ||
            tab?.url?.includes("teams.live.com") ||
            tab?.url?.includes("teams.cloud.microsoft");

          if (isTeamsTab) {
            teamsStatusBadge.textContent = "检测中…";
            teamsStatusBadge.className = "badge badge--checking";
          } else {
            teamsStatusBadge.textContent = "未检测到";
            teamsStatusBadge.className = "badge badge--disconnected";
          }
        });
      }

      const today = new Date().toDateString();
      if (result.teams_today_date === today) {
        teamsMessageCount.textContent = result.teams_today_count ?? 0;
      } else {
        teamsMessageCount.textContent = 0;
      }

      const sendSummary = result.teams_send_content_summary ?? false;
      teamsCaptureLevel.value = sendSummary ? "summary" : "metadata";
    }
  );
}

teamsCaptureLevel.addEventListener("change", () => {
  const sendSummary = teamsCaptureLevel.value === "summary";
  chrome.storage.local.set({ teams_send_content_summary: sendSummary });
});

// --- 检查连接状态（popup 打开时执行）---

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

// --- Sync 按钮 ---

syncBtn.addEventListener("click", () => {
  syncBtn.disabled = true;
  syncBtn.textContent = "Syncing…";

  chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
    const tab = tabs[0];
    if (!tab?.id) {
      syncBtn.disabled = false;
      syncBtn.textContent = "Sync Now";
      return;
    }

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

// --- 恢复上次同步时间 ---

chrome.storage.local.get(["lastSync"], (result) => {
  if (result.lastSync) {
    lastSync.textContent = `Last sync: ${formatTime(result.lastSync)}`;
  }
});

// ── 追踪开关 ──────────────────────────────────────────────────────────────────

chrome.storage.local.get(["trackingEnabled"], (result) => {
  const enabled = result.trackingEnabled !== false;
  trackingToggle.checked = enabled;
});

trackingToggle.addEventListener("change", () => {
  const enabled = trackingToggle.checked;
  chrome.storage.local.set({ trackingEnabled: enabled });
});

// ── 今日统计展示 ──────────────────────────────────────────────────────────────

function renderDailyStats(stats) {
  if (!stats || stats.date !== new Date().toDateString()) {
    todayPageCount.textContent = "0";
    todayTopDomain.textContent = "—";
    todayActiveTime.textContent = "0m";
    return;
  }

  todayPageCount.textContent = stats.pageCount || 0;

  const domainDurations = stats.domainDurations || {};
  const topDomain = Object.entries(domainDurations).sort(
    ([, a], [, b]) => b - a
  )[0];

  if (topDomain) {
    todayTopDomain.textContent = topDomain[0];
    todayTopDomain.title = `${topDomain[0]} · ${formatDuration(topDomain[1])}`;
  } else {
    todayTopDomain.textContent = "—";
  }

  todayActiveTime.textContent = formatDuration(stats.totalActiveSeconds || 0);
}

chrome.storage.local.get(["dailyStats"], (result) => {
  renderDailyStats(result.dailyStats);
});

// --- 初始化 Teams 状态 ---
updateTeamsStatus();
