use super::super::theme;
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

const APP_TABS: &[&str] = &["claude", "codex"];

pub fn render_app_bar(f: &mut Frame, area: Rect, app_type: &str) {
    let mut spans: Vec<Span> = Vec::new();
    for (i, app) in APP_TABS.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(
                " | ",
                Style::default().fg(theme::current().dim),
            ));
        }
        let style = if *app == app_type {
            Style::default().fg(theme::current().cyan)
        } else {
            Style::default().fg(theme::current().dim)
        };
        // Capitalize first letter for display
        let label = if *app == "claude" { "Claude" } else { "Codex" };
        spans.push(Span::styled(format!(" {} ", label), style));
    }

    let p = Paragraph::new(Line::from(spans)).centered().block(
        Block::bordered()
            .border_set(ratatui::symbols::border::ROUNDED)
            .border_style(Style::default().fg(theme::current().dim)),
    );
    f.render_widget(p, area);
}

/// Check if app_type is valid. Returns the toggled value (claude ↔ codex).
pub fn toggle_app_type(current: &str) -> &'static str {
    if current == "codex" { "claude" } else { "codex" }
}
