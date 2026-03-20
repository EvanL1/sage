use sage_core::store::Store;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq)]
pub enum Panel {
    Brief,
    Activity,
    Stats,
    Tags,
}

impl Panel {
    pub fn next(self) -> Self {
        match self {
            Panel::Brief => Panel::Activity,
            Panel::Activity => Panel::Stats,
            Panel::Stats => Panel::Tags,
            Panel::Tags => Panel::Brief,
        }
    }
}

pub struct ActivityItem {
    pub kind: String,
    pub title: String,
    pub time: String,
    pub detail: String,
}

pub struct App {
    pub brief: Option<String>,
    pub brief_type: String,
    pub activity: Vec<ActivityItem>,
    pub stats: [(&'static str, usize); 5],
    pub tags: Vec<(String, usize)>,

    pub focused: Panel,
    pub brief_scroll: u16,
    pub activity_offset: usize,
    pub command_input: String,
    pub command_mode: bool,
    pub status_msg: Option<String>,
    pub detail_view: Option<String>,

    pub store: Arc<Store>,
    pub should_quit: bool,
    pub last_refresh: Instant,
}

impl App {
    pub fn new(store: Arc<Store>) -> Self {
        Self {
            brief: None,
            brief_type: String::new(),
            activity: vec![],
            stats: [("MEM", 0), ("EDGE", 0), ("SESS", 0), ("MSG", 0), ("PPL", 0)],
            tags: vec![],
            focused: Panel::Brief,
            brief_scroll: 0,
            activity_offset: 0,
            command_input: String::new(),
            command_mode: false,
            status_msg: None,
            detail_view: None,
            store,
            should_quit: false,
            last_refresh: Instant::now(),
        }
    }

    pub fn tick(&mut self) {
        let s = &self.store;

        // Brief
        if let Ok(Some(report)) = s.get_latest_report("morning") {
            self.brief = Some(report.content);
            self.brief_type = "Morning Brief".into();
        } else if let Ok(Some(report)) = s.get_latest_report("evening") {
            self.brief = Some(report.content);
            self.brief_type = "Evening Review".into();
        }

        // Stats
        self.stats = [
            ("MEM", s.count_memories().unwrap_or(0) as usize),
            ("EDGE", s.count_memory_edges().unwrap_or(0) as usize),
            ("SESS", s.count_distinct_sessions().unwrap_or(0) as usize),
            ("MSG", s.count_messages().unwrap_or(0) as usize),
            ("PPL", s.get_known_persons().map(|v| v.len()).unwrap_or(0)),
        ];

        // Tags
        self.tags = s
            .get_all_tags()
            .unwrap_or_default()
            .into_iter()
            .take(12)
            .map(|(tag, count)| (tag, count as usize))
            .collect();

        // Activity: recent messages (emails + sessions)
        self.activity.clear();
        if let Ok(msgs) = s.get_messages_by_source("email", 10) {
            for m in msgs {
                self.activity.push(ActivityItem {
                    kind: "EMAIL".into(),
                    title: m.channel.clone(), // channel = subject
                    time: short_time(&m.timestamp),
                    detail: m.content.unwrap_or_default(),
                });
            }
        }
        if let Ok(mems) = s.get_messages_by_source("claude-code", 5) {
            for m in mems {
                self.activity.push(ActivityItem {
                    kind: "SESS".into(),
                    title: m.channel.clone(),
                    time: short_time(&m.timestamp),
                    detail: m.content.unwrap_or_default(),
                });
            }
        }
        // Sort by time desc (crude string sort, works for consistent timestamp formats)
        self.activity.sort_by(|a, b| b.time.cmp(&a.time));
        self.activity.truncate(15);

        self.last_refresh = Instant::now();
    }

    pub fn execute_command(&mut self) {
        let cmd = self.command_input.trim().to_string();
        self.command_input.clear();
        self.command_mode = false;

        if cmd == "q" || cmd == "quit" {
            self.should_quit = true;
        } else if cmd == "brief" {
            self.status_msg = Some("Brief 请通过 Desktop 生成（CLI 为只读模式）".into());
        } else if cmd.starts_with("correct ") {
            if let Some((wrong, fact)) = cmd
                .strip_prefix("correct ")
                .and_then(|s| s.split_once("->"))
            {
                let wrong = wrong.trim();
                let fact = fact.trim();
                if wrong.len() >= 5 && fact.len() >= 5 {
                    match self.store.save_correction("morning", wrong, fact, "") {
                        Ok(_) => {
                            self.status_msg = Some(format!(
                                "✓ 校准已保存：{} → {}",
                                &wrong[..wrong.len().min(30)],
                                &fact[..fact.len().min(30)]
                            ))
                        }
                        Err(e) => self.status_msg = Some(format!("✗ 保存失败: {e}")),
                    }
                } else {
                    self.status_msg = Some("格式: :correct 错误内容 -> 正确内容（各≥5字）".into());
                }
            } else {
                self.status_msg = Some("格式: :correct 错误内容 -> 正确内容".into());
            }
        } else if cmd == "help" || cmd == "?" {
            self.status_msg =
                Some(":q退出  :correct A->B校准  Tab切面板  j/k滚动  Enter展开".into());
        } else {
            self.status_msg = Some(format!("未知命令: :{cmd}  输入 :help 查看帮助"));
        }
    }

    pub fn scroll_up(&mut self) {
        match self.focused {
            Panel::Brief => self.brief_scroll = self.brief_scroll.saturating_sub(1),
            Panel::Activity => self.activity_offset = self.activity_offset.saturating_sub(1),
            _ => {}
        }
    }

    pub fn scroll_down(&mut self) {
        match self.focused {
            Panel::Brief => self.brief_scroll = self.brief_scroll.saturating_add(1),
            Panel::Activity => {
                if self.activity_offset + 1 < self.activity.len() {
                    self.activity_offset += 1;
                }
            }
            _ => {}
        }
    }

    pub fn enter(&mut self) {
        if self.focused == Panel::Activity && self.activity_offset < self.activity.len() {
            let item = &self.activity[self.activity_offset];
            self.detail_view = Some(format!("[{}] {}\n\n{}", item.kind, item.title, item.detail));
        }
    }
}

fn short_time(ts: &str) -> String {
    // Extract just date+time portion for display
    if ts.len() > 16 {
        ts[..16].to_string()
    } else {
        ts.to_string()
    }
}
