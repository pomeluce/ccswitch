use std::sync::Arc;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};
use crossterm::event::KeyCode;
use crate::core::config::ConfigManager;
use crate::db::sessions::SessionRecord;
use super::super::theme::Theme;
use super::TabContent;

pub struct HistoryTab {
    all_sessions: Vec<SessionRecord>,
    pub sessions: Vec<SessionRecord>,
    pub state: ListState,
    pub search_query: String,
    pub is_searching: bool,
    pub detail_mode: bool,
    pub confirm_delete: bool,
    selected_button: usize, // 0=Open, 1=Delete
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

        let all = mgr.db().query_sessions(None, None, 200)
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.size_bytes > 0)
            .collect::<Vec<_>>();
        let sessions = all.clone();
        let mut state = ListState::default();
        if !sessions.is_empty() {
            state.select(Some(0));
        }
        HistoryTab {
            all_sessions: all,
            sessions,
            state,
            search_query: String::new(),
            is_searching: false,
            detail_mode: false,
            confirm_delete: false,
            selected_button: 0,
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
        // Detail mode: full-width detail panel
        if self.detail_mode {
            self.render_detail_full(f, area);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(area);

        // Left panel: search box + session list
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(chunks[0]);

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

                let date = format_date(&s.start_time);
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
        // Delete confirmation mode
        if self.confirm_delete {
            match code {
                KeyCode::Char('j') | KeyCode::Char('l') | KeyCode::Right => {
                    self.selected_button ^= 1;
                }
                KeyCode::Char('k') | KeyCode::Char('h') | KeyCode::Left => {
                    self.selected_button ^= 1;
                }
                KeyCode::Enter => {
                    if self.selected_button == 0 {
                        // Confirm
                        self.delete_selected();
                    }
                    self.confirm_delete = false;
                    self.selected_button = 0;
                }
                KeyCode::Esc => {
                    self.confirm_delete = false;
                    self.selected_button = 0;
                }
                _ => {}
            }
            return;
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
            return;
        }

        // Detail mode
        if self.detail_mode {
            match code {
                KeyCode::Esc => {
                    self.detail_mode = false;
                }
                KeyCode::Char('j') | KeyCode::Char('l') | KeyCode::Right => {
                    self.selected_button = (self.selected_button + 1) % 2;
                }
                KeyCode::Char('k') | KeyCode::Char('h') | KeyCode::Left => {
                    self.selected_button = if self.selected_button == 0 { 1 } else { 0 };
                }
                KeyCode::Enter => {
                    match self.selected_button {
                        0 => { /* Open — resume session */ }
                        1 => {
                            self.confirm_delete = true;
                            self.selected_button = 0; // Confirm=0, Cancel=1
                        }
                        _ => {}
                    }
                }
                KeyCode::Char('d') => {
                    self.confirm_delete = true;
                    self.selected_button = 0;
                }
                _ => {}
            }
            return;
        }

        // List mode
        match code {
            KeyCode::Char('j') | KeyCode::Down => {
                let len = self.sessions.len();
                if len == 0 { return; }
                let i = self.state.selected().unwrap_or(0);
                self.state.select(Some(if i + 1 < len { i + 1 } else { 0 }));
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let len = self.sessions.len();
                if len == 0 { return; }
                let i = self.state.selected().unwrap_or(0);
                self.state.select(Some(if i > 0 { i - 1 } else { len - 1 }));
            }
            KeyCode::Enter => {
                self.detail_mode = true;
                self.selected_button = 0;
            }
            KeyCode::Char('/') => {
                self.is_searching = true;
            }
            KeyCode::Char('d') => {
                self.confirm_delete = true;
                self.selected_button = 0;
            }
            _ => {}
        }
    }
}

impl HistoryTab {
    fn render_detail_full(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)])
            .split(area);

        if let Some(idx) = self.state.selected() {
            if let Some(s) = self.sessions.get(idx) {
                let padding = "  ";
                let lines = vec![
                    Line::from(Span::styled(
                        s.title.as_deref().unwrap_or(&s.id),
                        Style::default().fg(Theme::CYAN),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled(format!("{}Project:  ", padding), Style::default().fg(Theme::PURPLE)),
                        Span::styled(&s.project_path, Style::default().fg(Theme::YELLOW)),
                    ]),
                    Line::from(vec![
                        Span::styled(format!("{}Profile:  ", padding), Style::default().fg(Theme::PURPLE)),
                        Span::styled(s.profile_id.as_deref().unwrap_or("-"), Style::default().fg(Theme::FG)),
                    ]),
                    Line::from(vec![
                        Span::styled(format!("{}Mode:     ", padding), Style::default().fg(Theme::PURPLE)),
                        Span::styled(&s.mode, Style::default().fg(Theme::GREEN)),
                    ]),
                    Line::from(vec![
                        Span::styled(format!("{}Tokens:   ", padding), Style::default().fg(Theme::PURPLE)),
                        Span::styled(
                            format!("{} prompt / {} completion", s.prompt_tokens, s.completion_tokens),
                            Style::default().fg(Theme::FG),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled(format!("{}Started:  ", padding), Style::default().fg(Theme::PURPLE)),
                        Span::styled(&s.start_time, Style::default().fg(Theme::DIM)),
                    ]),
                    Line::from(vec![
                        Span::styled(format!("{}Messages: ", padding), Style::default().fg(Theme::PURPLE)),
                        Span::styled(format!("{}", s.message_count), Style::default().fg(Theme::FG)),
                    ]),
                    Line::from(vec![
                        Span::styled(format!("{}Size:     ", padding), Style::default().fg(Theme::PURPLE)),
                        Span::styled(format_size(s.size_bytes), Style::default().fg(Theme::FG)),
                    ]),
                ];

                let p = Paragraph::new(lines)
                    .block(
                        Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                            .title(format!("Session Detail — Esc back"))
                            .border_style(Style::default().fg(Theme::DIM)),
                    );
                f.render_widget(p, chunks[0]);

                // Buttons
                self.render_buttons(f, chunks[1]);
            }
        }

        // Delete confirmation popup
        if self.confirm_delete {
            self.render_confirm_popup(f, area);
        }
    }

    fn render_buttons(&self, f: &mut Frame, area: Rect) {
        let open_style = if self.selected_button == 0 {
            Style::default().fg(Color::Black).bg(Theme::CYAN)
        } else {
            Style::default().fg(Theme::DIM)
        };
        let del_style = if self.selected_button == 1 {
            Style::default().fg(Color::Black).bg(Theme::RED)
        } else {
            Style::default().fg(Theme::DIM)
        };

        let line = Line::from(vec![
            Span::styled(" Open ", open_style),
            Span::styled("  ", Style::default()),
            Span::styled(" Delete ", del_style),
            Span::styled("  ", Style::default()),
            Span::styled("j/l ←/→ switch  ⏎ confirm", Style::default().fg(Theme::COMMENT)),
        ]);
        f.render_widget(Paragraph::new(line), area);
    }

    fn render_confirm_popup(&self, f: &mut Frame, area: Rect) {
        let popup_area = centered_rect(40, 5, area);
        let confirm_style = if self.selected_button == 0 {
            Style::default().fg(Color::Black).bg(Theme::RED)
        } else {
            Style::default().fg(Theme::DIM)
        };
        let cancel_style = if self.selected_button == 1 {
            Style::default().fg(Color::Black).bg(Theme::CYAN)
        } else {
            Style::default().fg(Theme::DIM)
        };

        let p = Paragraph::new(vec![
            Line::from("Delete this session?"),
            Line::from(""),
            Line::from(vec![
                Span::styled(" Confirm ", confirm_style),
                Span::styled("  ", Style::default()),
                Span::styled(" Cancel ", cancel_style),
            ]),
        ])
        .block(
            Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                .title("Confirm Delete")
                .border_style(Style::default().fg(Theme::RED)),
        );
        f.render_widget(Clear, popup_area); // clear behind
        f.render_widget(p, popup_area);
    }

    fn render_search_box(&self, f: &mut Frame, area: Rect) {
        let cursor = if self.is_searching { "▌" } else { "" };
        let text = if self.search_query.is_empty() && !self.is_searching {
            "⌕ Search (/ to focus)".to_string()
        } else if !self.search_query.is_empty() && !self.is_searching {
            format!("⌕ {} (/) — Esc to clear", self.search_query)
        } else {
            format!("⌕ {}{}", self.search_query, cursor)
        };
        let color = if self.is_searching { Theme::CYAN } else { Theme::COMMENT };
        let p = Paragraph::new(Line::from(Span::styled(text, Style::default().fg(color))))
            .block(
                Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                    .border_style(Style::default().fg(Theme::DIM)),
            );
        f.render_widget(p, area);
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}...", s.chars().take(max - 3).collect::<String>())
    } else {
        s.to_string()
    }
}

fn format_date(iso: &str) -> String {
    if iso.len() >= 16 { iso[5..16].to_string() } else { iso.to_string() }
}

fn centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let x = r.x + (r.width.saturating_sub(width)) / 2;
    let y = r.y + (r.height.saturating_sub(height)) / 2;
    Rect { x, y, width: width.min(r.width), height: height.min(r.height) }
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
