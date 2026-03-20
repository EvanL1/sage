use crate::app::{App, Panel};
use ratatui::prelude::*;
use ratatui::widgets::*;

// ─── Longbridge 配色 ───
const BG: Color = Color::Rgb(13, 17, 23);
const GREEN: Color = Color::Rgb(0, 176, 124);
const GOLD: Color = Color::Rgb(255, 184, 0);
const FG: Color = Color::Rgb(201, 209, 217);
const FG_DIM: Color = Color::Rgb(100, 110, 120);
const BORDER_ACT: Color = GREEN;
const BORDER_DIM: Color = Color::Rgb(48, 54, 61);

fn panel_block(title: &str, focused: bool) -> Block<'_> {
    let border_color = if focused { BORDER_ACT } else { BORDER_DIM };
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(if focused { GREEN } else { FG_DIM })
                .bold(),
        ))
        .style(Style::default().bg(BG))
}

pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // 如果有详情视图，全屏显示
    if let Some(detail) = &app.detail_view {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(GREEN))
            .title(Span::styled(
                " Detail [Esc 关闭] ",
                Style::default().fg(GOLD).bold(),
            ))
            .style(Style::default().bg(BG));
        let p = Paragraph::new(detail.as_str())
            .wrap(Wrap { trim: false })
            .block(block)
            .style(Style::default().fg(FG));
        frame.render_widget(p, area);
        return;
    }

    // 主布局：status(3) + main(fill) + bottom(7) + cmd(3)
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(8),
        Constraint::Length(7),
        Constraint::Length(3),
    ])
    .split(area);

    render_status_bar(frame, app, chunks[0]);
    render_main(frame, app, chunks[1]);
    render_bottom(frame, app, chunks[2]);
    render_command_bar(frame, app, chunks[3]);
}

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let stats_str: String = app
        .stats
        .iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join("  ");

    let line = Line::from(vec![
        Span::styled(" ● ", Style::default().fg(GREEN).bold()),
        Span::styled("SAGE", Style::default().fg(FG).bold()),
        Span::styled("  │  ", Style::default().fg(BORDER_DIM)),
        Span::styled(&now, Style::default().fg(GOLD)),
        Span::styled("  │  ", Style::default().fg(BORDER_DIM)),
        Span::styled(stats_str, Style::default().fg(FG_DIM)),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_DIM))
        .style(Style::default().bg(BG));
    let p = Paragraph::new(line).block(block);
    frame.render_widget(p, area);
}

fn render_main(frame: &mut Frame, app: &App, area: Rect) {
    let cols =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).split(area);

    // Brief panel
    let brief_title = if app.brief_type.is_empty() {
        "BRIEF".to_string()
    } else {
        app.brief_type.to_uppercase()
    };
    let brief_block = panel_block(&brief_title, app.focused == Panel::Brief);
    let brief_text = app
        .brief
        .as_deref()
        .unwrap_or("暂无报告。在 Desktop 中点 AM 生成。");
    let brief_lines = render_markdown(brief_text);
    let p = Paragraph::new(brief_lines)
        .wrap(Wrap { trim: false })
        .scroll((app.brief_scroll, 0))
        .block(brief_block);
    frame.render_widget(p, cols[0]);

    // Activity panel
    let act_block = panel_block("ACTIVITY", app.focused == Panel::Activity);
    let items: Vec<ListItem> = app
        .activity
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let kind_color = match item.kind.as_str() {
                "EMAIL" => GOLD,
                "SESS" => Color::Rgb(99, 102, 241),
                _ => FG_DIM,
            };
            let style = if i == app.activity_offset && app.focused == Panel::Activity {
                Style::default().bg(Color::Rgb(30, 35, 45))
            } else {
                Style::default()
            };
            let line = Line::from(vec![
                Span::styled(format!("{:5} ", item.kind), Style::default().fg(kind_color)),
                Span::styled(&item.title, Style::default().fg(FG)),
                Span::styled(format!("  {}", item.time), Style::default().fg(FG_DIM)),
            ]);
            ListItem::new(line).style(style)
        })
        .collect();
    let list = List::new(items).block(act_block);
    frame.render_widget(list, cols[1]);
}

fn render_bottom(frame: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::horizontal([Constraint::Length(24), Constraint::Min(20)]).split(area);

    // Stats panel
    let stats_block = panel_block("STATS", app.focused == Panel::Stats);
    let stats_lines: Vec<Line> = app
        .stats
        .iter()
        .map(|(k, v)| {
            Line::from(vec![
                Span::styled(format!("  {k:>5} "), Style::default().fg(FG_DIM)),
                Span::styled(format!("{v:>6}"), Style::default().fg(GREEN).bold()),
            ])
        })
        .collect();
    let p = Paragraph::new(stats_lines).block(stats_block);
    frame.render_widget(p, cols[0]);

    // Tags panel
    let tags_block = panel_block("TAGS", app.focused == Panel::Tags);
    let tag_spans: Vec<Span> = app
        .tags
        .iter()
        .flat_map(|(tag, count)| {
            vec![
                Span::styled(format!("#{tag}"), Style::default().fg(GREEN)),
                Span::styled(format!("({count}) "), Style::default().fg(FG_DIM)),
            ]
        })
        .collect();
    let line = Line::from(tag_spans);
    let p = Paragraph::new(vec![line])
        .wrap(Wrap { trim: false })
        .block(tags_block);
    frame.render_widget(p, cols[1]);
}

fn render_command_bar(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_DIM))
        .style(Style::default().bg(BG));

    let content = if app.command_mode {
        Line::from(vec![
            Span::styled(" :", Style::default().fg(GREEN).bold()),
            Span::styled(&app.command_input, Style::default().fg(FG)),
            Span::styled("█", Style::default().fg(GREEN)),
        ])
    } else if let Some(msg) = &app.status_msg {
        Line::from(Span::styled(format!(" {msg}"), Style::default().fg(GOLD)))
    } else {
        Line::from(vec![
            Span::styled(" [:", Style::default().fg(FG_DIM)),
            Span::styled("命令", Style::default().fg(FG_DIM)),
            Span::styled("]  [Tab]", Style::default().fg(FG_DIM)),
            Span::styled("切换  ", Style::default().fg(FG_DIM)),
            Span::styled("[j/k]", Style::default().fg(FG_DIM)),
            Span::styled("滚动  ", Style::default().fg(FG_DIM)),
            Span::styled("[?]", Style::default().fg(FG_DIM)),
            Span::styled("帮助  ", Style::default().fg(FG_DIM)),
            Span::styled("[q]", Style::default().fg(FG_DIM)),
            Span::styled("退出", Style::default().fg(FG_DIM)),
        ])
    };
    let p = Paragraph::new(content).block(block);
    frame.render_widget(p, area);
}

/// 简易 markdown → ratatui Line 渲染
fn render_markdown(text: &str) -> Vec<Line<'static>> {
    text.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("# ")
                || trimmed.starts_with("## ")
                || trimmed.starts_with("### ")
            {
                let content = trimmed.trim_start_matches('#').trim();
                Line::from(Span::styled(
                    content.to_string(),
                    Style::default().fg(GOLD).bold(),
                ))
            } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                let content = trimmed[2..].to_string();
                Line::from(vec![
                    Span::styled("  • ", Style::default().fg(GREEN)),
                    Span::styled(render_bold_inline(&content), Style::default().fg(FG)),
                ])
            } else if trimmed.starts_with("| ") {
                // Table row — render as-is with dim color
                Line::from(Span::styled(line.to_string(), Style::default().fg(FG_DIM)))
            } else if trimmed.starts_with("---") || trimmed.starts_with("```") {
                Line::from(Span::styled(
                    "─".repeat(40),
                    Style::default().fg(BORDER_DIM),
                ))
            } else if trimmed.starts_with("> ") {
                Line::from(Span::styled(
                    format!("│ {}", &trimmed[2..]),
                    Style::default().fg(FG_DIM).italic(),
                ))
            } else if trimmed.is_empty() {
                Line::from("")
            } else {
                Line::from(Span::styled(
                    render_bold_inline(line),
                    Style::default().fg(FG),
                ))
            }
        })
        .collect()
}

/// 简易 **bold** 处理（单 span，去掉 ** 标记）
fn render_bold_inline(text: &str) -> String {
    text.replace("**", "")
}
