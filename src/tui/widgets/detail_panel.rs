use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};
use crate::core::models::Profile;
use super::super::theme::Theme;

pub struct DetailPanel;

impl DetailPanel {
    #[allow(clippy::too_many_arguments)]
    pub fn render_profile_detail(
        f: &mut Frame,
        area: Rect,
        provider_name: &str,
        profile: &Profile,
        api_url: &str,
        api_key: &str,
        is_active: bool,
        can_delete: bool,
    ) {
        let active_tag = if is_active { " \u{2605} active" } else { "" };
        let source_tag = if can_delete { "user" } else { "system" };

        let lines = vec![
            Line::from(vec![
                Span::styled(
                    format!("{} / {}", provider_name, profile.name),
                    Style::default().fg(Theme::CYAN),
                ),
                Span::styled(active_tag, Style::default().fg(Theme::YELLOW)),
                Span::styled(
                    format!("  [{}]", source_tag),
                    Style::default().fg(Theme::COMMENT),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Opus:     ", Style::default().fg(Theme::PURPLE)),
                Span::styled(&profile.opus, Style::default().fg(Theme::FG)),
            ]),
            Line::from(vec![
                Span::styled("Sonnet:   ", Style::default().fg(Theme::PURPLE)),
                Span::styled(&profile.sonnet, Style::default().fg(Theme::FG)),
            ]),
            Line::from(vec![
                Span::styled("Haiku:    ", Style::default().fg(Theme::PURPLE)),
                Span::styled(&profile.haiku, Style::default().fg(Theme::FG)),
            ]),
            Line::from(vec![
                Span::styled("SubAgent: ", Style::default().fg(Theme::PURPLE)),
                Span::styled(&profile.subagent, Style::default().fg(Theme::FG)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("URL:      ", Style::default().fg(Theme::PURPLE)),
                Span::styled(api_url, Style::default().fg(Theme::DIM)),
            ]),
            Line::from(vec![
                Span::styled("Key:      ", Style::default().fg(Theme::PURPLE)),
                Span::styled(api_key, Style::default().fg(Theme::GREEN)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    " \u{23ce} Apply  ",
                    Style::default().add_modifier(Modifier::REVERSED),
                ),
                Span::styled(
                    " e Edit  ",
                    Style::default().add_modifier(Modifier::REVERSED),
                ),
                Span::styled(
                    " d Delete",
                    Style::default().fg(Theme::RED),
                ),
            ]),
        ];

        let p = Paragraph::new(lines)
            .block(
                Block::bordered()
                    .title("Detail")
                    .border_style(Style::default().fg(Theme::DIM)),
            )
            .style(Style::default());
        f.render_widget(p, area);
    }
}
