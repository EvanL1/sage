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

// --- Sync 按钮：检测已打开的 AI 平台标签页，直接同步 ---

// 平台 → 设置页 URL 映射
const AI_PLATFORMS = [
  { name: "Claude", base: "https://claude.ai/", settings: "https://claude.ai/settings/capabilities?modal=memory" },
  { name: "ChatGPT", base: "https://chatgpt.com/", settings: "https://chatgpt.com/settings" },
  { name: "ChatGPT", base: "https://chat.openai.com/", settings: "https://chat.openai.com/settings" },
  { name: "Gemini", base: "https://gemini.google.com/", settings: "https://gemini.google.com/app/settings" },
];

syncBtn.addEventListener("click", () => {
  syncBtn.disabled = true;
  syncBtn.textContent = "同步中…";

  chrome.tabs.query({}, (tabs) => {
    // 按平台分组，优先找设置页，否则找任意页（后面导航到设置页）
    const byPlatform = new Map(); // name → { tabId, isSettings }
    for (const tab of tabs) {
      if (!tab?.id || !tab.url) continue;
      for (const p of AI_PLATFORMS) {
        if (!tab.url.startsWith(p.base)) continue;
        const isSettings = tab.url.startsWith(p.settings.split("?")[0]);
        const existing = byPlatform.get(p.name);
        // 设置页优先
        if (!existing || (!existing.isSettings && isSettings)) {
          byPlatform.set(p.name, { tabId: tab.id, name: p.name, settings: p.settings, isSettings });
        }
        break;
      }
    }

    if (byPlatform.size === 0) {
      syncBtn.textContent = "未检测到 AI 平台";
      setTimeout(() => { syncBtn.textContent = "Sync Now"; syncBtn.disabled = false; }, 2000);
      return;
    }

    const targets = [...byPlatform.values()];
    syncBtn.textContent = `同步 ${targets.map(t => t.name).join(", ")}…`;

    let done = 0;
    const results = [];
    for (const t of targets) {
      if (t.isSettings) {
        // 已经在设置页，直接触发
        _triggerSync(t.tabId, t.name, results, () => { if (++done >= targets.length) _onAllDone(results); });
      } else {
        // 导航到设置页，等加载后触发
        chrome.tabs.update(t.tabId, { url: t.settings }, () => {
          const onUpdated = (id, info) => {
            if (id !== t.tabId || info.status !== "complete") return;
            chrome.tabs.onUpdated.removeListener(onUpdated);
            // 等 content script 注入 + DOM 渲染
            setTimeout(() => {
              _triggerSync(t.tabId, t.name, results, () => { if (++done >= targets.length) _onAllDone(results); });
            }, 5000);
          };
          chrome.tabs.onUpdated.addListener(onUpdated);
        });
      }
    }
  });

  function _triggerSync(tabId, name, results, cb) {
    chrome.scripting.executeScript({
      target: { tabId },
      func: () => typeof window.__sageSyncMemories === "function" ? window.__sageSyncMemories() : false,
    }, (res) => {
      const ok = !chrome.runtime.lastError && res?.[0]?.result;
      results.push({ name, ok });
      cb();
    });
  }

  function _onAllDone(results) {
    setTimeout(() => {
      chrome.runtime.sendMessage({ type: "CHECK_STATUS" }, (res) => {
        if (res?.ok) {
          setConnected(res.data);
          lastSync.textContent = `Last sync: ${formatTime(new Date().toISOString())}`;
          chrome.storage.local.set({ lastSync: new Date().toISOString() });
        }
      });
      const ok = [...new Set(results.filter(r => r.ok).map(r => r.name))];
      syncBtn.textContent = ok.length > 0 ? `✓ ${ok.join(", ")} 已同步` : "未检测到记忆";
      setTimeout(() => { syncBtn.textContent = "Sync Now"; syncBtn.disabled = false; }, 3000);
    }, 3000);
  }
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

