// Lightweight i18n — no library dependency
// Language is read from UserProfile.prompt_language ("zh" | "en")

type Lang = "zh" | "en";

// Translation dictionary — all strings for Tasks page + common UI
const translations = {
  // Common
  "done": { zh: "完成", en: "Done" },
  "cancel": { zh: "取消", en: "Cancel" },
  "delete": { zh: "删除", en: "Delete" },
  "save": { zh: "保存", en: "Save" },
  "skip": { zh: "跳过", en: "Skip" },
  "close": { zh: "关闭", en: "Close" },
  "edit": { zh: "编辑", en: "Edit" },
  "reopen": { zh: "重新打开", en: "Reopen" },
  "loading": { zh: "加载中…", en: "Loading..." },
  "confirm": { zh: "确认", en: "Confirm" },
  "dismiss": { zh: "忽略", en: "Dismiss" },
  "search": { zh: "搜索", en: "Search" },
  "noData": { zh: "暂无数据", en: "No data" },
  "add": { zh: "添加", en: "Add" },

  // Tasks page
  "tasks.title": { zh: "任务", en: "Tasks" },
  "tasks.add": { zh: "+ 添加", en: "+ Add" },
  "tasks.open": { zh: "待办", en: "Open" },
  "tasks.overdue": { zh: "已过期", en: "Overdue" },
  "tasks.today": { zh: "今天", en: "Today" },
  "tasks.thisWeek": { zh: "本周", en: "This Week" },
  "tasks.later": { zh: "以后", en: "Later" },
  "tasks.noDate": { zh: "无日期", en: "No Date" },
  "tasks.noTasks": { zh: "暂无任务", en: "No tasks" },
  "tasks.showAll": { zh: "显示全部", en: "Show all" },
  "tasks.extractFrom": { zh: "从报告提取任务", en: "Extract tasks from" },
  "tasks.amBrief": { zh: "晨间简报", en: "AM Brief" },
  "tasks.pmReview": { zh: "晚间回顾", en: "PM Review" },
  "tasks.weekly": { zh: "周报", en: "Weekly" },
  "tasks.weekStart": { zh: "周初提醒", en: "Week Start" },
  "tasks.checkboxDone": { zh: "完成（右键：取消）", en: "Done (right-click: Cancel)" },
  "tasks.checkboxReopen": { zh: "重新打开", en: "Reopen" },
  "tasks.cancelled": { zh: "已取消", en: "Cancelled" },

  // Task form
  "taskForm.newTask": { zh: "新建任务", en: "New Task" },
  "taskForm.editTask": { zh: "编辑任务", en: "Edit Task" },
  "taskForm.titlePlaceholder": { zh: "任务标题", en: "Task title" },
  "taskForm.descPlaceholder": { zh: "描述（可选）…", en: "Description (optional)…" },
  "taskForm.priority": { zh: "优先级", en: "Priority" },
  "taskForm.normal": { zh: "普通", en: "Normal" },
  "taskForm.dueDate": { zh: "截止日期", en: "Due Date" },
  "taskForm.addTask": { zh: "添加任务", en: "Add Task" },

  // Date picker
  "datePicker.today": { zh: "今天", en: "Today" },
  "datePicker.tomorrow": { zh: "明天", en: "Tomorrow" },
  "datePicker.nextMon": { zh: "下周一", en: "Next Mon" },
  "datePicker.noDate": { zh: "无日期", en: "No date" },
  "datePicker.pickDate": { zh: "选择日期", en: "Pick date" },

  // Completion dialog
  "completion.completing": { zh: "完成确认", en: "Completing" },
  "completion.cancelling": { zh: "取消确认", en: "Cancelling" },
  "completion.typeAnswer": { zh: "或输入你的回答…", en: "or type your answer…" },
  "completion.notes": { zh: "补充说明（可选）…", en: "Additional notes (optional)…" },
  "completion.due": { zh: "截止", en: "Due" },
  "completion.source": { zh: "来源", en: "Source" },
  "completion.created": { zh: "创建于", en: "Created" },

  // Task detail panel
  "detail.source": { zh: "来源", en: "Source" },
  "detail.created": { zh: "创建于", en: "Created" },
  "detail.status": { zh: "状态", en: "Status" },
  "detail.outcome": { zh: "结果", en: "Outcome" },
  "detail.descPlaceholder": { zh: "点击添加描述…", en: "Click to add description…" },
  "detail.descEdit": { zh: "支持 Markdown…", en: "Supports Markdown…" },
  "detail.aiSuggestsDone": { zh: "AI 建议：完成", en: "AI suggests: Done" },
  "detail.aiSuggestsCancel": { zh: "AI 建议：取消", en: "AI suggests: Cancel" },
  "detail.aiSuggestsNew": { zh: "AI 建议：新任务", en: "AI suggests: New" },

  // Fallback completion questions
  "fallback.doneQ": { zh: "完成情况？", en: "How did it go?" },
  "fallback.doneAsPlanned": { zh: "按计划完成", en: "As planned" },
  "fallback.donePartially": { zh: "部分完成", en: "Partially" },
  "fallback.doneDelegated": { zh: "委托他人", en: "Delegated" },
  "fallback.doneDifferent": { zh: "换了方式", en: "Different approach" },
  "fallback.cancelQ": { zh: "取消原因？", en: "Why cancelled?" },
  "fallback.cancelIrrelevant": { zh: "已不相关", en: "No longer relevant" },
  "fallback.cancelBlocked": { zh: "被阻塞", en: "Blocked" },
  "fallback.cancelDeprioritized": { zh: "降级了", en: "Deprioritized" },
  "fallback.cancelMerged": { zh: "合并到其他任务", en: "Merged" },
  "fallback.cancelSomeoneElse": { zh: "别人处理了", en: "Someone else" },

  // Tasks gen messages
  "tasks.done": { zh: "已完成", en: "Done" },
  "tasks.extracting": { zh: "正在提取…", en: "Extracting..." },
  "tasks.created": { zh: "已创建", en: "created" },
  "tasks.noNew": { zh: "未提取到新任务", en: "No new tasks extracted" },
  "tasks.failed": { zh: "失败", en: "Failed" },

  // Dashboard — layout names
  "dashboard.layoutCommand": { zh: "指令", en: "Command" },
  "dashboard.layoutNebula": { zh: "星云", en: "Nebula" },
  "dashboard.layoutClassic": { zh: "经典", en: "Classic" },

  // Dashboard — stats bar
  "dashboard.statMem": { zh: "记忆", en: "MEM" },
  "dashboard.statLink": { zh: "连接", en: "LINK" },
  "dashboard.statConv": { zh: "对话", en: "CONV" },

  // Dashboard — depth labels
  "dashboard.depthBeliefs": { zh: "信念", en: "Beliefs" },
  "dashboard.depthJudgments": { zh: "判断", en: "Judgments" },
  "dashboard.depthPatterns": { zh: "模式", en: "Patterns" },
  "dashboard.depthEvents": { zh: "事件", en: "Events" },

  // Dashboard — model selector
  "dashboard.noProvider": { zh: "无 Provider", en: "No provider" },
  "dashboard.providerActive": { zh: "活跃", en: "active" },

  // Dashboard — report buttons
  "dashboard.reportAm": { zh: "晨", en: "AM" },
  "dashboard.reportPm": { zh: "晚", en: "PM" },
  "dashboard.reportWk": { zh: "周", en: "WK" },

  // Dashboard — chat nav
  "dashboard.chat": { zh: "聊天", en: "Chat" },

  // Dashboard — layout error boundary
  "dashboard.layoutError": { zh: "布局错误", en: "LAYOUT ERROR" },
  "dashboard.resetLayout": { zh: "重置为默认", en: "Reset to Control" },

  // Dashboard — correction overlay
  "dashboard.cancelCorrection": { zh: "取消纠正", en: "Cancel Correction" },
  "dashboard.correct": { zh: "纠正", en: "Correct" },
  "dashboard.discuss": { zh: "讨论", en: "Discuss" },
  "dashboard.corrWrongPlaceholder": { zh: "Sage 说错了什么（摘录原文）", en: "What Sage got wrong (quote)" },
  "dashboard.corrFactPlaceholder": { zh: "实际正确情况是", en: "The actual correct fact" },
  "dashboard.corrHintPlaceholder": { zh: "关键词标签（可选）", en: "Keyword hint (optional)" },
  "dashboard.submitCorrection": { zh: "提交校准", en: "Submit Correction" },

  // Widget — report
  "widget.reportEmpty": { zh: "点 AM / PM / WK 生成报告", en: "Click AM / PM / WK to generate report" },

  // Widget — tags
  "widget.noTags": { zh: "无标签", en: "No tags" },

  // Widget — sessions
  "widget.noSessions": { zh: "暂无对话", en: "No conversations" },

  // Widget — memories
  "widget.noMemories": { zh: "暂无记忆", en: "No memories" },

  // Widget — question
  "widget.noQuestion": { zh: "今天暂无问题", en: "No question for today" },

  // Widget — connections
  "widget.noConnections": { zh: "无连接信息", en: "No connection info" },

  // Widget — messages
  "widget.noMessages": { zh: "暂无消息", en: "No messages" },

  // Widget — tasks
  "widget.noTasks": { zh: "无待办", en: "No tasks" },
  "widget.taskPlaceholder": { zh: "新建任务…", en: "New task..." },
  "widget.viewAllTasks": { zh: "查看全部任务 →", en: "View all tasks →" },
  "widget.suggestionsPending": { zh: "条建议待处理 →", en: "suggestion(s) pending →" },
  "widget.completing": { zh: "完成确认", en: "Completing" },
  "widget.due": { zh: "截止：", en: "Due:" },
  "widget.source": { zh: "来源：", en: "Source:" },
  "widget.skipBtn": { zh: "跳过", en: "Skip" },
  "widget.saveBtn": { zh: "保存", en: "Save" },
  "widget.notesPlaceholder": { zh: "补充说明…", en: "Additional notes…" },
  "widget.completedAsPlanned": { zh: "按计划完成", en: "Completed as planned" },
  "widget.partiallyDone": { zh: "部分完成", en: "Partially done" },
  "widget.delegated": { zh: "委托他人", en: "Delegated" },

  // Widget — news
  "widget.noNews": { zh: "暂无新闻", en: "No news" },
  "widget.refresh": { zh: "↻ 刷新", en: "↻ Refresh" },
  "widget.fetching": { zh: "获取中…", en: "Fetching…" },

  // Widget — pinned
  "widget.noPinned": { zh: "从星云视图钉住卡片", en: "Pin cards from the Nebula view" },

  // Widget — nebula speed labels
  "widget.speedFast": { zh: "快速", en: "Fast" },
  "widget.speedNormal": { zh: "正常", en: "Normal" },
  "widget.speedSlow": { zh: "慢速", en: "Slow" },

  // Command layout — KPI bar
  "widget.sageOnline": { zh: "Sage 在线", en: "Sage Online" },
  "widget.kpiMemories": { zh: "记忆", en: "Memories" },
  "widget.kpiLinks": { zh: "连接", en: "Links" },
  "widget.kpiSessions": { zh: "对话", en: "Sessions" },
  "widget.kpiPeople": { zh: "人物", en: "People" },

  // Command layout — widget picker
  "widget.pickerTitle": { zh: "小部件", en: "Widgets" },
  "widget.hideWidget": { zh: "隐藏", en: "Hide" },
  "widget.addRemoveWidgets": { zh: "添加/移除小部件", en: "Add / remove widgets" },

  // MessageFlow page
  "msg.loadingGraph": { zh: "加载图谱…", en: "Loading graph..." },
  "msg.people": { zh: "人", en: "people" },
  "msg.relationships": { zh: "条关系", en: "relationships" },
  "msg.nodeSizeHint": { zh: "节点大小 = 连接数，边宽 = 消息量", en: "Node size = connections, edge width = message volume" },
  "msg.noCommData": { zh: "暂无通讯数据。需要共享频道中至少 2 人。", en: "No communication data yet. Messages need at least 2 people in a shared channel." },
  "msg.list": { zh: "列表", en: "List" },
  "msg.graph": { zh: "图谱", en: "Graph" },
  "msg.commRelationships": { zh: "通讯关系", en: "Communication relationships" },
  "msg.all": { zh: "全部", en: "ALL" },
  "msg.in": { zh: "收到", en: "IN" },
  "msg.out": { zh: "发出", en: "OUT" },
  "msg.searchMessages": { zh: "搜索消息…", en: "Search messages..." },
  "msg.thinking": { zh: "思考中…", en: "Thinking..." },
  "msg.summarize": { zh: "总结", en: "Summarize" },
  "msg.me": { zh: "我", en: "Me" },
  "msg.group": { zh: "群组", en: "Group" },
  "msg.channel": { zh: "频道", en: "Channel" },
  "msg.yesterday": { zh: "昨天", en: "Yesterday" },
  "msg.failedSummary": { zh: "生成总结失败。", en: "Failed to generate summary." },
  "msg.actionItems": { zh: "行动项", en: "Action Items" },
  "msg.taskCreated": { zh: "个任务已创建", en: "task(s) created" },
  "msg.convSummary": { zh: "对话总结", en: "Conversation Summary" },
  "msg.noChannels": { zh: "暂无频道。安装 Chrome 扩展以捕获消息。", en: "No channels yet. Install the Chrome extension to capture messages." },
  "msg.noChannelsForSource": { zh: "此来源暂无频道。", en: "No channels for this source." },
  "msg.noMatchingMsgs": { zh: "无匹配消息", en: "No matching messages" },
  "msg.noMsgsInChannel": { zh: "此频道暂无消息", en: "No messages in this channel" },
  "msg.noContent": { zh: "[无内容]", en: "[no content]" },
  "msg.graphView": { zh: "图谱视图", en: "Graph view" },
  "msg.msgsCount": { zh: "条消息", en: "msgs" },
  "msg.chCount": { zh: "个频道", en: "ch" },
  "msg.connection": { zh: "个连接", en: "connection" },
  "msg.connectionsPlural": { zh: "个连接", en: "connections" },

  // Welcome / Onboarding page
  "welcome.getStarted": { zh: "开始", en: "Get started" },
  "welcome.continue": { zh: "继续", en: "Continue" },
  "welcome.yourName": { zh: "你的名字", en: "Your name" },
  "welcome.describeYourself": { zh: "你会怎么描述自己？", en: "How would you describe yourself?" },
  "welcome.tagsOptional": { zh: "标签是可选的，可以跳过", en: "Tags are optional, feel free to skip" },
  "welcome.previous": { zh: "← 上一题", en: "← Previous" },
  "welcome.skipAssessment": { zh: "跳过评测 →", en: "Skip assessment →" },
  "welcome.spectrumNote": { zh: "每个人都是独特的组合。\n这不是标签——而是自我了解的起点。", en: "Everyone is a unique combination.\nThis isn't a label — it's a starting point for self-understanding." },
  "welcome.aiProviderLabel": { zh: "AI 提供商", en: "AI Provider" },
  "welcome.selectProvider": { zh: "选择 AI 提供商…", en: "Select an AI provider..." },
  "welcome.apiKeyLabel": { zh: "API Key", en: "API Key" },
  "welcome.testing": { zh: "测试中…", en: "Testing..." },
  "welcome.testConnection": { zh: "测试连接", en: "Test connection" },
  "welcome.connSuccess": { zh: "连接成功", en: "Connection successful" },
  "welcome.connFail": { zh: "出了点问题，再试一次？", en: "Something went wrong, try again?" },
  "welcome.submitting": { zh: "设置中…", en: "Setting up..." },
  "welcome.beginJourney": { zh: "开始我们的旅程", en: "Begin our journey" },
  "welcome.errorRetry": { zh: "出了点问题，再试一次？", en: "Something went wrong, try again?" },
  "welcome.detectedProvider": { zh: "已检测到", en: "Detected" },
  "welcome.readyToUse": { zh: "—— 可以使用", en: "— ready to use" },
  "welcome.greet1a": { zh: "首先——", en: "First —" },
  "welcome.greet1b": { zh: "你叫什么名字？", en: "what's your name?" },
  "welcome.greet4a": { zh: "好的，我会记住。", en: "Got it. I'll remember that." },
  "welcome.greet4b": { zh: "最后一步——让我连接到我的思考能力——", en: "One last step — let me connect to my thinking capabilities —" },
  "welcome.thinkingSpectrum": { zh: "思维光谱", en: "thinking spectrum" },
  "welcome.spectrumTitle": { zh: "，这是你的思维光谱——", en: ", here's your thinking spectrum —" },

  // FeedIntelligence page
  "feed.searchPlaceholder": { zh: "搜索 Feed…", en: "Search feeds..." },
  "feed.byScore": { zh: "按分数", en: "By Score" },
  "feed.byTime": { zh: "按时间", en: "By Time" },
  "feed.grid": { zh: "网格", en: "Grid" },
  "feed.list": { zh: "列表", en: "List" },
  "feed.fetching": { zh: "获取中…", en: "Fetching..." },
  "feed.fetchNow": { zh: "立即获取", en: "Fetch Now" },
  "feed.noItems": { zh: "暂无 Feed 条目。", en: "No feed items yet." },
  "feed.configureHint": { zh: "点击上方齿轮图标配置并启用 Feed 源", en: "Click the gear icon above to configure and enable feed sources" },
  "feed.fetchHint": { zh: "点击「立即获取」开始获取", en: "Click Fetch Now to start fetching" },
  "feed.insight": { zh: "洞见", en: "Insight" },
  "feed.nextStep": { zh: "下一步", en: "Next Step" },
  "feed.unsavedChanges": { zh: "未保存的更改", en: "Unsaved changes" },
  "feed.saving": { zh: "保存中…", en: "Saving..." },
  "feed.saveApply": { zh: "保存并应用", en: "Save & Apply" },
  "feed.configFootnote": { zh: "更改在下次获取时生效。自动轮询间隔变更需重启 Sage。", en: "Changes take effect on next Fetch. Restart Sage for auto-polling interval changes." },
  "feed.userInterestsLabel": { zh: "用户兴趣（LLM 按此过滤）", en: "User Interests (LLM filters by this)" },
  "feed.userInterestsPlaceholder": { zh: "例如：AI、Rust、产品管理", en: "e.g. AI, Rust, product management" },
  "feed.redditSubredditsLabel": { zh: "Subreddits", en: "Subreddits" },
  "feed.hnMinScoreLabel": { zh: "最低分数", en: "Min Score" },
  "feed.githubLangLabel": { zh: "语言（留空 = 全部）", en: "Language (empty = all)" },
  "feed.arxivCategoriesLabel": { zh: "分类", en: "Categories" },
  "feed.arxivKeywordsLabel": { zh: "关键词", en: "Keywords" },
  "feed.rssFeedsLabel": { zh: "Feed URL（支持 RSSHub 路由）", en: "Feed URLs (supports RSSHub routes)" },
  "feed.dailyBriefing": { zh: "每日简报", en: "Daily Briefing" },
  "feed.headlines": { zh: "要闻", en: "Headlines" },
  "feed.patterns": { zh: "趋势", en: "Patterns" },
  "feed.ideasForYou": { zh: "灵感", en: "Ideas for You" },
  "feed.digestLoading": { zh: "正在生成摘要…", en: "Generating digest..." },
  "feed.digestEmpty": { zh: "暂无摘要。点击刷新生成。", en: "No digest yet. Click refresh to generate." },
  "feed.digestError": { zh: "摘要生成失败", en: "Failed to generate digest" },
  "feed.regenerate": { zh: "重新生成", en: "Regenerate" },
  "feed.featured": { zh: "精选", en: "Featured" },
  "feed.summary": { zh: "摘要", en: "Summary" },
  "feed.idea": { zh: "灵感", en: "Idea" },
  "feed.items": { zh: "条", en: "items" },

  // MemoryGraph page
  "graph.memoryGraph": { zh: "记忆图谱", en: "Memory Graph" },
  "graph.messages": { zh: "消息", en: "Messages" },
  "graph.loadingGraph": { zh: "加载图谱…", en: "Loading graph..." },
  "graph.nodes": { zh: "个节点", en: "nodes" },
  "graph.edges": { zh: "条边", en: "edges" },
  "graph.linking": { zh: "关联中…", en: "Linking..." },
  "graph.buildConnections": { zh: "构建关联", en: "Build Connections" },
  "graph.noEdges": { zh: "记忆已加载但尚无连接。运行记忆演化以发现关联。", en: "Memories loaded but no connections yet. Run Memory Evolution to discover links." },
  "graph.confidence": { zh: "置信度", en: "confidence" },

  // History page
  "history.title": { zh: "建议历史", en: "Suggestion history" },
  "history.subtitle": { zh: "查看和管理 Sage 的所有建议", en: "View and manage all of Sage's suggestions" },
  "history.searchPlaceholder": { zh: "搜索建议…", en: "Search suggestions..." },
  "history.noResults": { zh: "无匹配结果", en: "No matching results" },
  "history.noHistory": { zh: "暂无历史", en: "No history yet" },
  "history.tryKeywords": { zh: "换个关键词试试", en: "Try different keywords" },
  "history.sageSuggestions": { zh: "Sage 的建议会显示在这里", en: "Sage's suggestions will appear here" },
  "history.talkAboutThis": { zh: "聊聊这个", en: "Talk about this" },
  "history.talkMsg": { zh: "关于 Sage 之前的建议「", en: "Regarding Sage's earlier suggestion \"" },
  "history.talkMsgSuffix": { zh: "」——我想聊聊这个。", en: "\" — I'd like to discuss this." },

  // ── Settings page ──────────────────────────────────────────────────────────

  "settings.title": { zh: "设置", en: "Settings" },
  "settings.loading": { zh: "加载中...", en: "Loading..." },
  "settings.manageProfile": { zh: "管理你的设置和偏好", en: "Manage your profile and preferences" },
  "settings.noProfile": { zh: "尚无档案", en: "No profile yet" },
  "settings.noProfileHint": { zh: "请先完成初始设置", en: "Please complete the initial setup first" },
  "settings.startSetup": { zh: "开始设置", en: "Start setup" },

  "settings.sageObserving": { zh: "Sage 在观察", en: "Sage is observing" },
  "settings.noMemoriesYet": { zh: "还没有记忆...", en: "No memories yet..." },
  "settings.viewAllMemories": { zh: "查看全部记忆 →", en: "View all memories →" },

  "settings.ironRules": { zh: "不可逾越的界限", en: "Absolute limits" },
  "settings.noIronRules": { zh: "尚未设置底线规则", en: "No absolute rules set yet" },

  "settings.connectionsTitle": { zh: "连接与能力", en: "Connections & capabilities" },

  "settings.aiProviders": { zh: "AI 提供商", en: "AI Providers" },
  "settings.detecting": { zh: "检测中...", en: "Detecting..." },
  "settings.priorityOrder": { zh: "优先级顺序", en: "Priority order" },
  "settings.moveUp": { zh: "上移", en: "Move up" },
  "settings.moveDown": { zh: "下移", en: "Move down" },
  "settings.active": { zh: "活跃", en: "Active" },
  "settings.available": { zh: "可用", en: "Available" },
  "settings.needsLogin": { zh: "需要登录", en: "Needs login" },
  "settings.needsSetup": { zh: "需要配置", en: "Needs setup" },
  "settings.notInstalled": { zh: "未安装", en: "Not installed" },
  "settings.configured": { zh: "已配置", en: "Configured" },
  "settings.noProvidersDetected": { zh: "未检测到 AI 提供商", en: "No AI providers detected" },

  "settings.loginHintClaude": { zh: "登录已失效，终端执行 `claude auth login` 后再刷新。", en: "Session expired. Run `claude auth login` in terminal then refresh." },
  "settings.loginHintCodex": { zh: "登录已失效，终端执行 `codex login` 后再刷新。", en: "Session expired. Run `codex login` in terminal then refresh." },
  "settings.loginHintGemini": { zh: "未检测到 Gemini 认证，配置 `~/.gemini/settings.json` 或 `GEMINI_API_KEY` 后再刷新。", en: "No Gemini auth detected. Configure `~/.gemini/settings.json` or `GEMINI_API_KEY` then refresh." },
  "settings.loginHintDefault": { zh: "登录状态失效，请重新认证后再刷新。", en: "Session expired. Please re-authenticate then refresh." },

  "settings.apiKeys": { zh: "API 密钥", en: "API Keys" },
  "settings.enterApiKey": { zh: "输入 API 密钥...", en: "Enter API key..." },
  "settings.testing": { zh: "测试中...", en: "Testing..." },
  "settings.testConnection": { zh: "测试连接", en: "Test connection" },
  "settings.connectionOk": { zh: "连接成功", en: "Connection successful" },
  "settings.connectionFail": { zh: "连接失败，请重试", en: "Something went wrong, try again?" },
  "settings.modelDefault": { zh: "默认", en: "Default" },
  "settings.modelCustom": { zh: "自定义...", en: "Custom..." },
  "settings.enterModelId": { zh: "输入模型 ID...", en: "Enter model ID..." },

  "settings.profile": { zh: "档案", en: "Profile" },
  "settings.name": { zh: "姓名", en: "Name" },
  "settings.role": { zh: "职位", en: "Role" },
  "settings.primaryLanguage": { zh: "主要语言", en: "Primary language" },
  "settings.secondaryLanguage": { zh: "次要语言", en: "Secondary language" },
  "settings.promptLanguage": { zh: "AI 提示词语言", en: "AI prompt language" },
  "settings.promptLanguageHint": { zh: "控制 Sage 在 AI 生成的提示词和分析中使用的语言", en: "Controls the language Sage uses in AI-generated prompts and analysis" },
  "settings.promptLanguageSaved": { zh: "提示词语言已保存", en: "Prompt language saved" },
  "settings.chinese": { zh: "中文", en: "Chinese" },

  "settings.schedule": { zh: "日程偏好", en: "Schedule preferences" },
  "settings.morningBrief": { zh: "晨间简报时间", en: "Morning brief" },
  "settings.eveningReview": { zh: "晚间回顾时间", en: "Evening review" },
  "settings.workStart": { zh: "上班时间", en: "Work start" },
  "settings.workEnd": { zh: "下班时间", en: "Work end" },
  "settings.weeklyReportDay": { zh: "周报日", en: "Weekly report day" },
  "settings.weeklyReportTime": { zh: "周报时间", en: "Weekly report time" },
  "settings.timeFormatHint": { zh: "所有时间使用 24 小时制 (0–23)", en: "All times use 24-hour format (0–23)" },

  "settings.communication": { zh: "沟通偏好", en: "Communication preferences" },
  "settings.commStyle": { zh: "沟通风格", en: "Communication style" },
  "settings.commDirect": { zh: "直接 — 简洁明了", en: "Direct — concise and to the point" },
  "settings.commFormal": { zh: "正式 — 结构化和专业", en: "Formal — structured and professional" },
  "settings.commCasual": { zh: "随意 — 轻松自然", en: "Casual — relaxed and natural" },
  "settings.maxNotifLen": { zh: "最大通知长度", en: "Max notification length" },
  "settings.maxNotifLenHint": { zh: "建议通知的最大字符数 (50–500)", en: "Maximum characters for suggestion notifications (50–500)" },

  "settings.memoryManagement": { zh: "记忆管理", en: "Memory management" },
  "settings.memoryEvolution": { zh: "记忆演化", en: "Memory Evolution" },
  "settings.memoryEvolutionHint": { zh: "去重、合成特征、衰减旧记忆、提升已验证记忆", en: "Deduplicate, synthesize traits, decay stale, promote validated" },
  "settings.runNow": { zh: "立即执行", en: "Run now" },
  "settings.evolutionNotif": { zh: "完成后会显示结果", en: "Results will be shown when done" },


  "settings.reconcile": { zh: "调和矛盾", en: "Reconcile" },
  "settings.reconcileHint": { zh: "扫描所有决策和洞见，标注矛盾项", en: "Scan all decisions & insights for contradictions, annotate outdated ones" },
  "settings.reconciling": { zh: "执行中...", en: "Running..." },
  "settings.reconcileNone": { zh: "完成：未发现矛盾", en: "Done: no contradictions found" },
  "settings.reconcileRevised": { zh: "完成：{n} 条记忆已修订", en: "Done: {n} memories revised" },

  "settings.syncToClaudeCode": { zh: "同步到 Claude Code", en: "Sync to Claude Code" },
  "settings.syncHint": { zh: "用最新 Sage 记忆覆盖 Claude Code 记忆", en: "Overwrite Claude Code memory with latest Sage memories" },
  "settings.syncNow": { zh: "立即同步", en: "Sync now" },

  "settings.redoSetup": { zh: "重新设置", en: "Redo setup" },
  "settings.saving": { zh: "保存中...", en: "Saving..." },
  "settings.saved": { zh: "设置已保存", en: "Settings saved" },
  "settings.saveError": { zh: "出了点问题，请重试", en: "Something went wrong, try again?" },

  // ── Chat page ──────────────────────────────────────────────────────────────

  "chat.history": { zh: "聊天记录", en: "Chat history" },
  "chat.newChat": { zh: "新对话", en: "New chat" },
  "chat.reflecting": { zh: "Sage 正在思考这段对话...", en: "Sage is reflecting on this conversation..." },
  "chat.conversations": { zh: "对话", en: "Conversations" },
  "chat.noConversations": { zh: "还没有对话", en: "No conversations yet" },
  "chat.emptyConversation": { zh: "空对话", en: "Empty conversation" },
  "chat.msgs": { zh: "条消息", en: "msgs" },
  "chat.delete": { zh: "删除", en: "Delete" },
  "chat.emptyTitle": { zh: "与 Sage 对话", en: "Chat with Sage" },
  "chat.emptyHint1": { zh: "每次对话都帮助我更了解你。", en: "Every conversation helps me understand you better." },
  "chat.emptyHint2": { zh: "问我任何事 — 工作决策、自我反思，或者随便聊聊。", en: "Ask me anything — work decisions, self-reflection, or just talk." },
  "chat.placeholder": { zh: "说点什么...", en: "Say something..." },
  "chat.stopGenerating": { zh: "停止生成", en: "Stop generating" },
  "chat.providerError": { zh: "我还没有连接到 AI 提供商。请去**设置**配置一个，然后回来聊天。", en: "I'm not connected to an AI provider yet. Go to **Settings** to configure one, then come back and chat." },
  "chat.errorPrefix": { zh: "出了点问题：", en: "Something went wrong: " },

  // ── AboutYou page ──────────────────────────────────────────────────────────

  "about.title": { zh: "Sage 了解到的你", en: "What Sage knows about you" },
  "about.subtitle": { zh: "这些是通过我们对话和日常互动积累的观察。你可以纠正或删除任何不准确的内容。", en: "These are observations accumulated through our conversations and daily interactions. You can correct or delete anything that's inaccurate." },
  "about.exportMemories": { zh: "导出记忆", en: "Export memories" },
  "about.exporting": { zh: "导出中...", en: "Exporting..." },
  "about.importFromClipboard": { zh: "从剪贴板导入", en: "Import from clipboard" },
  "about.syncToClaudeCode": { zh: "同步到 Claude Code", en: "Sync to Claude Code" },
  "about.loadingState": { zh: "Sage 正在整理它了解你的内容...", en: "Sage is organizing what it knows about you..." },
  "about.emptyState1": { zh: "还不够了解你。", en: "Not enough to go on yet." },
  "about.emptyState2": { zh: "多和我聊聊，我会逐渐认识你。", en: "Chat with me more and I'll get to know you." },
  "about.cognitiveDepth": { zh: "认知深度", en: "Cognitive Depth" },
  "about.tags": { zh: "标签", en: "Tags" },
  "about.clearFilter": { zh: "清除过滤", en: "Clear filter" },
  "about.searchPlaceholder": { zh: "搜索记忆...", en: "Search memories..." },
  "about.tagLabel": { zh: "标签：", en: "Tag: " },
  "about.searchLabel": { zh: "搜索：", en: "Search: " },
  "about.memoriesCount": { zh: "条记忆", en: "memories" },
  "about.tellSageLabel": { zh: "告诉 Sage 一些关于你的事", en: "Tell Sage something about yourself" },
  "about.tellSagePlaceholder": { zh: "告诉 Sage 一些关于你的事...", en: "Tell Sage something about yourself..." },
  "about.saveHint": { zh: "⌘↵ 保存", en: "⌘↵ to save" },
  "about.saving": { zh: "保存中...", en: "Saving..." },
  "about.saved": { zh: "已保存", en: "Saved" },
  "about.saveFailed": { zh: "保存失败", en: "Save failed" },
  "about.importAiLabel": { zh: "从其他 AI 助手导入记忆", en: "Import memories from other AI assistants" },
  "about.importAiPlaceholder": { zh: "粘贴来自 Claude、Gemini 或 ChatGPT 的记忆。Sage 会自动结构化并保存它们。", en: "Paste your memories from Claude, Gemini, or ChatGPT here. Sage will automatically structure and save them." },
  "about.importAiHint": { zh: "粘贴自 Claude / Gemini / ChatGPT", en: "Paste from Claude / Gemini / ChatGPT" },
  "about.importing": { zh: "导入中...", en: "Importing..." },
  "about.import": { zh: "导入", en: "Import" },
  "about.copiedToClipboard": { zh: "已复制到剪贴板", en: "Copied to clipboard" },
  "about.exportFailed": { zh: "导出失败", en: "Export failed" },
  "about.importFailed": { zh: "导入失败 — 请先将内容复制到剪贴板", en: "Import failed — please copy content to clipboard first" },
  "about.syncFailed": { zh: "同步失败：", en: "Sync failed: " },
  "about.deleteMemory": { zh: "删除此记忆", en: "Delete this memory" },
  "about.expand": { zh: "展开", en: "expand" },
  "about.collapse": { zh: "收起", en: "collapse" },
  "about.footer": { zh: "这些观察可能不完全准确 — 人是复杂的。删除任何感觉不对的内容，帮助我更好地了解你。", en: "These observations may not be fully accurate — people are complex. Delete anything that doesn't feel right to help me understand you better." },
  "about.pastePrompt": { zh: "在此粘贴你的内容：", en: "Paste your content here:" },
  "about.importedCount": { zh: "{n} 条记忆已导入", en: "Imported {n} memories" },
  "about.importedFromAi": { zh: "已从 AI 导入 {n} 条记忆", en: "Imported {n} memories from AI" },
  "about.noMemoriesExtracted": { zh: "未提取到记忆 — 请尝试粘贴更多内容", en: "No memories extracted — try pasting more content" },
  "about.importAiFailed": { zh: "导入失败：", en: "Import failed: " },

  // Depth layer labels (used in AboutYou DEPTH_CONFIG)
  "about.depth.episodic": { zh: "事件", en: "Events" },
  "about.depth.semantic": { zh: "规律", en: "Patterns" },
  "about.depth.procedural": { zh: "判断", en: "Judgments" },
  "about.depth.axiom": { zh: "信念", en: "Beliefs" },

  // Pages
  "pages.title": { zh: "页面", en: "Pages" },
  "pages.empty": { zh: "还没有自定义页面", en: "No custom pages yet" },
  "pages.delete": { zh: "删除", en: "Delete" },
  "pages.generating": { zh: "正在生成页面…", en: "Generating page..." },
  "pages.created": { zh: "页面已创建", en: "Page created" },
  "pages.backToList": { zh: "返回列表", en: "Back to list" },
} as const;

type TranslationKey = keyof typeof translations;

// Create translate function for a given language
export function createT(lang: Lang) {
  return function t(key: TranslationKey): string {
    const entry = translations[key];
    if (!entry) return key;
    return entry[lang] ?? entry["en"];
  };
}

// Default lang detection — overridden by profile at runtime
export function detectLang(): Lang {
  const nav = typeof navigator !== "undefined" ? navigator.language : "en";
  return nav.startsWith("zh") ? "zh" : "en";
}

export type { Lang, TranslationKey };
