use crate::tui::theme;
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

/// Render a simple app bar showing the current app (only Claude).
pub fn render_app_bar(f: &mut Frame, area: Rect) {
    let inner_w = area.width.saturating_sub(2) as usize;
    let label = " Claude ";
    let dw = label.chars().count(); // ASCII only
    let pad = " ".repeat(inner_w.saturating_sub(dw) / 2);

    let block = Block::bordered()
        .border_set(ratatui::symbols::border::ROUNDED)
        .border_style(Style::default().fg(theme::current().dim));

    let p = Paragraph::new(Line::from(Span::styled(
        format!("{}{}", pad, label),
        Style::default().fg(theme::current().cyan),
    )))
    .block(block);
    f.render_widget(p, area);
}
