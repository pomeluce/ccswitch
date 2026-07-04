use super::super::tabs::Tab;
use crate::tui::lang;
use super::super::theme;
use crate::db::Db;
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

pub fn render_sidebar(f: &mut Frame, area: Rect, active_tab: Tab, db: &Db) {
    let tabs = [
        (Tab::Providers, lang::current().tab_providers),
        (Tab::Usage, lang::current().tab_usage),
        (Tab::History, lang::current().tab_history),
        (Tab::Settings, lang::current().tab_settings),
    ];

    let proxy_running = db.get_setting("proxy_mode").map(|v| v == "true").unwrap_or(false);
    let mode_label = if proxy_running {
        format!("{} proxy", lang::current().mode_prefix)
    } else {
        format!("{} local", lang::current().mode_prefix)
    };

    let tab_lines = (tabs.len() * 2) as u16;
    let header_lines = 3u16;  // title + 2 blank lines
    let footer_lines = 1u16;  // mode
    let inner_h = area.height.saturating_sub(2); // border
    let avail = inner_h.saturating_sub(header_lines + footer_lines);
    let pad_bottom = avail.saturating_sub(tab_lines);

    let mut lines: Vec<Line> = Vec::new();
    // Title
    lines.push(Line::from(Span::styled(
        "ccswitch-tui",
        Style::default().fg(theme::current().dim),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(""));

    for (tab, label) in &tabs {
        let style = if *tab == active_tab {
            Style::default().fg(theme::current().cyan)
        } else {
            Style::default().fg(theme::current().dim)
        };
        lines.push(Line::from(Span::styled(*label, style)));
        lines.push(Line::from(""));
    }

    for _ in 0..pad_bottom {
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        mode_label,
        Style::default().fg(theme::current().dim),
    )));

    let p = Paragraph::new(lines)
        .centered()
        .block(
            Block::bordered()
                .border_set(ratatui::symbols::border::ROUNDED)
                .border_style(Style::default().fg(theme::current().dim)),
        );
    f.render_widget(p, area);
}
