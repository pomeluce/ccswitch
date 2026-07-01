use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
    Frame,
};
use super::super::theme::Theme;

/// Centered rectangle helper
pub fn centered_rect(w: u16, h: u16, r: Rect) -> Rect {
    Rect {
        x: r.x + (r.width.saturating_sub(w)) / 2,
        y: r.y + (r.height.saturating_sub(h)) / 2,
        width: w.min(r.width),
        height: h.min(r.height),
    }
}

/// Render a search box input widget
pub fn render_search_box(
    f: &mut Frame, area: Rect,
    query: &str, is_searching: bool,
) {
    let cursor = if is_searching { "\u{258c}" } else { "" };
    let text = if query.is_empty() && !is_searching {
        "\u{2315} Search (/ to focus)".to_string()
    } else if !query.is_empty() && !is_searching {
        format!("\u{2315} {} (/) — Esc to clear", query)
    } else {
        format!("\u{2315} {}{}", query, cursor)
    };
    let color = if is_searching { Theme::CYAN } else { Theme::COMMENT };
    let p = Paragraph::new(Line::from(Span::styled(text, Style::default().fg(color))))
        .block(Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
            .border_style(Style::default().fg(Theme::DIM)));
    f.render_widget(p, area);
}

/// Render a shortcut bar that wraps at group boundaries for narrow windows
pub fn render_shortcut_bar(
    f: &mut Frame, area: Rect,
    groups: &[Vec<(String, Color)>],
) {
    let sep = || Span::styled("  ".to_string(), Style::default());
    let group_spans: Vec<Vec<Span>> = groups.iter().map(|grp| {
        let label = Span::styled(
            format!(" {}", grp[1].0.clone()),
            Style::default().fg(Theme::COMMENT),
        );
        vec![
            Span::styled(grp[0].0.clone(), Style::default().fg(grp[0].1)),
            label,
        ]
    }).collect();

    let width = area.width.saturating_sub(2).max(10) as usize; // account for border
    let mut rows: Vec<Line> = Vec::new();
    let mut cur: Vec<Span> = Vec::new();
    let mut cur_w = 0usize;

    for g in &group_spans {
        let gw: usize = g.iter().map(|s| s.width()).sum();
        if cur_w + gw > width && !cur.is_empty() {
            rows.push(Line::from(std::mem::take(&mut cur)));
            cur_w = 0;
        }
        if !cur.is_empty() { cur.push(sep()); cur_w += 2; }
        cur.extend(g.clone());
        cur_w += gw;
    }
    if !cur.is_empty() { rows.push(Line::from(cur)); }
    if rows.is_empty() { rows.push(Line::default()); }

    f.render_widget(
        Paragraph::new(rows).centered().block(
            Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                .border_style(Style::default().fg(Theme::DIM)),
        ),
        area,
    );
}

/// Render a confirmation popup with two buttons
pub fn render_confirm_popup(
    f: &mut Frame, area: Rect,
    title: &str, msg: &str,
    confirm_label: &str, cancel_label: &str,
    confirm_color: Color, selected: usize, // 0=confirm, 1=cancel
) {
    let popup = centered_rect(44, 6, area);
    let cs = if selected == 0 { Style::default().fg(Color::Black).bg(confirm_color) } else { Style::default().fg(Theme::DIM) };
    let xs = if selected == 1 { Style::default().fg(Color::Black).bg(Theme::CYAN) } else { Style::default().fg(Theme::DIM) };

    let p = Paragraph::new(vec![
        Line::from(msg).centered(), Line::from(""),
        Line::from(vec![
            Span::styled(format!("  {}  ", confirm_label), cs),
            Span::raw("     "),
            Span::styled(format!("  {}  ", cancel_label), xs),
        ]).centered(),
    ]).block(Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
        .title(Line::from(title).centered())
        .border_style(Style::default().fg(confirm_color)));
    f.render_widget(Clear, popup);
    f.render_widget(p, popup);
}

/// Render a simple message/notice popup with OK button
pub fn render_message_popup(
    f: &mut Frame, area: Rect,
    msg: &str,
) {
    let popup = centered_rect(44, 5, area);
    let p = Paragraph::new(vec![
        Line::from(""), Line::from(msg).centered(), Line::from(""),
        Line::from(Span::styled("  OK  ", Style::default().fg(Color::Black).bg(Theme::CYAN))).centered(),
    ]).block(Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
        .title(Line::from(" Notice ").centered())
        .border_style(Style::default().fg(Theme::YELLOW)));
    f.render_widget(Clear, popup);
    f.render_widget(p, popup);
}

/// === Format helpers ===

pub fn format_size(bytes: i64) -> String {
    if bytes < 1024 { format!("{}B", bytes) }
    else if bytes < 1024 * 1024 { format!("{:.1}KB", bytes as f64 / 1024.0) }
    else { format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0)) }
}

pub fn format_date(iso: &str) -> String {
    if iso.len() >= 16 { iso[5..16].to_string() } else { iso.to_string() }
}

pub fn relative_time(iso: &str) -> String {
    if iso.len() < 19 { return format_date(iso); }
    let parsed = chrono::NaiveDateTime::parse_from_str(&iso[..19], "%Y-%m-%d %H:%M:%S");
    let dt = match parsed { Ok(d) => d.and_utc(), Err(_) => return format_date(iso) };
    let dur = chrono::Utc::now() - dt;
    let mins = dur.num_minutes(); let hrs = dur.num_hours(); let days = dur.num_days();
    if mins < 1 { "just now".into() }
    else if mins < 60 { format!("{} min ago", mins) }
    else if hrs < 24 { format!("{} hours ago", hrs) }
    else if days < 7 { format!("{} days ago", days) }
    else if days < 30 { format!("{} weeks ago", days / 7) }
    else { format!("{} months ago", days / 30) }
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}...", s.chars().take(max.saturating_sub(3)).collect::<String>())
    } else { s.to_string() }
}

/// Shortcut lines needed for dynamic bar height
pub fn shortcut_lines(available_width: u16, group_widths: &[usize]) -> usize {
    let sep = 2usize;
    let w = available_width.max(10) as usize;
    let mut lines = 1usize;
    let mut cur = 0usize;
    for gw in group_widths {
        if cur + gw > w && cur > 0 { lines += 1; cur = 0; }
        if cur > 0 { cur += sep; }
        cur += gw;
    }
    lines
}
