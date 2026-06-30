use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};
use super::super::theme::Theme;

pub fn render_bar_chart(
    f: &mut Frame,
    area: Rect,
    data: &[(String, i64, bool)], // (label, value, is_highlight)
    title: &str,
) {
    let max_val = data.iter().map(|(_, v, _)| *v).max().unwrap_or(1);
    let width = area.width.saturating_sub(10) as usize;

    let lines: Vec<Line> = data
        .iter()
        .map(|(label, value, highlight)| {
            let bar_len = ((*value as f64 / max_val as f64) * width as f64) as usize;
            let bar = "█".repeat(bar_len.min(width));
            let color = if *highlight {
                Theme::YELLOW
            } else {
                Theme::PURPLE
            };
            Line::from(vec![
                Span::styled(
                    format!("{:>4} ", label),
                    Style::default().fg(Theme::COMMENT),
                ),
                Span::styled(bar, Style::default().fg(color)),
                Span::styled(
                    format!(" {}", value),
                    Style::default().fg(if *highlight {
                        Theme::CYAN
                    } else {
                        Theme::DIM
                    }),
                ),
            ])
        })
        .collect();

    let p = Paragraph::new(lines)
        .block(
            Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                .title(title)
                .border_style(Style::default().fg(Theme::DIM)),
        )
        .style(Style::default());
    f.render_widget(p, area);
}
