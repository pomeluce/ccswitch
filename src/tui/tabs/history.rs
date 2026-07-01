use std::sync::Arc;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph},
    Frame,
};
use crossterm::event::KeyCode;
use crate::core::config::ConfigManager;
use crate::db::sessions::SessionRecord;
use super::super::theme::Theme;
use super::super::widgets::shared::{render_search_box as shared_search, render_shortcut_bar as shared_shortcuts, render_confirm_popup as shared_confirm, shortcut_lines};
use super::TabContent;

#[derive(Clone, Copy, PartialEq)]
pub enum ConfirmAction { Open, Delete }

pub struct HistoryTab {
    all_sessions: Vec<SessionRecord>,
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
        match mgr.db().import_claude_sessions() {
            Ok(n) if n > 0 => tracing::info!("Imported {} Claude Code sessions", n),
            Err(e) => tracing::warn!("Failed to import sessions: {}", e),
            _ => {}
        }

        let all = mgr.db().query_sessions(None, None, 200)
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

                    if let Err(e) = self.mgr.db().delete_session(&session.id) {
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

                let date = relative_time(&s.start_time);
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
                let pad = "  ";
                let home = std::env::var("HOME").unwrap_or_default();
                let path_short = s.project_path.replace(&home, "~");
                let label = format!("{}Project:  ", pad);
                let path_start_len = (right_chunks[0].width as usize).saturating_sub(14);
                let (first_part, rest_lines) = split_path(&path_short, path_start_len);

                let mut lines = vec![
                    Line::from(vec![
                        Span::styled(pad, Style::default()),
                        Span::styled(
                            s.title.as_deref().unwrap_or(&s.id),
                            Style::default().fg(Theme::CYAN),
                        ),
                    ]),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled(label, Style::default().fg(Theme::PURPLE)),
                        Span::styled(&first_part, Style::default().fg(Theme::YELLOW)),
                    ]),
                ];
                // Continuation lines for long paths
                let cont_indent = format!("{}           ", pad);
                for rest in rest_lines {
                    lines.push(Line::from(Span::styled(
                        format!("{}{}", cont_indent, rest),
                        Style::default().fg(Theme::YELLOW),
                    )));
                }
                lines.extend(vec![
                    Line::from(vec![
                        Span::styled(format!("{}Profile:  ", pad), Style::default().fg(Theme::PURPLE)),
                        Span::styled(s.profile_id.as_deref().unwrap_or("-"), Style::default().fg(Theme::FG)),
                    ]),
                    Line::from(vec![
                        Span::styled(format!("{}Mode:     ", pad), Style::default().fg(Theme::PURPLE)),
                        Span::styled(&s.mode, Style::default().fg(Theme::GREEN)),
                    ]),
                    Line::from(vec![
                        Span::styled(format!("{}Tokens:   ", pad), Style::default().fg(Theme::PURPLE)),
                        Span::styled(format!("{} prompt / {} completion", s.prompt_tokens, s.completion_tokens), Style::default().fg(Theme::FG)),
                    ]),
                    Line::from(vec![
                        Span::styled(format!("{}Started:  ", pad), Style::default().fg(Theme::PURPLE)),
                        Span::styled(&s.start_time, Style::default().fg(Theme::DIM)),
                    ]),
                    Line::from(vec![
                        Span::styled(format!("{}Messages: ", pad), Style::default().fg(Theme::PURPLE)),
                        Span::styled(format!("{}", s.message_count), Style::default().fg(Theme::FG)),
                    ]),
                    Line::from(vec![
                        Span::styled(format!("{}Size:     ", pad), Style::default().fg(Theme::PURPLE)),
                        Span::styled(format_size(s.size_bytes), Style::default().fg(Theme::FG)),
                    ]),
                ]);

                let p = Paragraph::new(lines)
                    .block(
                        Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                            .title("Session Detail")
                            .border_style(Style::default().fg(Theme::DIM)),
                    )
                    .style(Style::default());
                f.render_widget(p, right_chunks[0]);
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

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}...", s.chars().take(max - 3).collect::<String>())
    } else {
        s.to_string()
    }
}

fn relative_time(iso: &str) -> String {
    if iso.len() < 19 { return iso[5..16].to_string(); }
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

/// How many content lines the shortcut bar needs at this width


/// Split path: first `max_width` chars on first line, remainder on continuation lines
fn split_path(path: &str, max_width: usize) -> (String, Vec<String>) {
    let max = max_width.max(10);
    if path.len() <= max { return (path.to_string(), vec![]); }
    let first: String = path.chars().take(max).collect();
    let remainder: String = path.chars().skip(max).collect();
    let cont_width = max.max(10);
    let rest = remainder.chars().collect::<Vec<_>>()
        .chunks(cont_width)
        .map(|c| c.iter().collect::<String>())
        .collect();
    (first, rest)
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
