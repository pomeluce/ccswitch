use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};
use crate::core::models::Profile;
use super::super::theme;

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
        let pad = "  ";
        let active_tag = if is_active { " \u{2605} active" } else { "" };
        let source_tag = if can_delete { "user" } else { "system" };
        let masked_key = mask_api_key(api_key);

        let mut lines = vec![
            Line::from(vec![
                Span::styled(pad, Style::default()),
                Span::styled(
                    format!("{} / {}", provider_name, profile.name),
                    Style::default().fg(theme::current().cyan),
                ),
                Span::styled(active_tag, Style::default().fg(theme::current().yellow)),
                Span::styled(
                    format!("  [{}]", source_tag),
                    Style::default().fg(theme::current().comment),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(format!("{}Opus:     ", pad), Style::default().fg(theme::current().purple)),
                Span::styled(&profile.opus, Style::default().fg(theme::current().fg)),
            ]),
            Line::from(vec![
                Span::styled(format!("{}Sonnet:   ", pad), Style::default().fg(theme::current().purple)),
                Span::styled(&profile.sonnet, Style::default().fg(theme::current().fg)),
            ]),
            Line::from(vec![
                Span::styled(format!("{}Haiku:    ", pad), Style::default().fg(theme::current().purple)),
                Span::styled(&profile.haiku, Style::default().fg(theme::current().fg)),
            ]),
            Line::from(vec![
                Span::styled(format!("{}SubAgent: ", pad), Style::default().fg(theme::current().purple)),
                Span::styled(&profile.subagent, Style::default().fg(theme::current().fg)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(format!("{}URL:      ", pad), Style::default().fg(theme::current().purple)),
                Span::styled(api_url, Style::default().fg(theme::current().dim)),
            ]),
            Line::from(vec![
                Span::styled(format!("{}Key:      ", pad), Style::default().fg(theme::current().purple)),
                Span::styled(&masked_key, Style::default().fg(theme::current().green)),
            ]),
        ];
        // Continuation lines — align with value text (width of "  Key:      ")
        let indent = "            ";
        let max_w = (area.width as usize).saturating_sub(indent.len()).max(10);
        for &(val, color) in &[(api_url, theme::current().dim), (masked_key.as_str(), theme::current().green)] {
            if val.len() > max_w {
                let remainder: String = val.chars().skip(max_w).collect();
                for chunk in remainder.chars().collect::<Vec<_>>().chunks(max_w) {
                    let cont: String = chunk.iter().collect();
                    if !cont.is_empty() {
                        lines.push(Line::from(Span::styled(
                            format!("{}{}", indent, cont),
                            Style::default().fg(color),
                        )));
                    }
                }
            }
        }
        let p = Paragraph::new(lines)
            .block(
                Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                    .title("Detail")
                    .border_style(Style::default().fg(theme::current().dim)),
            )
            .style(Style::default());
        f.render_widget(p, area);
    }

    pub fn render_empty(f: &mut Frame, area: Rect, hint: &str) {
        let p = Paragraph::new(Line::from(Span::styled(
            hint,
            Style::default().fg(theme::current().comment),
        )))
        .block(
            Block::bordered()
                .border_set(ratatui::symbols::border::ROUNDED)
                .title("Detail")
                .border_style(Style::default().fg(theme::current().dim)),
        );
        f.render_widget(p, area);
    }
}

/// Mask literal API keys: show first 4 chars, replace rest with same-length ***
fn mask_api_key(key: &str) -> String {
    if key.starts_with("env:") || key.is_empty() { return key.to_string(); }
    if key.len() <= 4 { return "*".repeat(key.len()); }
    format!("{}{}", &key[..4], "*".repeat(key.len() - 4))
}
