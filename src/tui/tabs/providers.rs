use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};
use crossterm::event::KeyCode;
use crate::core::config::ConfigManager;
use crate::core::models::Provider;
use super::super::widgets::detail_panel::DetailPanel;
use super::TabContent;
use super::super::theme::Theme;

use std::sync::Arc;

#[derive(Clone, Copy, PartialEq)]
pub enum ProviderAction { Switch, Delete, Edit }

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
}

impl ProvidersTab {
    pub fn new(mgr: Arc<ConfigManager>) -> Self {
        let providers = mgr.list_providers().unwrap_or_default();
        let mut all_profiles = Vec::new();
        for p in &providers {
            for pr in &p.profiles {
                all_profiles.push((p.clone(), pr.clone()));
            }
        }
        let active_provider = mgr.db().get_setting("active_provider").unwrap_or_default();
        let active_profile = mgr.db().get_setting("active_profile").unwrap_or_default();
        let filtered: Vec<usize> = (0..all_profiles.len()).collect();
        let mut state = ListState::default();
        if !filtered.is_empty() { state.select(Some(0)); }
        ProvidersTab {
            mgr,
            all_profiles, filtered, state,
            search_query: String::new(), is_searching: false,
            active_provider, active_profile,
            confirm_action: None, confirm_button: 0,
            message: None,
        }
    }

    pub fn refresh_filter(&mut self) {
        let q = self.search_query.trim().to_lowercase();
        let tokens: Vec<&str> = q.split_whitespace().collect();
        self.filtered = self.all_profiles.iter().enumerate()
            .filter(|(_, (prov, prof))| {
                if tokens.is_empty() { return true; }
                let hay = format!("{} {} {} {}", prov.name, prov.id, prof.name, prof.id).to_lowercase();
                tokens.iter().all(|t| hay.contains(t))
            })
            .map(|(i, _)| i).collect();
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
        } else { format!("\u{2315} {}{}", self.search_query, cursor) };
        let color = if self.is_searching { Theme::CYAN } else { Theme::COMMENT };
        let p = Paragraph::new(Line::from(Span::styled(text, Style::default().fg(color))))
            .block(Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                .border_style(Style::default().fg(Theme::DIM)));
        f.render_widget(p, area);
    }

    fn selected_profile(&self) -> Option<&(Provider, crate::core::models::Profile)> {
        let idx = self.state.selected()?;
        let &ai = self.filtered.get(idx)?;
        self.all_profiles.get(ai)
    }

    fn do_edit(&mut self) {
        let (prov_id, prof_id, prof_name, opus, sonnet, haiku, subagent) = {
            let Some((prov, prof)) = self.selected_profile() else { return };
            (prov.id.clone(), prof.id.clone(), prof.name.clone(),
             prof.opus.clone(), prof.sonnet.clone(), prof.haiku.clone(), prof.subagent.clone())
        };
        // Suspend TUI for interactive editing
        ratatui::restore();
        use dialoguer::Input;
        let name: String = Input::new().with_prompt("Name").with_initial_text(&prof_name).interact_text().unwrap_or(prof_name);
        let opus: String = Input::new().with_prompt("Opus model").with_initial_text(&opus).interact_text().unwrap_or(opus);
        let sonnet: String = Input::new().with_prompt("Sonnet model").with_initial_text(&sonnet).interact_text().unwrap_or(sonnet);
        let haiku: String = Input::new().with_prompt("Haiku model").with_initial_text(&haiku).interact_text().unwrap_or(haiku);
        let subagent: String = Input::new().with_prompt("SubAgent model").with_initial_text(&subagent).interact_text().unwrap_or(subagent);
        ratatui::init();
        let pr = crate::core::models::Profile {
            id: prof_id, name, opus: opus.clone(), sonnet: sonnet.clone(), haiku: haiku.clone(),
            subagent: subagent.clone(), default: false, source: crate::core::models::Source::User,
        };
        self.mgr.db().insert_user_profile(&prov_id, &pr).ok();
        // Update in-memory
        if let Some((_, p)) = self.all_profiles.iter_mut().find(|(prov, prof)| prov.id == prov_id && prof.id == pr.id) {
            p.opus = opus;
            p.sonnet = sonnet;
            p.haiku = haiku;
            p.subagent = subagent;
        }
    }

    fn do_switch(&mut self) {
        let (prov_id, prof_id) = {
            let Some((prov, prof)) = self.selected_profile() else { return };
            (prov.id.clone(), prof.id.clone())
        };
        let mode = if self.mgr.db().get_setting("proxy_mode").map(|v| v == "true").unwrap_or(false) {
            crate::core::models::SwitchMode::Proxy
        } else { crate::core::models::SwitchMode::Local };
        if let Err(e) = crate::core::switcher::switch_profile(&self.mgr, &prov_id, &prof_id, mode, None) {
            self.message = Some(format!("Error: {}", e));
            return;
        }
        self.active_provider = prov_id;
        self.active_profile = prof_id;
        self.mgr.db().set_setting("active_provider", &self.active_provider).ok();
        self.mgr.db().set_setting("active_profile", &self.active_profile).ok();
    }

    fn do_delete(&mut self) {
        let prof_id = {
            let Some((prov, prof)) = self.selected_profile() else { return };
            if !prov.source.can_delete() { return; }
            prof.id.clone()
        };
        self.mgr.db().delete_user_profile(&prof_id).ok();
        self.all_profiles.retain(|(_, p)| p.id != prof_id);
        self.refresh_filter();
    }

    fn render_confirm_popup(&self, f: &mut Frame, area: Rect) {
        let (title, msg, c) = match self.confirm_action {
            Some(ProviderAction::Switch) => (" Switch Model ", " Switch to this profile? ", Theme::CYAN),
            Some(ProviderAction::Delete) => (" Delete Profile ", " Delete this profile? ", Theme::RED),
            Some(ProviderAction::Edit) => (" Edit Profile ", " Edit this profile? ", Theme::CYAN),
            _ => return,
        };
        let popup = centered_rect(44, 6, area);
        let cs = if self.confirm_button == 0 { Style::default().fg(Color::Black).bg(c) } else { Style::default().fg(Theme::DIM) };
        let xs = if self.confirm_button == 1 { Style::default().fg(Color::Black).bg(Theme::CYAN) } else { Style::default().fg(Theme::DIM) };
        let p = Paragraph::new(vec![
            Line::from(msg).centered(), Line::from(""),
            Line::from(vec![Span::styled("  Confirm  ", cs), Span::raw("     "), Span::styled("  Cancel  ", xs)]).centered(),
        ]).block(Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
            .title(Line::from(title).centered()).border_style(Style::default().fg(c)));
        f.render_widget(Clear, popup);
        f.render_widget(p, popup);
    }

    fn render_message_popup(&self, f: &mut Frame, area: Rect) {
        let msg = self.message.as_deref().unwrap_or("");
        let popup = centered_rect(44, 5, area);
        let p = Paragraph::new(vec![
            Line::from(""), Line::from(msg).centered(), Line::from(""),
            Line::from(Span::styled("  OK  ", Style::default().fg(Color::Black).bg(Theme::CYAN))).centered(),
        ]).block(Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
            .title(Line::from(" Notice ").centered()).border_style(Style::default().fg(Theme::YELLOW)));
        f.render_widget(Clear, popup);
        f.render_widget(p, popup);
    }
}

impl TabContent for ProvidersTab {
    fn render(&mut self, f: &mut Frame, area: Rect) {
        let main = Layout::default().direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);
        let left = Layout::default().direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(3)]).split(main[0]);
        self.render_search_box(f, left[0]);

        let items: Vec<ListItem> = self.filtered.iter().enumerate().map(|(fi, &ai)| {
            let (prov, prof) = &self.all_profiles[ai];
            let is_sel = self.state.selected() == Some(fi);
            let arrow = if is_sel { "\u{276f} " } else { "  " };
            let tc = if is_sel { Theme::CYAN } else { Theme::FG };
            let active = self.active_provider == prov.id && self.active_profile == prof.id;
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(format!("{}{}", arrow, prof.name), Style::default().fg(tc)),
                    if active { Span::styled(" (in use)", Style::default().fg(Theme::GREEN)) } else { Span::styled("", Style::default()) },
                ]),
                Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(&prov.name, Style::default().fg(Theme::COMMENT)),
                    Span::styled(" \u{b7} ", Style::default().fg(Theme::DIM)),
                    Span::styled(&prov.id, Style::default().fg(Theme::COMMENT)),
                    Span::styled(" \u{b7} ", Style::default().fg(Theme::DIM)),
                    Span::styled(&prof.id, Style::default().fg(Theme::COMMENT)),
                    if active { Span::styled(" \u{2605} active", Style::default().fg(Theme::YELLOW)) } else { Span::styled("", Style::default()) },
                ]),
                Line::from(""),
            ])
        }).collect();

        let list = List::new(items).block(Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
            .title(format!("Profiles ({})", self.filtered.len())).border_style(Style::default().fg(Theme::DIM)))
            .highlight_style(Style::default());
        f.render_stateful_widget(list, left[1], &mut self.state);

        if let Some(idx) = self.state.selected() {
            if let Some(&ai) = self.filtered.get(idx) {
                let (prov, prof) = &self.all_profiles[ai];
                let active = self.active_provider == prov.id && self.active_profile == prof.id;
                DetailPanel::render_profile_detail(f, main[1], &prov.name, prof, &prov.api_url, &prov.api_key, active, prov.source.can_delete());
            }
        } else { DetailPanel::render_empty(f, main[1], "No profiles available"); }

        if self.confirm_action.is_some() { self.render_confirm_popup(f, area); }
        if self.message.is_some() { self.render_message_popup(f, area); }
    }

    fn handle_key(&mut self, code: KeyCode) -> bool {
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
                KeyCode::Enter => { if self.confirm_button == 0 { match self.confirm_action { Some(ProviderAction::Switch) => self.do_switch(), Some(ProviderAction::Delete) => self.do_delete(), Some(ProviderAction::Edit) => self.do_edit(), _ => {} } } self.confirm_action = None; self.confirm_button = 0; }
                KeyCode::Esc | KeyCode::Char('q') => { self.confirm_action = None; self.confirm_button = 0; }
                _ => {}
            }
            return true;
        }
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
            KeyCode::Char('j') | KeyCode::Down => { let l = self.filtered.len(); if l > 0 { let i = self.state.selected().unwrap_or(0); self.state.select(Some(if i + 1 < l { i + 1 } else { 0 })); } }
            KeyCode::Char('k') | KeyCode::Up => { let l = self.filtered.len(); if l > 0 { let i = self.state.selected().unwrap_or(0); self.state.select(Some(if i > 0 { i - 1 } else { l - 1 })); } }
            KeyCode::Enter => { self.confirm_action = Some(ProviderAction::Switch); self.confirm_button = 0; }
            KeyCode::Char('d') => {
                if let Some(&ai) = self.filtered.get(self.state.selected().unwrap_or(0)) {
                    if !self.all_profiles[ai].0.source.can_delete() {
                        self.message = Some("Cannot delete system default profile".into());
                        return true;
                    }
                }
                self.confirm_action = Some(ProviderAction::Delete); self.confirm_button = 0;
            }
            KeyCode::Char('e') => {
                if let Some(&ai) = self.filtered.get(self.state.selected().unwrap_or(0)) {
                    if !self.all_profiles[ai].0.source.can_delete() {
                        self.message = Some("Cannot edit system default profile".into());
                        return true;
                    }
                }
                self.confirm_action = Some(ProviderAction::Edit); self.confirm_button = 0;
            }
            KeyCode::Char('/') => { self.is_searching = true; }
            _ => return false,
        }
        true
    }
}

fn centered_rect(w: u16, h: u16, r: Rect) -> Rect {
    Rect { x: r.x + (r.width.saturating_sub(w)) / 2, y: r.y + (r.height.saturating_sub(h)) / 2, width: w.min(r.width), height: h.min(r.height) }
}
