use crate::tui::theme;
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

pub fn render_scan_progress(
    f: &mut Frame,
    area: Rect,
    files_done: usize,
    files_total: usize,
    records: usize,
) {
    let pct = if files_total > 0 {
        (files_done as f64 / files_total as f64 * 100.0) as usize
    } else {
        0
    };
    let bar_w = if files_total > 0 {
        ((files_done as f64 / files_total.max(1) as f64) * 30.0) as usize
    } else {
        0
    };
    let bar_w = bar_w.min(30);
    let filled = "\u{2588}".repeat(bar_w);
    let empty = "\u{2591}".repeat(30usize.saturating_sub(bar_w));
    let bar = format!("{}{}", filled, empty);

    let spinner = ["\u{280b}","\u{2819}","\u{2839}","\u{2833}","\u{2827}","\u{280f}","\u{281f}","\u{283f}"][files_done % 8];

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("{}  Scanning Claude Code sessions...", spinner),
            Style::default().fg(theme::current().purple),
        )).centered(),
        Line::from(""),
        Line::from(Span::styled(
            format!("{} {}%", bar, pct),
            Style::default().fg(theme::current().comment),
        )).centered(),
        Line::from(Span::styled(
            format!("{} / {} files — {} records imported", files_done, files_total, records),
            Style::default().fg(theme::current().comment),
        )).centered(),
        Line::from(""),
        Line::from(Span::styled(
            "Data refreshes automatically when complete",
            Style::default().fg(theme::current().dim),
        )).centered(),
    ];

    let p = Paragraph::new(lines).block(
        Block::bordered()
            .border_set(ratatui::symbols::border::ROUNDED)
            .title(" Scanning ")
            .border_style(Style::default().fg(theme::current().purple)),
    );
    f.render_widget(p, area);
}
