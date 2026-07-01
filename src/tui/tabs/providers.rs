use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph},
    Frame,
};
use crossterm::event::KeyCode;
use crate::core::config::ConfigManager;
use crate::core::models::Provider;
use super::super::widgets::detail_panel::DetailPanel;
use super::TabContent;
use super::super::theme::Theme;

pub struct ProvidersTab {
    all_profiles: Vec<(Provider, crate::core::models::Profile)>,
    filtered: Vec<usize>,
    pub state: ListState,
    pub search_query: String,
    pub is_searching: bool,
    pub active_provider: String,
    pub active_profile: String,
    pub proxy_running: bool,
    pub proxy_port: u16,
}

impl ProvidersTab {
    pub fn new(mgr: &ConfigManager) -> Self {
        let providers = mgr.list_providers().unwrap_or_default();
        let mut all_profiles = Vec::new();
        for p in &providers {
            for pr in &p.profiles {
                all_profiles.push((p.clone(), pr.clone()));
            }
        }

        let active_provider = mgr.db().get_setting("active_provider").unwrap_or_default();
        let active_profile = mgr.db().get_setting("active_profile").unwrap_or_default();
        let proxy_running = mgr.db().get_setting("proxy_mode").map(|v| v == "true").unwrap_or(false);
        let proxy_port = mgr.db().get_setting("proxy_port").and_then(|s| s.parse().ok()).unwrap_or(15721);

        let filtered: Vec<usize> = (0..all_profiles.len()).collect();
        let mut state = ListState::default();
        if !filtered.is_empty() { state.select(Some(0)); }

        ProvidersTab {
            all_profiles,
            filtered,
            state,
            search_query: String::new(),
            is_searching: false,
            active_provider,
            active_profile,
            proxy_running,
            proxy_port,
        }
    }

    pub fn refresh_filter(&mut self) {
        let q = self.search_query.trim().to_lowercase();
        let tokens: Vec<&str> = q.split_whitespace().collect();
        self.filtered = self.all_profiles.iter().enumerate()
            .filter(|(_, (prov, prof))| {
                if tokens.is_empty() { return true; }
                let haystack = format!("{} {} {} {}", prov.name, prov.id, prof.name, prof.id).to_lowercase();
                tokens.iter().all(|t| haystack.contains(t))
            })
            .map(|(i, _)| i)
            .collect();
        if self.state.selected().unwrap_or(0) >= self.filtered.len() {
            self.state.select(if self.filtered.is_empty() { None } else { Some(0) });
        }
    }

    fn render_search_box(&self, f: &mut Frame, area: Rect) {
        let cursor = if self.is_searching { "\u{258c}" } else { "" };
        let text = if self.search_query.is_empty() && !self.is_searching {
            "\u{2315} Search (/ to focus)".to_string()
        } else if !self.search_query.is_empty() && !self.is_searching {
            format!("\u{2315} {} (/) — Esc to clear", self.search_query)
        } else {
            format!("\u{2315} {}{}", self.search_query, cursor)
        };
        let color = if self.is_searching { Theme::CYAN } else { Theme::COMMENT };
        let p = Paragraph::new(Line::from(Span::styled(text, Style::default().fg(color))))
            .block(Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                .border_style(Style::default().fg(Theme::DIM)));
        f.render_widget(p, area);
    }
}

impl TabContent for ProvidersTab {
    fn render(&mut self, f: &mut Frame, area: Rect) {
        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(3)])
            .split(main[0]);

        // Search box
        self.render_search_box(f, left[0]);

        // Profile list
        let items: Vec<ListItem> = self.filtered.iter().enumerate()
            .map(|(fi, &ai)| {
                let (prov, prof) = &self.all_profiles[ai];
                let is_selected = self.state.selected() == Some(fi);
                let arrow = if is_selected { "❯ " } else { "  " };
                let title_color = if is_selected { Theme::CYAN } else { Theme::FG };
                let is_active = self.active_provider == prov.id && self.active_profile == prof.id;

                ListItem::new(vec![
                    Line::from(Span::styled(
                        format!("{}{}", arrow, prof.name),
                        Style::default().fg(title_color),
                    )),
                    Line::from(vec![
                        Span::styled("    ", Style::default()),
                        Span::styled(&prov.name, Style::default().fg(Theme::COMMENT)),
                        Span::styled(" · ", Style::default().fg(Theme::DIM)),
                        Span::styled(&prov.id, Style::default().fg(Theme::COMMENT)),
                        Span::styled(" · ", Style::default().fg(Theme::DIM)),
                        Span::styled(&prof.id, Style::default().fg(Theme::COMMENT)),
                        if is_active {
                            Span::styled(" ★ active", Style::default().fg(Theme::YELLOW))
                        } else {
                            Span::styled("", Style::default())
                        },
                    ]),
                    Line::from(""),
                ])
            })
            .collect();

        let list = List::new(items)
            .block(Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                .title(format!("Profiles ({})", self.filtered.len()))
                .border_style(Style::default().fg(Theme::DIM)))
            .highlight_style(Style::default());
        f.render_stateful_widget(list, left[1], &mut self.state);

        // Right: detail preview
        if let Some(idx) = self.state.selected() {
            if let Some(&ai) = self.filtered.get(idx) {
                let (prov, prof) = &self.all_profiles[ai];
                let is_active = self.active_provider == prov.id && self.active_profile == prof.id;
                DetailPanel::render_profile_detail(
                    f, main[1], &prov.name, prof,
                    &prov.api_url, &prov.api_key,
                    is_active, prov.source.can_delete(),
                );
            }
        } else {
            DetailPanel::render_empty(f, main[1], "No profiles available");
        }
    }

    fn handle_key(&mut self, code: KeyCode) -> bool {
        if self.is_searching {
            match code {
                KeyCode::Esc => { self.is_searching = false; self.search_query.clear(); self.refresh_filter(); }
                KeyCode::Enter => { self.is_searching = false; if !self.filtered.is_empty() { self.state.select(Some(0)); } }
                KeyCode::Backspace | KeyCode::Delete => { self.search_query.pop(); self.refresh_filter(); }
                KeyCode::Char(c) => { self.search_query.push(c); self.refresh_filter(); }
                _ => {}
            }
            return true;
        }

        match code {
            KeyCode::Tab | KeyCode::BackTab => return false,
            KeyCode::Char('j') | KeyCode::Down => {
                let len = self.filtered.len();
                if len == 0 { return true; }
                let i = self.state.selected().unwrap_or(0);
                self.state.select(Some(if i + 1 < len { i + 1 } else { 0 }));
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let len = self.filtered.len();
                if len == 0 { return true; }
                let i = self.state.selected().unwrap_or(0);
                self.state.select(Some(if i > 0 { i - 1 } else { len - 1 }));
            }
            KeyCode::Char('/') => { self.is_searching = true; }
            _ => { return false; }
        }
        true
    }
}
