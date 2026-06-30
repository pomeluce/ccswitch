use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use super::super::theme::Theme;

pub fn render_status_bar(
    f: &mut Frame,
    area: Rect,
    active_provider: &str,
    active_profile: &str,
    proxy_running: bool,
    proxy_port: u16,
) {
    let proxy_status = if proxy_running {
        Span::styled(
            format!("\u{1f7e0} Proxy :{}", proxy_port),
            Style::default().fg(Theme::GREEN),
        )
    } else {
        Span::styled("\u{26ab} Proxy off", Style::default().fg(Theme::DIM))
    };

    let line = Line::from(vec![
        Span::styled(
            format!("\u{25cf} {} / {}  ", active_provider, active_profile),
            Style::default().fg(Theme::CYAN),
        ),
        Span::styled("|  ", Style::default().fg(Theme::COMMENT)),
        proxy_status,
        Span::styled("  |  ", Style::default().fg(Theme::COMMENT)),
        Span::styled(
            "1/2/3 tabs  j/k nav  \u{23ce} apply  / search  q quit",
            Style::default().fg(Theme::COMMENT),
        ),
    ]);

    let p = Paragraph::new(line).style(Style::default());
    f.render_widget(p, area);
}
