use super::super::tabs::Tab;
use super::super::theme;
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

pub fn render_sidebar(f: &mut Frame, area: Rect, active_tab: Tab, proxy_running: bool) {
    let tabs = [
        (Tab::Providers, "模型"),
        (Tab::Usage, "用量"),
        (Tab::History, "会话"),
        (Tab::Settings, "设置"),
    ];

    let mode_label = if proxy_running {
        "proxy"
    } else {
        "local"
    };

    let tab_lines = (tabs.len() * 2) as u16; // each tab + blank line
    let header_lines = 2u16;  // title + blank
    let footer_lines = 1u16;  // mode
    let inner_h = area.height.saturating_sub(2); // border
    let avail = inner_h.saturating_sub(header_lines + footer_lines);
    let pad_top = avail.saturating_sub(tab_lines) / 2;
    let pad_bottom = avail.saturating_sub(tab_lines + pad_top);

    let mut lines: Vec<Line> = Vec::new();
    // Title (centered by paragraph)
    lines.push(Line::from(Span::styled(
        "ccswitch-tui",
        Style::default().fg(theme::current().dim),
    )));
    lines.push(Line::from(""));

    for _ in 0..pad_top {
        lines.push(Line::from(""));
    }

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
