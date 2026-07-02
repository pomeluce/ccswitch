use std::sync::Arc;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState},
    Frame,
};
use crossterm::event::KeyCode;
use crate::core::config::ConfigManager;
use crate::db::sessions::SessionRecord;
use super::super::theme::Theme;
use super::super::widgets::session_detail::{render_session_detail, render_empty_detail};
use super::super::widgets::shared::{render_search_box as shared_search, render_shortcut_bar as shared_shortcuts, render_confirm_popup as shared_confirm, shortcut_lines, truncate, relative_time, format_size};
use super::TabContent;

#[derive(Clone, Copy, PartialEq)]
pub enum ConfirmAction { Open, Delete }

pub struct HistoryTab {
    pub all_sessions: Vec<SessionRecord>,
    pub sessions: Vec<SessionRecord>,
    pub state: ListState,
    pub search_query: String,
    pub is_searching: bool,
    pub confirm_action: Option<ConfirmAction>,
    pub needs_terminal_reinit: bool,
    pub launch_project: Option<String>,
    confirm_button: usize, // 0=Confirm, 1=Cancel
    mgr: Arc<ConfigManager>,
}

impl HistoryTab {
    pub fn new(mgr: Arc<ConfigManager>) -> Self {
        // Session import is handled before TUI launch (in main.rs with progress bar).
        // Just load whatever is already in the DB.
        let all = mgr.session_db().query_sessions(None, None, 200)
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.size_bytes > 0)
            .collect::<Vec<_>>();
        let sessions = all.clone();
        let mut state = ListState::default();
        if !sessions.is_empty() { state.select(Some(0)); }
        HistoryTab {
            all_sessions: all,
            sessions,
            state,
            search_query: String::new(),
            is_searching: false,
            confirm_action: None,
            needs_terminal_reinit: false,
            launch_project: None,
            confirm_button: 0,
            mgr,
        }
    }

    pub fn refresh(&mut self) {
        let query = self.search_query.trim().to_lowercase();
        let tokens: Vec<&str> = query.split_whitespace().collect();
        self.sessions = self.all_sessions
            .iter()
            .filter(|s| {
                if tokens.is_empty() { return true; }
                let haystack = format!(
                    "{} {}",
                    s.title.as_deref().unwrap_or(""),
                    s.project_path
                ).to_lowercase();
                tokens.iter().all(|t| haystack.contains(t))
            })
            .cloned()
            .collect();
        if self.state.selected().unwrap_or(0) >= self.sessions.len() {
            self.state.select(if self.sessions.is_empty() { None } else { Some(0) });
        }
    }

    fn delete_selected(&mut self) {
        if let Some(idx) = self.state.selected() {
            if idx < self.sessions.len() {
                if let Some(session) = self.sessions.get(idx) {
                    // Physically delete Claude Code session files
                    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                    let project_hash = session.project_path.replace('/', "-");
                    let jsonl_path = std::path::PathBuf::from(&home)
                        .join(".claude/projects")
                        .join(&project_hash)
                        .join(format!("{}.jsonl", session.id));
                    if let Err(e) = std::fs::remove_file(&jsonl_path) {
                        tracing::warn!("Failed to delete session file {:?}: {}", jsonl_path, e);
                    }

                    if let Err(e) = self.mgr.session_db().delete_session(&session.id) {
                        tracing::warn!("Failed to delete session from database: {}", e);
                    }
                    self.all_sessions.retain(|s| s.id != session.id);
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

    /// Signal App to launch claude (terminal suspend/restore handled by event loop)
    fn open_session(&mut self) {
        if let Some(idx) = self.state.selected() {
            if let Some(s) = self.sessions.get(idx) {
                self.needs_terminal_reinit = true;
                self.launch_project = Some(s.project_path.clone());
            }
        }
    }
}

impl TabContent for HistoryTab {
    fn render(&mut self, f: &mut Frame, area: Rect) {
        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        // Left panel: search box + session list
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(main[0]);

        // Right panel: detail preview + shortcut bar (dynamic height)
        let shortcut_lines = shortcut_lines(main[1].width, &[8, 9, 12, 7, 7]);
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(2 + shortcut_lines as u16), // borders + content
            ])
            .split(main[1]);

        // Search box
        self.render_search_box(f, left_chunks[0]);

        // Session list — 2-line items
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

                let date = if s.file_mtime.is_empty() {
                    relative_time(&s.start_time)
                } else {
                    relative_time(&s.file_mtime)
                };
                let size = format_size(s.size_bytes);

                let arrow = if is_selected { "❯ " } else { "  " };
                let title_color = if is_selected { Theme::CYAN } else { Theme::FG };

                ListItem::new(vec![
                    Line::from(Span::styled(
                        format!("{}{}", arrow, title),
                        Style::default().fg(title_color),
                    )),
                    Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(date, Style::default().fg(Theme::COMMENT)),
                        Span::styled(" · ", Style::default().fg(Theme::DIM)),
                        Span::styled(project, Style::default().fg(Theme::COMMENT)),
                        Span::styled(" · ", Style::default().fg(Theme::DIM)),
                        Span::styled(size, Style::default().fg(Theme::COMMENT)),
                    ]),
                    // Spacing between items
                    Line::from(""),
                ])
            })
            .collect();

        let count = items.len();
        let list = List::new(items)
            .block(
                Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                    .title(format!("Sessions ({})", count))
                    .border_style(Style::default().fg(Theme::DIM)),
            )
            .highlight_style(Style::default());

        f.render_stateful_widget(list, left_chunks[1], &mut self.state);

        // Right: detail preview + shortcut bar
        if let Some(idx) = self.state.selected() {
            if let Some(s) = self.sessions.get(idx) {
                render_session_detail(f, right_chunks[0], s);
            } else {
                render_empty_detail(f, right_chunks[0], "No session selected");
            }
        } else {
            render_empty_detail(f, right_chunks[0], "No sessions available");
        }

        // Shortcut bar under detail preview
        self.render_shortcut_bar(f, right_chunks[1]);

        // Confirmation popup
        if self.confirm_action.is_some() {
            self.render_confirm_popup(f, area);
        }
    }

    fn handle_key(&mut self, code: KeyCode) -> bool {
        // Confirm popup mode (open or delete)
        if self.confirm_action.is_some() {
            match code {
                KeyCode::Tab | KeyCode::Right |
                KeyCode::Char('j') | KeyCode::Char('l') => {
                    self.confirm_button = (self.confirm_button + 1) % 2;
                }
                KeyCode::BackTab | KeyCode::Left |
                KeyCode::Char('k') | KeyCode::Char('h') => {
                    self.confirm_button = if self.confirm_button == 0 { 1 } else { 0 };
                }
                KeyCode::Enter => {
                    if self.confirm_button == 0 {
                        match self.confirm_action {
                            Some(ConfirmAction::Open) => self.open_session(),
                            Some(ConfirmAction::Delete) => self.delete_selected(),
                            _ => {}
                        }
                    }
                    self.confirm_action = None;
                    self.confirm_button = 0;
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.confirm_action = None;
                    self.confirm_button = 0;
                }
                _ => {}
            }
            return true;
        }

        // Search mode
        if self.is_searching {
            match code {
                KeyCode::Esc => {
                    self.is_searching = false;
                    self.search_query.clear();
                    self.refresh();
                }
                KeyCode::Enter => {
                    self.is_searching = false;
                    if !self.sessions.is_empty() { self.state.select(Some(0)); }
                }
                KeyCode::Backspace | KeyCode::Delete => {
                    self.search_query.pop();
                    self.refresh();
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                    self.refresh();
                }
                _ => {}
            }
            return true;
        }

        // List mode
        match code {
            KeyCode::Char('j') | KeyCode::Down => {
                let len = self.sessions.len();
                if len == 0 { return true; }
                let i = self.state.selected().unwrap_or(0);
                self.state.select(Some(if i + 1 < len { i + 1 } else { 0 }));
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let len = self.sessions.len();
                if len == 0 { return true; }
                let i = self.state.selected().unwrap_or(0);
                self.state.select(Some(if i > 0 { i - 1 } else { len - 1 }));
            }
            KeyCode::Enter => {
                self.confirm_action = Some(ConfirmAction::Open);
                self.confirm_button = 0;
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                self.confirm_action = Some(ConfirmAction::Delete);
                self.confirm_button = 0;
            }
            KeyCode::Char('/') => {
                self.is_searching = true;
            }
            _ => { return false; }
        }
        true
    }
}

impl HistoryTab {
    fn render_confirm_popup(&self, f: &mut Frame, area: Rect) {
        let is_delete = self.confirm_action == Some(ConfirmAction::Delete);
        let (title, msg, c) = if is_delete {
            (" Confirm Delete ", " Delete this session? ", Theme::RED)
        } else {
            (" Open Session ", " Open this session in Claude Code? ", Theme::CYAN)
        };
        shared_confirm(f, area, title, msg, "Confirm", "Cancel", c, self.confirm_button);
    }

    fn render_shortcut_bar(&self, f: &mut Frame, area: Rect) {
        let groups = vec![
            vec![(" J/K ".into(), Theme::CYAN), ("Nav".into(), Theme::COMMENT)],
            vec![(" / ".into(), Theme::CYAN), ("Search".into(), Theme::COMMENT)],
            vec![(" ⏎  ".into(), Theme::GREEN), ("Open".into(), Theme::COMMENT)],
            vec![(" D ".into(), Theme::RED), ("Delete".into(), Theme::COMMENT)],
            vec![(" Q ".into(), Theme::ORANGE), ("Quit".into(), Theme::COMMENT)],
        ];
        shared_shortcuts(f, area, &groups);
    }

    fn render_search_box(&self, f: &mut Frame, area: Rect) {
        shared_search(f, area, &self.search_query, self.is_searching);
    }
}


