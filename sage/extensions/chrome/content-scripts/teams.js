// Teams Web 消息捕获内容脚本
// 监听 Microsoft Teams 网页版聊天消息，提取元数据发送到 Sage Bridge
// 兼容 Edge 浏览器

const SOURCE = "teams";

// --- 常量 ---

// 消息去重集合（页面刷新时自动清空）
const processedIds = new Set();

// 今日消息计数（存储到 chrome.storage.local）
let todayMessageCount = 0;

// 是否发送内容摘要（默认关闭，保护隐私）
let sendContentSummary = false;

// 防抖计时器
let debounceTimer = null;
const DEBOUNCE_MS = 500;

// --- 初始化：读取配置 ---

chrome.storage.local.get(
  ["teams_send_content_summary", "teams_today_count", "teams_today_date"],
  (result) => {
    sendContentSummary = result.teams_send_content_summary ?? false;

    // 如果是新的一天，重置计数
    const today = new Date().toDateString();
    if (result.teams_today_date === today) {
      todayMessageCount = result.teams_today_count ?? 0;
    } else {
      todayMessageCount = 0;
      chrome.storage.local.set({
        teams_today_count: 0,
        teams_today_date: today,
      });
    }
  }
);

// --- DOM 选择器工具函数 ---

/**
 * 从多个候选选择器中取第一个匹配的元素文本
 * Teams Web 是 React SPA，选择器随版本变化，需多写 fallback
 */
function queryText(root, selectors) {
  for (const sel of selectors) {
    try {
      const el = root.querySelector(sel);
      if (el) {
        const text = el.innerText?.trim() || el.textContent?.trim();
        if (text) return text;
      }
    } catch (_) {
      // 忽略无效选择器
    }
  }
  return null;
}

/**
 * 获取消息唯一 ID（从 DOM 属性提取）
 */
function getMessageId(el) {
  return (
    el.getAttribute("data-message-id") ||
    el.getAttribute("data-mid") ||
    el.getAttribute("id") ||
    el.getAttribute("data-item-key") ||
    null
  );
}

/**
 * 判断消息类型
 */
function detectMessageType(el) {
  // 文件附件
  if (
    el.querySelector(
      '[data-tid="attachment-card"], .ui-attachment, [aria-label*="file"], [aria-label*="文件"]'
    )
  ) {
    return "file";
  }
  // 会议/通话消息
  if (
    el.querySelector(
      '[data-tid="meeting-card"], .ui-meeting, [aria-label*="meeting"], [aria-label*="会议"], [aria-label*="通话"]'
    )
  ) {
    return "meeting";
  }
  return "text";
}

/**
 * 提取发送者名称
 */
function extractSender(el) {
  return queryText(el, [
    '[data-tid="message-header-name"]',
    ".ui-chat__message__author",
    '[class*="authorName"]',
    '[class*="senderName"]',
    '[aria-label*="sent by"]',
    ".ts-author",
    "[data-scope='message-author']",
    'span[title][class*="author"]',
  ]);
}

/**
 * 提取消息文本内容（前 100 字符）
 */
function extractContent(el) {
  const text = queryText(el, [
    '[data-tid="chat-pane-message"]',
    ".ui-chat__message__content",
    '[class*="messageBody"]',
    '[class*="messageContent"]',
    ".ts-message-content",
    "[data-scope='message-content']",
    "p",
  ]);
  if (!text) return null;
  return text.length > 100 ? text.slice(0, 100) + "…" : text;
}

/**
 * 获取当前频道/聊天名称
 */
function getCurrentChannel() {
  return queryText(document, [
    ".channel-header-title",
    '[data-tid="team-channel-header"]',
    '[class*="channelName"]',
    '[class*="chatName"]',
    '[aria-label*="channel"]',
    'h1[class*="header"]',
    '[class*="threadHeader"] h1',
    '[class*="threadHeader"] span',
    ".ts-channel-header-title",
    'div[role="banner"] h1',
    'div[role="banner"] span[title]',
  ]) || extractChannelFromUrl();
}

/**
 * 从 URL 中提取频道信息作为 fallback
 * 例如 https://teams.microsoft.com/l/channel/...
 */
function extractChannelFromUrl() {
  try {
    const url = new URL(location.href);
    // 尝试从路径或 hash 参数中提取频道标识
    const threadId = url.searchParams.get("threadId") || "";
    if (threadId) {
      // threadId 格式通常是 "19:xxx@thread.xxx"，提取前缀数字
      return `channel_${threadId.slice(0, 8)}`;
    }
  } catch (_) {}
  return "unknown";
}

// --- 消息处理 ---

/**
 * 处理单条消息 DOM 元素
 */
function processMessage(el) {
  const msgId = getMessageId(el);

  // 去重检查
  if (msgId) {
    if (processedIds.has(msgId)) return;
    processedIds.add(msgId);
  }

  const sender = extractSender(el) || "Unknown";
  const channel = getCurrentChannel();
  const timestamp = new Date().toISOString();
  const messageType = detectMessageType(el);

  // 更新计数
  todayMessageCount++;
  chrome.storage.local.set({
    teams_today_count: todayMessageCount,
    teams_today_date: new Date().toDateString(),
  });

  // 发送行为事件（始终发送元数据）
  chrome.runtime.sendMessage({
    type: "BEHAVIOR_EVENT",
    payload: {
      source: SOURCE,
      event_type: "message_received",
      metadata: {
        sender,
        channel,
        timestamp,
        message_type: messageType,
      },
    },
  });

  // 可选：发送内容摘要记忆（需用户配置开启，且只对文字消息）
  if (sendContentSummary && messageType === "text") {
    const content = extractContent(el);
    if (content && content.length > 5) {
      const topicHint = content.slice(0, 50);
      chrome.runtime.sendMessage({
        type: "IMPORT_MEMORIES",
        payload: {
          source: SOURCE,
          memories: [
            {
              category: "communication",
              content: `与 ${sender} 在 ${channel} 讨论了「${topicHint}」`,
              confidence: 0.6,
            },
          ],
        },
      });
    }
  }
}

/**
 * 扫描当前 DOM 中所有消息列表项
 */
function scanMessages() {
  const messageEls = document.querySelectorAll(
    [
      '[data-tid="message-pane-list-item"]',
      '[class*="message-list-item"]',
      '[class*="messageListItem"]',
      ".ui-chat__item",
      '[data-scope="message"]',
      '[class*="chatMessage"]',
    ].join(",")
  );

  messageEls.forEach((el) => processMessage(el));
}

// --- MutationObserver：监听新消息 DOM 节点 ---

function handleMutations(mutations) {
  let hasNewNodes = false;

  for (const mutation of mutations) {
    if (mutation.type !== "childList") continue;
    for (const node of mutation.addedNodes) {
      if (node.nodeType !== Node.ELEMENT_NODE) continue;

      // 检查新节点自身是否是消息列表项
      const isMessageItem =
        node.matches?.(
          [
            '[data-tid="message-pane-list-item"]',
            '[class*="message-list-item"]',
            '[class*="messageListItem"]',
            ".ui-chat__item",
            '[data-scope="message"]',
            '[class*="chatMessage"]',
          ].join(",")
        ) || false;

      // 检查新节点内部是否包含消息列表项
      const containsMessages =
        node.querySelector?.(
          [
            '[data-tid="message-pane-list-item"]',
            '[class*="message-list-item"]',
            '[class*="messageListItem"]',
            ".ui-chat__item",
          ].join(",")
        ) !== null;

      if (isMessageItem || containsMessages) {
        hasNewNodes = true;
        break;
      }
    }
    if (hasNewNodes) break;
  }

  if (!hasNewNodes) return;

  // 防抖：500ms 内合并多条消息
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(scanMessages, DEBOUNCE_MS);
}

// 启动 MutationObserver，等待 body 就绪
function startObserver() {
  const observer = new MutationObserver(handleMutations);
  observer.observe(document.body, {
    childList: true,
    subtree: true,
  });
  return observer;
}

// --- SPA 路由变化检测 ---

let lastHref = location.href;

function checkUrlChange() {
  if (location.href !== lastHref) {
    lastHref = location.href;
    // 导航到新频道，清空去重集合（新页面消息都是新的）
    processedIds.clear();
    // 延迟等待 React 渲染完成
    setTimeout(scanMessages, 1000);
  }
}

const urlObserver = new MutationObserver(checkUrlChange);

// --- 入口 ---

// 确保 DOM 就绪后再启动
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", () => {
    startObserver();
    urlObserver.observe(document.body, { childList: true, subtree: true });
    // 初次扫描（页面已有消息）
    setTimeout(scanMessages, 1500);
  });
} else {
  startObserver();
  urlObserver.observe(document.body, { childList: true, subtree: true });
  setTimeout(scanMessages, 1500);
}

// 通知 popup：当前页面是 Teams
chrome.storage.local.set({ teams_page_active: true });

// 页面卸载时清除标记
window.addEventListener("beforeunload", () => {
  chrome.storage.local.set({ teams_page_active: false });
});
