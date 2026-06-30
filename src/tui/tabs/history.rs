use std::sync::Arc;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph},
    Frame,
};
use crossterm::event::KeyCode;
use crate::core::config::ConfigManager;
use crate::db::sessions::SessionRecord;
use super::super::theme::Theme;
use super::TabContent;

pub struct HistoryTab {
    pub sessions: Vec<SessionRecord>,
    pub state: ListState,
    pub search_query: String,
    mgr: Arc<ConfigManager>,
}

impl HistoryTab {
    pub fn new(mgr: Arc<ConfigManager>) -> Self {
        let sessions = mgr.db().query_sessions(None, None, 100).unwrap_or_default();
        let mut state = ListState::default();
        if !sessions.is_empty() {
            state.select(Some(0));
        }
        HistoryTab {
            sessions,
            state,
            search_query: String::new(),
            mgr,
        }
    }

    #[allow(dead_code)]
    pub fn refresh(&mut self) {
        let search = if self.search_query.is_empty() {
            None
        } else {
            Some(self.search_query.as_str())
        };
        self.sessions = self.mgr.db().query_sessions(None, search, 100).unwrap_or_default();
    }

    fn delete_selected(&mut self) {
        if let Some(idx) = self.state.selected() {
            if idx < self.sessions.len() {
                // Delete from the database first
                if let Some(session) = self.sessions.get(idx) {
                    if let Err(e) = self.mgr.db().delete_session(&session.id) {
                        tracing::warn!("Failed to delete session from database: {}", e);
                    }
                }
                self.sessions.remove(idx);
                let len = self.sessions.len();
                if len == 0 {
                    self.state.select(None);
                } else if idx >= len {
                    self.state.select(Some(len - 1));
                } else {
                    self.state.select(Some(idx));
                }
            }
        }
    }
}

impl TabContent for HistoryTab {
    fn render(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(area);

        // Left: session list
        let items: Vec<ListItem> = self
            .sessions
            .iter()
            .map(|s| {
                let date = if s.start_time.len() >= 16 {
                    &s.start_time[5..16]
                } else {
                    &s.start_time
                };
                let title = s.title.as_deref().unwrap_or(&s.id);
                let project = std::path::Path::new(&s.project_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let tokens = s.prompt_tokens + s.completion_tokens;

                let line = Line::from(vec![
                    Span::styled(
                        format!("{}  ", date),
                        Style::default().fg(Theme::COMMENT),
                    ),
                    Span::styled(title.to_string(), Style::default().fg(Theme::FG)),
                    Span::styled(
                        format!("  {}", project),
                        Style::default().fg(Theme::YELLOW),
                    ),
                    Span::styled(
                        format!("  {}t", tokens),
                        Style::default().fg(Theme::GREEN),
                    ),
                ]);
                ListItem::new(line)
            })
            .collect();

        let search_hint = if self.search_query.is_empty() {
            "/ to search".to_string()
        } else {
            format!("Search: \"{}\"", self.search_query)
        };

        let list = List::new(items)
            .block(
                Block::bordered()
                    .title(format!("Sessions ({})", search_hint))
                    .border_style(Style::default().fg(Theme::DIM)),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        f.render_stateful_widget(list, chunks[0], &mut self.state);

        // Right: detail for selected session
        if let Some(idx) = self.state.selected() {
            if let Some(s) = self.sessions.get(idx) {
                let lines = vec![
                    Line::from(vec![Span::styled(
                        s.title.as_deref().unwrap_or(&s.id),
                        Style::default().fg(Theme::CYAN),
                    )]),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("Project:  ", Style::default().fg(Theme::PURPLE)),
                        Span::styled(
                            &s.project_path,
                            Style::default().fg(Theme::YELLOW),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("Profile:  ", Style::default().fg(Theme::PURPLE)),
                        Span::styled(
                            s.profile_id.as_deref().unwrap_or("-"),
                            Style::default().fg(Theme::FG),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("Mode:     ", Style::default().fg(Theme::PURPLE)),
                        Span::styled(&s.mode, Style::default().fg(Theme::GREEN)),
                    ]),
                    Line::from(vec![
                        Span::styled("Tokens:   ", Style::default().fg(Theme::PURPLE)),
                        Span::styled(
                            format!("{} prompt / {} completion", s.prompt_tokens, s.completion_tokens),
                            Style::default().fg(Theme::FG),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("Started:  ", Style::default().fg(Theme::PURPLE)),
                        Span::styled(&s.start_time, Style::default().fg(Theme::DIM)),
                    ]),
                    Line::from(vec![
                        Span::styled("Messages: ", Style::default().fg(Theme::PURPLE)),
                        Span::styled(
                            format!("{}", s.message_count),
                            Style::default().fg(Theme::FG),
                        ),
                    ]),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled(
                            "r Resume  ",
                            Style::default().add_modifier(Modifier::REVERSED),
                        ),
                        Span::styled(
                            "  d Delete",
                            Style::default().fg(Theme::RED),
                        ),
                    ]),
                ];

                let p = Paragraph::new(lines)
                    .block(
                        Block::bordered()
                            .title("Session Detail")
                            .border_style(Style::default().fg(Theme::DIM)),
                    )
                    .style(Style::default());
                f.render_widget(p, chunks[1]);
            }
        }
    }

    fn handle_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('j') | KeyCode::Down => {
                let i = self.state.selected().unwrap_or(0);
                if i < self.sessions.len().saturating_sub(1) {
                    self.state.select(Some(i + 1));
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let i = self.state.selected().unwrap_or(0);
                if i > 0 {
                    self.state.select(Some(i - 1));
                }
            }
            KeyCode::Char('/') => {
                // Enter search mode — simplified: just clear search for now
                self.search_query = String::new();
            }
            KeyCode::Char('d') => {
                self.delete_selected();
            }
            _ => {}
        }
    }
}
