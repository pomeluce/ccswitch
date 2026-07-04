use super::super::super::theme;
use super::super::super::widgets::shared::centered_rect;
use crossterm::event::KeyCode;
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
    Frame,
};

pub struct EditForm {
    pub fields: [String; 5],
    pub cursors: [usize; 5],
    pub focused: usize,
    pub prov_id: String,
    pub prof_id: String,
}

pub const EDIT_LABELS: [&str; 5] = [
    "Profile Name",
    "Opus model",
    "Sonnet model",
    "Haiku model",
    "SubAgent model",
];

impl EditForm {
    pub fn handle_key(&mut self, code: KeyCode) -> bool {
        let field = &mut self.fields[self.focused];
        let cur = &mut self.cursors[self.focused];
        match code {
            KeyCode::Enter => return false, // signal commit
            KeyCode::Esc => return false,   // signal cancel
            KeyCode::Tab => {
                self.focused = (self.focused + 1) % 5;
            }
            KeyCode::BackTab => {
                self.focused = if self.focused == 0 { 4 } else { self.focused - 1 };
            }
            KeyCode::Left => {
                *cur = cur.saturating_sub(1);
            }
            KeyCode::Right => {
                *cur = (*cur + 1).min(field.len());
            }
            KeyCode::Home => {
                *cur = 0;
            }
            KeyCode::End => {
                *cur = field.len();
            }
            KeyCode::Backspace => {
                if *cur > 0 {
                    *cur -= 1;
                    field.remove(*cur);
                }
            }
            KeyCode::Delete => {
                if *cur < field.len() {
                    field.remove(*cur);
                }
            }
            KeyCode::Char(c) => {
                field.insert(*cur, c);
                *cur += 1;
            }
            _ => {}
        }
        true
    }
}

pub fn render_edit_form(form: &EditForm, f: &mut Frame, area: Rect) {
    let popup = centered_rect(60, 20, area);
    let inner_w = popup.width.saturating_sub(2) as usize;
    let pad_w = (inner_w.saturating_sub(40)) / 2;
    let pad = " ".repeat(pad_w);
    let value_w = inner_w.saturating_sub(pad_w * 2 + 17);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(""));
    for (i, label) in EDIT_LABELS.iter().enumerate() {
        let val = &form.fields[i];
        let pos = form.cursors[i].min(val.len());
        let vis = slice_value(val, pos, value_w);
        let cur = (pos - vis.skip).min(vis.text.len());
        let (left, right) = vis.text.split_at(cur);
        let cursor = if i == form.focused { "▌" } else { "" };
        let style = if i == form.focused {
            Style::default().fg(theme::current().cyan)
        } else {
            Style::default().fg(theme::current().fg)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{}{:<15}: ", pad, label), Style::default().fg(theme::current().fg)),
            Span::styled(left.to_string(), style),
            Span::styled(cursor.to_string(), style),
            Span::styled(right.to_string(), style),
            Span::styled(pad.clone(), Style::default()),
        ]));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(
        Line::from(vec![
            Span::styled(" Enter ", Style::default().fg(theme::current().comment)),
            Span::styled(" Save  ", Style::default().fg(theme::current().comment)),
            Span::styled(" Esc ", Style::default().fg(theme::current().dim)),
            Span::styled(" Cancel  ", Style::default().fg(theme::current().comment)),
            Span::styled(" Tab ", Style::default().fg(theme::current().comment)),
            Span::styled(" Next field", Style::default().fg(theme::current().comment)),
        ])
        .centered(),
    );

    let p = Paragraph::new(lines).block(
        Block::bordered()
            .border_set(ratatui::symbols::border::ROUNDED)
            .title(Line::from(" Edit Profile ").centered())
            .border_style(Style::default().fg(theme::current().cyan)),
    );
    f.render_widget(Clear, popup);
    f.render_widget(p, popup);
}

struct VisSlice {
    text: String,
    skip: usize,
}

fn slice_value(text: &str, cursor: usize, max_w: usize) -> VisSlice {
    if text.len() <= max_w || max_w < 4 {
        return VisSlice {
            text: text.to_string(),
            skip: 0,
        };
    }
    let half = max_w / 2;
    let mut start = if cursor > half { cursor - half } else { 0 };
    let end = (start + max_w).min(text.len());
    if end == text.len() && end > max_w {
        start = end - max_w;
    }
    VisSlice {
        text: text[start..end].to_string(),
        skip: start,
    }
}
