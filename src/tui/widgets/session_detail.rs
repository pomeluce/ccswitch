use super::shared::{format_size, format_tokens};
use crate::db::sessions::SessionRecord;
use crate::tui::theme;
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
            Span::styled(session.title.as_deref().unwrap_or(&session.id), Style::default().fg(theme::current().cyan)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(label, Style::default().fg(theme::current().purple)),
            Span::styled(&first_part, Style::default().fg(theme::current().yellow)),
        ]),
    ];

    let cont_indent = format!("{}           ", pad);
    for rest in rest_lines {
        lines.push(Line::from(Span::styled(format!("{}{}", cont_indent, rest), Style::default().fg(theme::current().yellow))));
    }

    lines.extend(vec![
        Line::from(vec![
            Span::styled(format!("{}Profile:  ", pad), Style::default().fg(theme::current().purple)),
            Span::styled(session.profile_id.as_deref().unwrap_or("-"), Style::default().fg(theme::current().fg)),
        ]),
        Line::from(vec![
            Span::styled(format!("{}Mode:     ", pad), Style::default().fg(theme::current().purple)),
            Span::styled(&session.mode, Style::default().fg(theme::current().green)),
        ]),
        Line::from(vec![
            Span::styled(format!("{}Tokens:   ", pad), Style::default().fg(theme::current().purple)),
            Span::styled(
                if let Some((p, c)) = tokens {
                    format!("{} prompt / {} completion", format_tokens(p), format_tokens(c))
                } else {
                    format!("{} prompt / {} completion", format_tokens(session.prompt_tokens), format_tokens(session.completion_tokens))
                },
                Style::default().fg(theme::current().fg),
            ),
        ]),
        Line::from(vec![
            Span::styled(format!("{}Started:  ", pad), Style::default().fg(theme::current().purple)),
            Span::styled(&session.start_time, Style::default().fg(theme::current().dim)),
        ]),
        Line::from(vec![
            Span::styled(format!("{}Messages: ", pad), Style::default().fg(theme::current().purple)),
            Span::styled(format!("{}", session.message_count), Style::default().fg(theme::current().fg)),
        ]),
        Line::from(vec![
            Span::styled(format!("{}Size:     ", pad), Style::default().fg(theme::current().purple)),
            Span::styled(format_size(session.size_bytes), Style::default().fg(theme::current().fg)),
        ]),
    ]);

    let p = Paragraph::new(lines)
        .block(
            Block::bordered()
                .border_set(ratatui::symbols::border::ROUNDED)
                .title("Session Detail")
                .border_style(Style::default().fg(theme::current().dim)),
        )
        .style(Style::default());
    f.render_widget(p, area);
}

/// Render empty detail placeholder
pub fn render_empty_detail(f: &mut Frame, area: Rect, hint: &str) {
    let p = Paragraph::new(vec![Line::from(""), Line::from(Span::styled(hint, Style::default().fg(theme::current().comment))).centered()]).block(
        Block::bordered()
            .border_set(ratatui::symbols::border::ROUNDED)
            .title("Session Detail")
            .border_style(Style::default().fg(theme::current().dim)),
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
            // If the part before separator is too short (< max_w/2), just cut at max_w
            if part.len() < max_w / 2 {
                rest.push(remaining[max_w..].to_string());
                first = remaining[..max_w].to_string();
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
