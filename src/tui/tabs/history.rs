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
        // Auto-import Claude Code sessions
        match mgr.db().import_claude_sessions() {
            Ok(n) if n > 0 => tracing::info!("Imported {} Claude Code sessions", n),
            Err(e) => tracing::warn!("Failed to import sessions: {}", e),
            _ => {}
        }

        let sessions = mgr.db().query_sessions(None, None, 200).unwrap_or_default();
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

        // Left: session list — 2-line items like claude --resume
        let items: Vec<ListItem> = self
            .sessions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let is_selected = self.state.selected() == Some(i);

                let raw = s.title.as_deref().unwrap_or(&s.id);
                let is_uuid = raw.len() >= 32 && raw.chars().filter(|c| *c == '-').count() >= 4;
                let title = if is_uuid {
                    std::path::Path::new(&s.project_path)
                        .file_name().map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| raw.to_string())
                } else {
                    raw.to_string()
                };
                let title = truncate(&title, 50);

                let project = std::path::Path::new(&s.project_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                let time_ago = relative_time(&s.start_time);
                let size = format_size(s.size_bytes);

                let arrow = if is_selected { "❯ " } else { "  " };
                let title_color = if is_selected { Theme::CYAN } else { Theme::FG };

                ListItem::new(vec![
                    Line::from(Span::styled(
                        format!("{}{}", arrow, title),
                        Style::default().fg(title_color),
                    )),
                    Line::from(vec![
                        Span::styled("    ", Style::default()),
                        Span::styled(time_ago, Style::default().fg(Theme::COMMENT)),
                        Span::styled(" · ", Style::default().fg(Theme::DIM)),
                        Span::styled(project, Style::default().fg(Theme::YELLOW)),
                        Span::styled(" · ", Style::default().fg(Theme::DIM)),
                        Span::styled(size, Style::default().fg(Theme::GREEN)),
                    ]),
                ])
            })
            .collect();

        let search_hint = if self.search_query.is_empty() {
            "/ to search".to_string()
        } else {
            format!("Search: \"{}\"", self.search_query)
        };

        let list = List::new(items)
            .block(
                Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                    .title(format!("Sessions ({})", search_hint))
                    .border_style(Style::default().fg(Theme::DIM)),
            )
            .highlight_style(Style::default());

        f.render_stateful_widget(list, chunks[0], &mut self.state);

        // Right: detail for selected session (always render bordered block)
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
                        Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                            .title("Session Detail")
                            .border_style(Style::default().fg(Theme::DIM)),
                    )
                    .style(Style::default());
                f.render_widget(p, chunks[1]);
            } else {
                render_empty_detail(f, chunks[1], "No session selected");
            }
        } else {
            render_empty_detail(f, chunks[1], "No sessions available");
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

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}...", s.chars().take(max - 3).collect::<String>())
    } else {
        s.to_string()
    }
}

fn relative_time(iso: &str) -> String {
    if iso.len() < 19 { return iso.to_string(); }
    let parsed = chrono::NaiveDateTime::parse_from_str(&iso[..19], "%Y-%m-%d %H:%M:%S");
    let dt = match parsed {
        Ok(d) => d.and_utc(),
        Err(_) => return iso[5..16].to_string(),
    };
    let dur = chrono::Utc::now() - dt;
    let mins = dur.num_minutes();
    let hrs = dur.num_hours();
    let days = dur.num_days();

    if mins < 1 { "just now".into() }
    else if mins < 60 { format!("{} min ago", mins) }
    else if hrs < 24 { format!("{} hours ago", hrs) }
    else if days < 7 { format!("{} days ago", days) }
    else if days < 30 { format!("{} weeks ago", days / 7) }
    else { format!("{} months ago", days / 30) }
}

fn format_size(bytes: i64) -> String {
    if bytes < 1024 { format!("{}B", bytes) }
    else if bytes < 1024 * 1024 { format!("{:.1}KB", bytes as f64 / 1024.0) }
    else { format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0)) }
}

fn render_empty_detail(f: &mut Frame, area: Rect, hint: &str) {
    use ratatui::{style::Style, text::Line, widgets::Block};
    let p = Paragraph::new(Line::from(Span::styled(
        hint,
        Style::default().fg(Theme::COMMENT),
    )))
    .block(
        Block::bordered()
            .border_set(ratatui::symbols::border::ROUNDED)
            .title("Session Detail")
            .border_style(Style::default().fg(Theme::DIM)),
    );
    f.render_widget(p, area);
}
