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

// 将秒数格式化为可读时长，如 "1h 23m" 或 "45m"
function formatDuration(seconds) {
  if (!seconds || seconds < 60) return `${seconds || 0}s`;
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

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

  // 让当前标签的 content script 重新推送记忆
  chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
    const tab = tabs[0];
    if (!tab?.id) {
      syncBtn.disabled = false;
      syncBtn.textContent = "Sync Now";
      return;
    }

    // 短暂延迟后刷新状态计数
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

// 从 storage 加载追踪开关状态（默认开启）
chrome.storage.local.get(["trackingEnabled"], (result) => {
  const enabled = result.trackingEnabled !== false;
  trackingToggle.checked = enabled;
});

// 用户切换追踪开关时，持久化到 storage（background.js 会监听变更）
trackingToggle.addEventListener("change", () => {
  const enabled = trackingToggle.checked;
  chrome.storage.local.set({ trackingEnabled: enabled });
});

// ── 今日统计展示 ──────────────────────────────────────────────────────────────

function renderDailyStats(stats) {
  if (!stats || stats.date !== new Date().toDateString()) {
    // 没有今日数据，显示默认值
    todayPageCount.textContent = "0";
    todayTopDomain.textContent = "—";
    todayActiveTime.textContent = "0m";
    return;
  }

  todayPageCount.textContent = stats.pageCount || 0;

  // 找出停留时间最长的域名
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

// 加载今日统计数据
chrome.storage.local.get(["dailyStats"], (result) => {
  renderDailyStats(result.dailyStats);
});
