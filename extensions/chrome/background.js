const SAGE_API = "http://127.0.0.1:18522";

// ── 常量配置 ─────────────────────────────────────────────────────────────────

// 内部页面前缀，不追踪
const IGNORED_SCHEMES = [
  "chrome://",
  "chrome-extension://",
  "edge://",
  "about:",
  "data:",
  "javascript:",
  "moz-extension://",
];

// 深度专注判定：同一域名累计停留超过此秒数
const DEEP_FOCUS_SECONDS = 10 * 60; // 10 分钟

// 频繁切换判定：5 分钟内不同域名数量超过此值
const CONTEXT_SWITCH_THRESHOLD = 8;

// 聚合分析间隔（毫秒）
const PATTERN_INTERVAL_MS = 5 * 60 * 1000; // 5 分钟

// 深夜时段（小时，闭区间）
const LATE_NIGHT_START = 22;
const LATE_NIGHT_END = 6;

// ── API 工具函数 ──────────────────────────────────────────────────────────────

async function callApi(path, method, body) {
  const options = { method, headers: { "Content-Type": "application/json" } };
  if (body !== undefined) {
    options.body = JSON.stringify(body);
  }
  const res = await fetch(`${SAGE_API}${path}`, options);
  if (!res.ok) {
    throw new Error(`Sage API ${method} ${path} returned ${res.status}`);
  }
  return res.json();
}

// ── 隐私过滤工具 ──────────────────────────────────────────────────────────────

// 判断 URL 是否应被忽略（内部页面）
function isIgnoredUrl(url) {
  if (!url) return true;
  return IGNORED_SCHEMES.some((scheme) => url.startsWith(scheme));
}

// 从 URL 中提取域名，去掉路径和参数（保护隐私）
function extractDomain(url) {
  try {
    const u = new URL(url);
    return u.hostname;
  } catch {
    return null;
  }
}

// ── 追踪状态（内存中，Service Worker 生命周期内有效）─────────────────────────

// 当前激活的标签信息：{ tabId, url, domain, title, activatedAt }
let currentTab = null;

// 最近 5 分钟内的页面访问记录，用于模式分析
// 格式：[{ domain, activatedAt, duration }]
let recentVisits = [];

// ── 追踪开关 —— 从 storage 异步加载 ─────────────────────────────────────────

let trackingEnabled = true;

chrome.storage.local.get(["trackingEnabled"], (result) => {
  // 默认开启；用户可在 popup 中关闭
  trackingEnabled = result.trackingEnabled !== false;
});

// 监听 storage 变更，实时同步追踪开关状态
chrome.storage.onChanged.addListener((changes) => {
  if (changes.trackingEnabled !== undefined) {
    trackingEnabled = changes.trackingEnabled.newValue !== false;
  }
});

// ── 发送行为事件 ──────────────────────────────────────────────────────────────

async function sendBehaviorEvent(eventType, metadata) {
  try {
    await callApi("/api/behaviors", "POST", {
      source: "browser",
      event_type: eventType,
      metadata,
    });
  } catch (err) {
    // 静默失败，不影响用户浏览
    console.warn("[Sage] 发送行为事件失败:", err.message);
  }
}

// ── 停留时长结算 ──────────────────────────────────────────────────────────────

// 结算当前标签的停留时长，并发送 page_visit 事件
function flushCurrentTab() {
  if (!currentTab) return;

  const now = Date.now();
  const duration = Math.round((now - currentTab.activatedAt) / 1000);

  // 至少停留 2 秒才记录，过滤误触
  if (duration >= 2 && currentTab.domain) {
    const visit = {
      domain: currentTab.domain,
      duration_seconds: duration,
      title: currentTab.title || "",
      activatedAt: currentTab.activatedAt,
    };

    // 追加到近期访问列表
    recentVisits.push(visit);

    // 只保留最近 5 分钟的记录
    const cutoff = now - PATTERN_INTERVAL_MS;
    recentVisits = recentVisits.filter((v) => v.activatedAt >= cutoff);

    if (trackingEnabled) {
      sendBehaviorEvent("page_visit", {
        url: currentTab.domain, // 只发域名，不发完整路径
        domain: currentTab.domain,
        duration_seconds: duration,
        title: currentTab.title || "",
      });
      updateDailyStats(currentTab.domain, duration);
    }
  }

  currentTab = null;
}

// ── 更新今日统计（写入 storage 供 popup 读取）─────────────────────────────────

async function updateDailyStats(domain, duration) {
  const todayKey = new Date().toDateString();
  const result = await chrome.storage.local.get(["dailyStats"]);
  const stats = result.dailyStats || {};

  // 如果是新的一天，重置统计
  if (stats.date !== todayKey) {
    stats.date = todayKey;
    stats.pageCount = 0;
    stats.domainDurations = {};
    stats.totalActiveSeconds = 0;
  }

  stats.pageCount = (stats.pageCount || 0) + 1;
  stats.totalActiveSeconds = (stats.totalActiveSeconds || 0) + duration;

  if (domain) {
    stats.domainDurations = stats.domainDurations || {};
    stats.domainDurations[domain] =
      (stats.domainDurations[domain] || 0) + duration;
  }

  await chrome.storage.local.set({ dailyStats: stats });
}

// ── 标签激活监听 ──────────────────────────────────────────────────────────────

chrome.tabs.onActivated.addListener(async (activeInfo) => {
  // 结算上一个标签
  flushCurrentTab();

  try {
    const tab = await chrome.tabs.get(activeInfo.tabId);
    if (isIgnoredUrl(tab.url)) return;

    const domain = extractDomain(tab.url);
    if (!domain) return;

    currentTab = {
      tabId: activeInfo.tabId,
      url: tab.url,
      domain,
      title: tab.title || "",
      activatedAt: Date.now(),
    };
  } catch (err) {
    // 标签可能已关闭
    console.warn("[Sage] 获取标签信息失败:", err.message);
  }
});

// ── 页面导航完成监听（单标签内页面跳转）─────────────────────────────────────

chrome.webNavigation.onCompleted.addListener((details) => {
  // 只处理主框架导航（frame 0）
  if (details.frameId !== 0) return;
  if (isIgnoredUrl(details.url)) return;

  const domain = extractDomain(details.url);
  if (!domain) return;

  // 如果是当前激活标签内的导航，结算旧页面并开始新计时
  if (currentTab && currentTab.tabId === details.tabId) {
    flushCurrentTab();

    // 获取最新标题（导航完成后标题可能已更新）
    chrome.tabs.get(details.tabId).then((tab) => {
      currentTab = {
        tabId: details.tabId,
        url: details.url,
        domain,
        title: tab.title || "",
        activatedAt: Date.now(),
      };
    }).catch(() => {
      // 标签关闭时忽略
    });
  }
});

// ── 标签关闭监听 ──────────────────────────────────────────────────────────────

chrome.tabs.onRemoved.addListener((tabId) => {
  if (currentTab && currentTab.tabId === tabId) {
    flushCurrentTab();
  }
});

// ── 活动模式分析（每 5 分钟聚合一次）────────────────────────────────────────

function analyzeActivityPattern() {
  if (!trackingEnabled) return;

  const now = Date.now();
  const cutoff = now - PATTERN_INTERVAL_MS;
  const window = recentVisits.filter((v) => v.activatedAt >= cutoff);

  if (window.length === 0) return;

  // 统计各域名的累计停留时长
  const domainTotals = {};
  for (const visit of window) {
    domainTotals[visit.domain] =
      (domainTotals[visit.domain] || 0) + visit.duration_seconds;
  }

  const domains = Object.keys(domainTotals);
  const hour = new Date().getHours();
  const isLateNight =
    hour >= LATE_NIGHT_START || hour < LATE_NIGHT_END;

  // 检测深度专注：单一域名停留超过 10 分钟
  for (const [domain, totalSecs] of Object.entries(domainTotals)) {
    if (totalSecs >= DEEP_FOCUS_SECONDS) {
      const metadata = {
        pattern: "deep_focus",
        domain,
        duration_seconds: totalSecs,
        domains,
      };
      if (isLateNight) metadata.late_night = true;

      sendBehaviorEvent("activity_pattern", metadata);
      return; // 深度专注优先，只报告一次
    }
  }

  // 检测频繁切换：5 分钟内不同域名超过 8 个
  if (domains.length > CONTEXT_SWITCH_THRESHOLD) {
    const metadata = {
      pattern: "context_switching",
      domain_count: domains.length,
      domains,
      switch_rate: (domains.length / 5).toFixed(1), // 每分钟切换次数
    };
    if (isLateNight) metadata.late_night = true;

    sendBehaviorEvent("activity_pattern", metadata);
  }
}

// 启动定时聚合分析
setInterval(analyzeActivityPattern, PATTERN_INTERVAL_MS);

// ── 消息处理（来自 popup / content scripts）──────────────────────────────────

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  const { type, payload } = message;

  if (type === "IMPORT_MEMORIES") {
    // POST 到 /api/memories — daemon 直接写入 memories 表（dedup 处理）
    const importPayload = {
      source: payload.source || "browser",
      memories: payload.memories || [],
    };
    callApi("/api/memories", "POST", importPayload)
      .then((data) => sendResponse({ ok: true, data }))
      .catch((err) => sendResponse({ ok: false, error: err.message }));
    return true;
  }

  if (type === "BEHAVIOR_EVENT") {
    callApi("/api/behaviors", "POST", payload)
      .then((data) => sendResponse({ ok: true, data }))
      .catch((err) => sendResponse({ ok: false, error: err.message }));
    return true;
  }

  if (type === "CHECK_STATUS") {
    callApi("/api/status", "GET")
      .then((data) => sendResponse({ ok: true, data }))
      .catch((err) => sendResponse({ ok: false, error: err.message }));
    return true;
  }

  if (type === "FETCH_CONTEXT") {
    const limit = payload?.limit || 10;
    callApi(`/api/context?limit=${limit}`, "GET")
      .then((data) => sendResponse({ ok: true, data }))
      .catch((err) => sendResponse({ ok: false, error: err.message }));
    return true;
  }

  // 未知消息类型，立即关闭端口
  return false;
});
