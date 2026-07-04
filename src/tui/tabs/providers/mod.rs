pub mod form;

use form::EditForm;
use super::super::theme;
use super::super::widgets::detail_panel::DetailPanel;
use super::super::widgets::shared::{render_confirm_popup as shared_confirm, render_message_popup as shared_msg};
use super::TabContent;
use crate::core::config::ConfigManager;
use crate::core::models::Provider;
use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph},
    Frame,
};

use std::sync::Arc;

#[derive(Clone, Copy, PartialEq)]
pub enum ProviderAction {
    Switch,
    Delete,
}

pub struct ProvidersTab {
    mgr: Arc<ConfigManager>,
    all_profiles: Vec<(Provider, crate::core::models::Profile)>,
    filtered: Vec<usize>,
    pub state: ListState,
    pub search_query: String,
    pub is_searching: bool,
    pub active_provider: String,
    pub active_profile: String,
    pub confirm_action: Option<ProviderAction>,
    confirm_button: usize,
    pub message: Option<String>,
    edit_form: Option<EditForm>,
}

impl ProvidersTab {
    pub fn new(mgr: Arc<ConfigManager>) -> Self {
        // Sync active state from settings.json's last_switch.source
        crate::core::sync::sync_active_from_settings(&mgr);

        let providers = mgr.list_providers().unwrap_or_default();
        let mut all_profiles = Vec::new();
        for p in &providers {
            for pr in &p.profiles {
                all_profiles.push((p.clone(), pr.clone()));
            }
        }
        // Sort by profile name alphabetically (case-insensitive)
        all_profiles.sort_by(|(_, a), (_, b)| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        let active_provider = mgr.db().get_setting("active_provider").unwrap_or_default();
        let active_profile = mgr.db().get_setting("active_profile").unwrap_or_default();
        let filtered: Vec<usize> = (0..all_profiles.len()).collect();
        let mut state = ListState::default();
        // Select the active profile by default, fallback to first
        if !filtered.is_empty() {
            let active_idx = all_profiles.iter().position(|(p, pr)| p.id == active_provider && pr.id == active_profile);
            state.select(active_idx.or(Some(0)));
        }
        ProvidersTab {
            mgr,
            all_profiles,
            filtered,
            state,
            search_query: String::new(),
            is_searching: false,
            active_provider,
            active_profile,
            confirm_action: None,
            confirm_button: 0,
            message: None,
            edit_form: None,
        }
    }

    pub fn refresh_filter(&mut self) {
        let q = self.search_query.trim().to_lowercase();
        let tokens: Vec<&str> = q.split_whitespace().collect();
        self.filtered = self
            .all_profiles
            .iter()
            .enumerate()
            .filter(|(_, (prov, prof))| {
                if tokens.is_empty() {
                    return true;
                }
                let hay = format!("{} {} {} {}", prov.name, prov.id, prof.name, prof.id).to_lowercase();
                tokens.iter().all(|t| hay.contains(t))
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
        let color = if self.is_searching { theme::current().cyan } else { theme::current().comment };
        let p = Paragraph::new(Line::from(Span::styled(text, Style::default().fg(color)))).block(
            Block::bordered()
                .border_set(ratatui::symbols::border::ROUNDED)
                .border_style(Style::default().fg(theme::current().dim)),
        );
        f.render_widget(p, area);
    }

    fn selected_profile(&self) -> Option<&(Provider, crate::core::models::Profile)> {
        let idx = self.state.selected()?;
        let &ai = self.filtered.get(idx)?;
        self.all_profiles.get(ai)
    }

    fn do_edit(&mut self) {
        let Some((prov, prof)) = self.selected_profile() else { return };
        let fields = [prof.name.clone(), prof.opus.clone(), prof.sonnet.clone(), prof.haiku.clone(), prof.subagent.clone()];
        let cursors = [fields[0].len(), fields[1].len(), fields[2].len(), fields[3].len(), fields[4].len()];
        self.edit_form = Some(EditForm {
            fields,
            cursors,
            focused: 0,
            prov_id: prov.id.clone(),
            prof_id: prof.id.clone(),
        });
    }

    fn commit_edit(&mut self) {
        let Some(form) = self.edit_form.take() else { return };
        let pr = crate::core::models::Profile {
            id: form.prof_id.clone(),
            name: form.fields[0].clone(),
            opus: form.fields[1].clone(),
            sonnet: form.fields[2].clone(),
            haiku: form.fields[3].clone(),
            subagent: form.fields[4].clone(),
            default: false,
            source: crate::core::models::Source::User,
        };
        if let Err(e) = self.mgr.db().insert_claude_profile(&form.prov_id, &pr) {
            tracing::error!("Failed to insert user profile: {}", e);
        }
        if let Some((_, p)) = self.all_profiles.iter_mut().find(|(prov, prof)| prov.id == form.prov_id && prof.id == form.prof_id) {
            p.name = pr.name;
            p.opus = pr.opus;
            p.sonnet = pr.sonnet;
            p.haiku = pr.haiku;
            p.subagent = pr.subagent;
        }
        self.refresh_filter();
    }

    fn render_edit_form(&self, f: &mut Frame, area: Rect) {
        if let Some(ref form) = self.edit_form {
            form::render_edit_form(form, f, area);
        }
    }

    fn do_switch(&mut self) {
        let (prov_id, prof_id) = {
            let Some((prov, prof)) = self.selected_profile() else { return };
            (prov.id.clone(), prof.id.clone())
        };
        let mode = if self.mgr.db().get_setting("proxy_mode").map(|v| v == "true").unwrap_or(false) {
            crate::core::models::SwitchMode::Proxy
        } else {
            crate::core::models::SwitchMode::Local
        };
        if let Err(e) = crate::core::switcher::switch_profile(&self.mgr, &prov_id, &prof_id, mode, None) {
            self.message = Some(format!("Error: {}", e));
            return;
        }
        self.active_provider = prov_id;
        self.active_profile = prof_id;
        if let Err(e) = self.mgr.db().set_setting("active_provider", &self.active_provider) {
            tracing::error!("Failed to save active_provider: {}", e);
        }
        if let Err(e) = self.mgr.db().set_setting("active_profile", &self.active_profile) {
            tracing::error!("Failed to save active_profile: {}", e);
        }
    }

    fn do_delete(&mut self) {
        let prof_id = {
            let Some((prov, prof)) = self.selected_profile() else { return };
            if !prov.source.can_delete() {
                return;
            }
            prof.id.clone()
        };
        if let Err(e) = self.mgr.db().delete_claude_profile(&prof_id) {
            tracing::error!("Failed to delete user profile: {}", e);
        }
        self.all_profiles.retain(|(_, p)| p.id != prof_id);
        self.refresh_filter();
    }

    fn render_confirm_popup(&self, f: &mut Frame, area: Rect) {
        let (title, msg, c) = match self.confirm_action {
            Some(ProviderAction::Switch) => (" Switch Model ", " Switch to this profile? ", theme::current().cyan),
            Some(ProviderAction::Delete) => (" Delete Profile ", " Delete this profile? ", theme::current().red),
            _ => return,
        };
        shared_confirm(f, area, title, msg, "Confirm", "Cancel", c, self.confirm_button);
    }

    fn render_message_popup(&self, f: &mut Frame, area: Rect) {
        shared_msg(f, area, self.message.as_deref().unwrap_or(""));
    }
}

impl TabContent for ProvidersTab {
    fn render(&mut self, f: &mut Frame, area: Rect, _app_type: &str) {
        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(3)])
            .split(main[0]);
        // Right panel: detail preview
        let right = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(3)]).split(main[1]);
        self.render_search_box(f, left[0]);

        let items: Vec<ListItem> = self
            .filtered
            .iter()
            .enumerate()
            .map(|(fi, &ai)| {
                let (prov, prof) = &self.all_profiles[ai];
                let is_sel = self.state.selected() == Some(fi);
                let arrow = if is_sel { "\u{276f} " } else { "  " };
                let tc = if is_sel { theme::current().cyan } else { theme::current().fg };
                let active = self.active_provider == prov.id && self.active_profile == prof.id;
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(format!("{}{}", arrow, prof.name), Style::default().fg(tc)),
                        if active {
                            Span::styled(" (in use)", Style::default().fg(theme::current().green))
                        } else {
                            Span::styled("", Style::default())
                        },
                    ]),
                    Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(&prov.name, Style::default().fg(theme::current().comment)),
                        Span::styled(" \u{b7} ", Style::default().fg(theme::current().dim)),
                        Span::styled(&prov.id, Style::default().fg(theme::current().comment)),
                        Span::styled(" \u{b7} ", Style::default().fg(theme::current().dim)),
                        Span::styled(&prof.id, Style::default().fg(theme::current().comment)),
                        if active {
                            Span::styled(" \u{2605} active", Style::default().fg(theme::current().yellow))
                        } else {
                            Span::styled("", Style::default())
                        },
                    ]),
                    Line::from(""),
                ])
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::bordered()
                    .border_set(ratatui::symbols::border::ROUNDED)
                    .title(format!("Profiles ({})", self.filtered.len()))
                    .border_style(Style::default().fg(theme::current().dim)),
            )
            .highlight_style(Style::default());
        f.render_stateful_widget(list, left[1], &mut self.state);

        if let Some(idx) = self.state.selected() {
            if let Some(&ai) = self.filtered.get(idx) {
                let (prov, prof) = &self.all_profiles[ai];
                let active = self.active_provider == prov.id && self.active_profile == prof.id;
                DetailPanel::render_profile_detail(f, right[0], &prov.name, prof, &prov.api_url, &prov.api_key, active, prov.source.can_delete());
            }
        } else {
            DetailPanel::render_empty(f, right[0], "No profiles available");
        }

        if self.confirm_action.is_some() {
            self.render_confirm_popup(f, area);
        }
        if self.message.is_some() {
            self.render_message_popup(f, area);
        }
        if self.edit_form.is_some() {
            self.render_edit_form(f, area);
        }
    }

    fn handle_key(&mut self, code: KeyCode) -> bool {
        // Edit form mode
        if let Some(ref mut f) = self.edit_form {
            match code {
                KeyCode::Esc => { self.edit_form = None; }
                KeyCode::Enter => { self.commit_edit(); }
                _ => { f.handle_key(code); }
            }
            return true;
        }
        if self.message.is_some() {
            if matches!(code, KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q')) {
                self.message = None;
            }
            return true;
        }
        if self.confirm_action.is_some() {
            match code {
                KeyCode::Tab | KeyCode::Right | KeyCode::Char('j') | KeyCode::Char('l') => self.confirm_button = (self.confirm_button + 1) % 2,
                KeyCode::BackTab | KeyCode::Left | KeyCode::Char('k') | KeyCode::Char('h') => self.confirm_button = if self.confirm_button == 0 { 1 } else { 0 },
                KeyCode::Enter => {
                    if self.confirm_button == 0 {
                        match self.confirm_action {
                            Some(ProviderAction::Switch) => self.do_switch(),
                            Some(ProviderAction::Delete) => self.do_delete(),
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
        if self.is_searching {
            match code {
                KeyCode::Esc => {
                    self.is_searching = false;
                    self.search_query.clear();
                    self.refresh_filter();
                }
                KeyCode::Enter => {
                    self.is_searching = false;
                    if !self.filtered.is_empty() {
                        self.state.select(Some(0));
                    }
                }
                KeyCode::Backspace | KeyCode::Delete => {
                    self.search_query.pop();
                    self.refresh_filter();
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                    self.refresh_filter();
                }
                _ => {}
            }
            return true;
        }
        match code {
            KeyCode::Tab | KeyCode::BackTab => return false,
            KeyCode::Char('j') | KeyCode::Down => {
                let l = self.filtered.len();
                if l > 0 {
                    let i = self.state.selected().unwrap_or(0);
                    self.state.select(Some(if i + 1 < l { i + 1 } else { 0 }));
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let l = self.filtered.len();
                if l > 0 {
                    let i = self.state.selected().unwrap_or(0);
                    self.state.select(Some(if i > 0 { i - 1 } else { l - 1 }));
                }
            }
            KeyCode::Enter => {
                self.confirm_action = Some(ProviderAction::Switch);
                self.confirm_button = 0;
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                if let Some(&ai) = self.filtered.get(self.state.selected().unwrap_or(0)) {
                    if !self.all_profiles[ai].0.source.can_delete() {
                        self.message = Some("Cannot delete system default profile".into());
                        return true;
                    }
                }
                self.confirm_action = Some(ProviderAction::Delete);
                self.confirm_button = 0;
            }
            KeyCode::Char('e') | KeyCode::Char('E') => {
                if let Some(&ai) = self.filtered.get(self.state.selected().unwrap_or(0)) {
                    if !self.all_profiles[ai].0.source.can_delete() {
                        self.message = Some("Cannot edit system default profile".into());
                        return true;
                    }
                }
                self.do_edit();
            }
            KeyCode::Char('/') => {
                self.is_searching = true;
            }
            _ => return false,
        }
        true
    }

    fn shortcut_groups(&self) -> Vec<Vec<(String, Color)>> {
        vec![
            vec![(" J/K ".into(), theme::current().comment), ("Nav".into(), theme::current().comment)],
            vec![(" / ".into(), theme::current().comment), ("Search".into(), theme::current().comment)],
            vec![(" ⏎  ".into(), theme::current().comment), ("Switch".into(), theme::current().comment)],
            vec![(" D ".into(), theme::current().comment), ("Delete".into(), theme::current().comment)],
            vec![(" E ".into(), theme::current().comment), ("Edit".into(), theme::current().comment)],
            vec![(" Q ".into(), theme::current().comment), ("Quit".into(), theme::current().comment)],
        ]
    }

    fn shortcut_lines(&self, available_width: u16) -> usize {
        let group_widths = [8usize, 9, 12, 8, 7, 7];
        let sep = 2usize;
        let w = available_width.saturating_sub(2).max(10) as usize;
        let mut lines = 1usize;
        let mut cur = 0usize;
        for gw in &group_widths {
            if cur + gw > w && cur > 0 {
                lines += 1;
                cur = 0;
            }
            if cur > 0 {
                cur += sep;
            }
            cur += gw;
        }
        lines
    }
}