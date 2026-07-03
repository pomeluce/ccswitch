use super::shared::{format_size, format_tokens};
use crate::db::sessions::SessionRecord;
use crate::tui::theme::Theme;
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

/// Render a session detail panel in the given area.
pub fn render_session_detail(f: &mut Frame, area: Rect, session: &SessionRecord, tokens: Option<(i64, i64)>) {
    let pad = "  ";
    let home = std::env::var("HOME").unwrap_or_default();
    let path_short = session.project_path.replace(&home, "~");
    let label = format!("{}Project:  ", pad);
    let path_start_len = (area.width as usize).saturating_sub(14);
    let (first_part, rest_lines) = split_path(&path_short, path_start_len);

    let mut lines = vec![
        Line::from(vec![
            Span::styled(pad, Style::default()),
            Span::styled(session.title.as_deref().unwrap_or(&session.id), Style::default().fg(Theme::CYAN)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(label, Style::default().fg(Theme::PURPLE)),
            Span::styled(&first_part, Style::default().fg(Theme::YELLOW)),
        ]),
    ];

    let cont_indent = format!("{}           ", pad);
    for rest in rest_lines {
        lines.push(Line::from(Span::styled(format!("{}{}", cont_indent, rest), Style::default().fg(Theme::YELLOW))));
    }

    lines.extend(vec![
        Line::from(vec![
            Span::styled(format!("{}Profile:  ", pad), Style::default().fg(Theme::PURPLE)),
            Span::styled(session.profile_id.as_deref().unwrap_or("-"), Style::default().fg(Theme::FG)),
        ]),
        Line::from(vec![
            Span::styled(format!("{}Mode:     ", pad), Style::default().fg(Theme::PURPLE)),
            Span::styled(&session.mode, Style::default().fg(Theme::GREEN)),
        ]),
        Line::from(vec![
            Span::styled(format!("{}Tokens:   ", pad), Style::default().fg(Theme::PURPLE)),
            Span::styled(
                if let Some((p, c)) = tokens {
                    format!("{} prompt / {} completion", format_tokens(p), format_tokens(c))
                } else {
                    format!("{} prompt / {} completion", format_tokens(session.prompt_tokens), format_tokens(session.completion_tokens))
                },
                Style::default().fg(Theme::FG),
            ),
        ]),
        Line::from(vec![
            Span::styled(format!("{}Started:  ", pad), Style::default().fg(Theme::PURPLE)),
            Span::styled(&session.start_time, Style::default().fg(Theme::DIM)),
        ]),
        Line::from(vec![
            Span::styled(format!("{}Messages: ", pad), Style::default().fg(Theme::PURPLE)),
            Span::styled(format!("{}", session.message_count), Style::default().fg(Theme::FG)),
        ]),
        Line::from(vec![
            Span::styled(format!("{}Size:     ", pad), Style::default().fg(Theme::PURPLE)),
            Span::styled(format_size(session.size_bytes), Style::default().fg(Theme::FG)),
        ]),
    ]);

    let p = Paragraph::new(lines)
        .block(
            Block::bordered()
                .border_set(ratatui::symbols::border::ROUNDED)
                .title("Session Detail")
                .border_style(Style::default().fg(Theme::DIM)),
        )
        .style(Style::default());
    f.render_widget(p, area);
}

/// Render empty detail placeholder
pub fn render_empty_detail(f: &mut Frame, area: Rect, hint: &str) {
    let p = Paragraph::new(vec![Line::from(""), Line::from(Span::styled(hint, Style::default().fg(Theme::COMMENT))).centered()]).block(
        Block::bordered()
            .border_set(ratatui::symbols::border::ROUNDED)
            .title("Session Detail")
            .border_style(Style::default().fg(Theme::DIM)),
    );
    f.render_widget(p, area);
}

/// Split a long path into first line + continuation lines at word boundaries
fn split_path(path: &str, max_w: usize) -> (String, Vec<String>) {
    if path.len() <= max_w {
        return (path.to_string(), vec![]);
    }
    let mut rest: Vec<String> = Vec::new();
    let mut remaining = path;
    let first;
    loop {
        if remaining.len() <= max_w {
            first = remaining.to_string();
            break;
        }
        let cut = &remaining[..max_w];
        if let Some(sep) = cut.rfind('/') {
            let part = &remaining[..=sep];
            if rest.is_empty() && part.len() < max_w / 2 {
                // First line too short — just cut at max_w
                first = remaining[..max_w].to_string();
                rest.push(remaining[max_w..].to_string());
                break;
            }
            rest.push(part.trim_end_matches('/').to_string());
            remaining = &remaining[sep + 1..];
        } else {
            rest.push(remaining[max_w..].to_string());
            first = remaining[..max_w].to_string();
            break;
        }
    }
    rest.reverse();
    (first, rest)
}
