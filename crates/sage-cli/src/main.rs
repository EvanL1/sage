mod app;
mod ui;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use serde_json::json;
use std::io::{self, Read};
use std::sync::Arc;
use std::time::Duration;

use app::App;
use sage_core::store::Store;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json_mode = args.iter().any(|a| a == "--json");
    let args: Vec<String> = args.into_iter().filter(|a| a != "--json").collect();

    let data_dir = sage_core::config::Config::expand_path("~/.sage/data");
    let db_path = data_dir.join("sage.db");
    let store = Arc::new(Store::open(&db_path)?);

    if args.is_empty() {
        return tui_mode(store);
    }

    cli_mode(&store, &args, json_mode)
}

// ─── CLI 命令 ───

fn cli_mode(store: &Store, args: &[String], json: bool) -> Result<()> {
    let rest = args[1..].join(" ");
    match args[0].as_str() {
        // ── 读取命令 ──
        "brief" | "b" => cmd_brief(store, json),
        "status" | "s" => cmd_status(store, json),
        "emails" | "e" => cmd_emails(store, json),
        "memories" | "m" => cmd_memories(store, json),
        "tags" | "t" => cmd_tags(store, json),

        // ── AI 原生：上下文 ──
        "context" | "ctx" => cmd_context(store),
        "search" => cmd_search(store, &rest, json),

        // ── AI 原生：写入 ──
        "learn" | "l" => cmd_learn(store, &rest),
        "observe" | "o" => cmd_observe(store, &rest),
        "correct" | "c" => cmd_correct(store, &args[1..]),

        // ── AI 原生：管道 ──
        "pipe" | "p" => cmd_pipe(store, &rest),

        // ── 帮助 ──
        "help" | "h" | "--help" | "-h" => cmd_help(),

        other => {
            eprintln!("未知命令: {other}\n运行 sage help 查看帮助");
            std::process::exit(1);
        }
    }
    Ok(())
}

fn cmd_brief(store: &Store, json: bool) {
    for rt in &["morning", "evening", "weekly"] {
        if let Ok(Some(r)) = store.get_latest_report(rt) {
            if json {
                println!(
                    "{}",
                    json!({ "type": rt, "content": r.content, "created_at": r.created_at })
                );
            } else {
                println!("{}", r.content);
            }
            return;
        }
    }
    if json {
        println!("{}", json!(null));
    } else {
        println!("暂无报告。");
    }
}

fn cmd_status(store: &Store, json: bool) {
    let mem = store.count_memories().unwrap_or(0);
    let edges = store.count_memory_edges().unwrap_or(0);
    let sess = store.count_distinct_sessions().unwrap_or(0);
    let msgs = store.count_messages().unwrap_or(0);
    let ppl = store.get_known_persons().map(|v| v.len()).unwrap_or(0);
    let tags = store.get_all_tags().unwrap_or_default();
    if json {
        println!(
            "{}",
            json!({ "memories": mem, "edges": edges, "sessions": sess, "messages": msgs, "people": ppl, "tags": tags.len(),
            "top_tags": tags.iter().take(10).map(|(t,c)| json!({"tag":t,"count":c})).collect::<Vec<_>>() })
        );
    } else {
        println!("● SAGE ONLINE\n  Memories: {mem}  Links: {edges}  Sessions: {sess}  Messages: {msgs}  People: {ppl}  Tags: {}", tags.len());
        if !tags.is_empty() {
            let top: Vec<String> = tags
                .iter()
                .take(10)
                .map(|(t, c)| format!("#{t}({c})"))
                .collect();
            println!("  {}", top.join(" "));
        }
    }
}

fn cmd_emails(store: &Store, json: bool) {
    let msgs = store
        .get_messages_by_source("email", 10)
        .unwrap_or_default();
    if json {
        let items: Vec<_> = msgs
            .iter()
            .map(|m| {
                json!({ "subject": m.channel, "from": m.sender, "time": m.timestamp,
            "body": m.content.as_deref().map(|b| b.chars().take(300).collect::<String>()) })
            })
            .collect();
        println!("{}", json!(items));
    } else {
        if msgs.is_empty() {
            println!("暂无邮件。");
            return;
        }
        for m in &msgs {
            println!("[{}] {} — {}", m.timestamp, m.channel, m.sender);
            if let Some(b) = &m.content {
                let p: String = b.chars().take(80).collect();
                println!("  > {p}");
            }
        }
    }
}

fn cmd_memories(store: &Store, json: bool) {
    let since = (chrono::Local::now() - chrono::Duration::days(7)).to_rfc3339();
    let mems = store.get_memories_since(&since).unwrap_or_default();
    if json {
        let items: Vec<_> = mems
            .iter()
            .take(30)
            .map(|m| {
                json!({ "category": m.category, "content": m.content,
            "confidence": m.confidence, "created_at": m.created_at })
            })
            .collect();
        println!("{}", json!(items));
    } else {
        if mems.is_empty() {
            println!("近 7 天无新记忆。");
            return;
        }
        for m in mems.iter().take(20) {
            println!(
                "[{}] {}: {}",
                &m.created_at[..16.min(m.created_at.len())],
                m.category,
                m.content
            );
        }
    }
}

fn cmd_tags(store: &Store, json: bool) {
    let tags = store.get_all_tags().unwrap_or_default();
    if json {
        println!(
            "{}",
            json!(tags
                .iter()
                .map(|(t, c)| json!({"tag":t,"count":c}))
                .collect::<Vec<_>>())
        );
    } else {
        if tags.is_empty() {
            println!("暂无标签。");
            return;
        }
        for (t, c) in tags.iter().take(20) {
            println!("  #{t:20} {c}");
        }
    }
}

// ── AI 原生：完整上下文输出（JSON，供其他 AI agent 消费）──

fn cmd_context(store: &Store) {
    let mem = store.count_memories().unwrap_or(0);
    let edges = store.count_memory_edges().unwrap_or(0);
    let sess = store.count_distinct_sessions().unwrap_or(0);

    // 最新报告
    let brief = ["morning", "evening", "weekly"]
        .iter()
        .find_map(|rt| store.get_latest_report(rt).ok().flatten())
        .map(
            |r| json!({ "type": r.report_type, "content": r.content, "created_at": r.created_at }),
        );

    // 近期记忆
    let since_3d = (chrono::Local::now() - chrono::Duration::days(3)).to_rfc3339();
    let recent_mems: Vec<_> = store
        .get_memories_since(&since_3d)
        .unwrap_or_default()
        .iter()
        .take(20)
        .map(
            |m| json!({ "category": m.category, "content": m.content, "confidence": m.confidence }),
        )
        .collect();

    // 近期邮件
    let emails: Vec<_> = store
        .get_messages_by_source("email", 5)
        .unwrap_or_default()
        .iter()
        .map(|m| json!({ "subject": m.channel, "from": m.sender, "time": m.timestamp }))
        .collect();

    // 决策
    let since_7d = (chrono::Local::now() - chrono::Duration::days(7)).to_rfc3339();
    let decisions: Vec<_> = store
        .get_memories_since(&since_7d)
        .unwrap_or_default()
        .iter()
        .filter(|m| m.category == "decision")
        .map(|m| m.content.clone())
        .collect();

    // 校准
    let corrections: Vec<_> = store
        .get_all_corrections()
        .unwrap_or_default()
        .iter()
        .map(|c| json!({ "wrong": c.wrong_claim, "correct": c.correct_fact }))
        .collect();

    // Tags
    let tags: Vec<_> = store
        .get_all_tags()
        .unwrap_or_default()
        .iter()
        .take(10)
        .map(|(t, c)| json!({"tag":t,"count":c}))
        .collect();

    let ctx = json!({
        "timestamp": chrono::Local::now().to_rfc3339(),
        "user": "Alex",
        "stats": { "memories": mem, "edges": edges, "sessions": sess },
        "latest_brief": brief,
        "recent_memories": recent_mems,
        "recent_emails": emails,
        "decisions": decisions,
        "corrections": corrections,
        "tags": tags,
    });
    println!("{}", serde_json::to_string_pretty(&ctx).unwrap_or_default());
}

// ── AI 原生：语义搜索 ──

fn cmd_search(store: &Store, query: &str, json: bool) {
    if query.is_empty() {
        eprintln!("用法: sage search \"关键词\"");
        return;
    }

    // 搜索记忆
    let mems = store.search_memories(query, 10).unwrap_or_default();
    // 搜索消息
    let msgs = store.search_messages(query, 5).unwrap_or_default();

    if json {
        let mem_items: Vec<_> = mems.iter().map(|m| json!({"type":"memory","category":m.category,"content":m.content,"confidence":m.confidence})).collect();
        let msg_items: Vec<_> = msgs.iter().map(|m| json!({"type":"message","subject":m.channel,"from":m.sender,"time":m.timestamp})).collect();
        println!(
            "{}",
            json!({ "query": query, "memories": mem_items, "messages": msg_items })
        );
    } else {
        println!("搜索: \"{query}\"\n");
        if !mems.is_empty() {
            println!("── 记忆 ──");
            for m in &mems {
                println!("  [{}] {}", m.category, m.content);
            }
        }
        if !msgs.is_empty() {
            println!("\n── 消息 ──");
            for m in &msgs {
                println!("  [{}] {} — {}", m.timestamp, m.channel, m.sender);
            }
        }
        if mems.is_empty() && msgs.is_empty() {
            println!("无结果。");
        }
    }
}

// ── AI 原生：写入记忆 ──

fn cmd_learn(store: &Store, content: &str) {
    if content.len() < 3 {
        eprintln!("用法: sage learn \"要记住的事实\"");
        return;
    }
    match store.save_memory("fact", content, "cli", 0.7) {
        Ok(id) => println!("{}", json!({"ok":true,"id":id,"content":content})),
        Err(e) => eprintln!("{}", json!({"ok":false,"error":e.to_string()})),
    }
}

// ── AI 原生：记录观察 ──

fn cmd_observe(store: &Store, content: &str) {
    if content.len() < 3 {
        eprintln!("用法: sage observe \"观察到的行为模式\"");
        return;
    }
    match store.record_observation("cli_observation", content, None) {
        Ok(_) => println!("{}", json!({"ok":true,"content":content})),
        Err(e) => eprintln!("{}", json!({"ok":false,"error":e.to_string()})),
    }
}

// ── 校准 ──

fn cmd_correct(store: &Store, args: &[String]) {
    if args.len() >= 2 {
        let wrong = &args[0];
        let fact = &args[1];
        let hint = args.get(2).map(|s| s.as_str()).unwrap_or("");
        match store.save_correction("morning", wrong, fact, hint) {
            Ok(id) => println!(
                "{}",
                json!({"ok":true,"id":id,"wrong":wrong,"correct":fact})
            ),
            Err(e) => eprintln!("{}", json!({"ok":false,"error":e.to_string()})),
        }
    } else {
        eprintln!("用法: sage correct \"错误内容\" \"正确内容\" [标签]");
    }
}

// ── 管道：读 stdin + 存储 ──

fn cmd_pipe(store: &Store, label: &str) {
    let mut stdin_buf = String::new();
    if io::stdin().read_to_string(&mut stdin_buf).is_err() || stdin_buf.trim().is_empty() {
        eprintln!("用法: echo \"内容\" | sage pipe \"标签\"");
        return;
    }
    let label = if label.is_empty() {
        "pipe_input"
    } else {
        label
    };
    let content = format!("[{label}] {}", stdin_buf.trim());
    // 存为 observation，供 Coach/Brief 消费
    match store.record_observation("pipe", &content, None) {
        Ok(_) => println!(
            "{}",
            json!({"ok":true,"label":label,"length":stdin_buf.len()})
        ),
        Err(e) => eprintln!("{}", json!({"ok":false,"error":e.to_string()})),
    }
}

fn cmd_help() {
    println!("sage — Personal AI Assistant CLI\n");
    println!("用法: sage [command] [args] [--json]\n");
    println!("无参数启动实时 TUI 面板\n");
    println!("── 查询 ──");
    println!("  brief    (b)   最新报告");
    println!("  status   (s)   系统状态");
    println!("  emails   (e)   近期邮件");
    println!("  memories (m)   近期记忆");
    println!("  tags     (t)   标签列表\n");
    println!("── AI 原生 ──");
    println!("  context  (ctx) 完整上下文 JSON（供其他 AI 消费）");
    println!("  search   \"kw\"  语义搜索记忆+消息");
    println!("  learn    \"..\" 写入记忆（category=fact）");
    println!("  observe  \"..\" 记录行为观察");
    println!("  correct  \"错\" \"对\"  提交校准");
    println!("  pipe     \"标签\"    读 stdin 存为观察\n");
    println!("── 选项 ──");
    println!("  --json         结构化 JSON 输出\n");
    println!("── 示例 ──");
    println!("  sage context | jq .latest_brief");
    println!("  sage search \"ProjectY\" --json");
    println!("  sage learn \"AWS Saving Plan 已于3月实施\"");
    println!("  git log -5 | sage pipe \"today commits\"");
}

// ─── TUI 模式 ───

fn tui_mode(store: Arc<Store>) -> Result<()> {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    let mut app = App::new(store);
    app.tick();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|f| ui::render(f, &app))?;
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                handle_key(&mut app, key);
            }
        }
        if app.last_refresh.elapsed() > Duration::from_secs(10) {
            app.tick();
        }
        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }
    if app.detail_view.is_some() {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => app.detail_view = None,
            _ => {}
        }
        return;
    }
    if app.command_mode {
        match key.code {
            KeyCode::Enter => app.execute_command(),
            KeyCode::Esc => {
                app.command_mode = false;
                app.command_input.clear();
            }
            KeyCode::Backspace => {
                app.command_input.pop();
            }
            KeyCode::Char(c) => app.command_input.push(c),
            _ => {}
        }
        return;
    }
    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char(':') => {
            app.command_mode = true;
            app.status_msg = None;
        }
        KeyCode::Char('?') => {
            app.command_mode = true;
            app.command_input = "help".into();
            app.execute_command();
        }
        KeyCode::Tab => app.focused = app.focused.next(),
        KeyCode::Char('j') | KeyCode::Down => app.scroll_down(),
        KeyCode::Char('k') | KeyCode::Up => app.scroll_up(),
        KeyCode::Enter => app.enter(),
        KeyCode::Char('r') => {
            app.tick();
            app.status_msg = Some("刷新完成".into());
        }
        _ => {}
    }
}
