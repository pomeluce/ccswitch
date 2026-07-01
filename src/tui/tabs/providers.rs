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
pub enum ProviderAction { Switch, Delete }

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
    /// Edit form state
    edit_form: Option<EditForm>,
}

struct EditForm {
    fields: [String; 5],   // name, opus, sonnet, haiku, subagent
    cursors: [usize; 5],   // cursor position in each field
    focused: usize,        // 0..4
    prov_id: String,
    prof_id: String,
}

const EDIT_LABELS: [&str; 5] = ["Profile Name", "Opus model", "Sonnet model", "Haiku model", "SubAgent model"];

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
            message: None, edit_form: None,
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
        let Some((prov, prof)) = self.selected_profile() else { return };
        let fields = [prof.name.clone(), prof.opus.clone(), prof.sonnet.clone(), prof.haiku.clone(), prof.subagent.clone()];
        let cursors = [
            fields[0].len(), fields[1].len(), fields[2].len(), fields[3].len(), fields[4].len(),
        ];
        self.edit_form = Some(EditForm {
            fields, cursors, focused: 0,
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
            default: false, source: crate::core::models::Source::User,
        };
        self.mgr.db().insert_user_profile(&form.prov_id, &pr).ok();
        if let Some((_, p)) = self.all_profiles.iter_mut().find(|(prov, prof)| prov.id == form.prov_id && prof.id == form.prof_id) {
            p.name = pr.name; p.opus = pr.opus; p.sonnet = pr.sonnet; p.haiku = pr.haiku; p.subagent = pr.subagent;
        }
        self.refresh_filter();
    }

    fn render_edit_form(&mut self, f: &mut Frame, area: Rect) {
        let Some(ref mut form) = self.edit_form else { return };
        let popup = centered_rect(60, 20, area);
        let inner_w = popup.width.saturating_sub(2) as usize;
        // Equal left/right padding, label(15) + ": " (2) → value_w = inner - pad*2 - 17
        let pad_w = (inner_w.saturating_sub(40)) / 2;
        let pad = " ".repeat(pad_w);
        let value_w = inner_w.saturating_sub(pad_w * 2 + 17);

        let mut lines: Vec<Line> = Vec::new();
        // Top padding (3 lines)
        lines.push(Line::from("")); lines.push(Line::from("")); lines.push(Line::from(""));
        for (i, label) in EDIT_LABELS.iter().enumerate() {
            let val = &form.fields[i];
            let pos = form.cursors[i].min(val.len());
            let vis = slice_value(val, pos, value_w);
            let cur = (pos - vis.skip).min(vis.text.len());
            let (left, right) = vis.text.split_at(cur);
            let cursor = if i == form.focused { "▌" } else { "" };
            let style = if i == form.focused { Style::default().fg(Theme::CYAN) } else { Style::default().fg(Theme::FG) };
            lines.push(Line::from(vec![
                Span::styled(format!("{}{:<15}: ", pad, label), Style::default().fg(Theme::FG)),
                Span::styled(left.to_string(), style),
                Span::styled(cursor.to_string(), style),
                Span::styled(right.to_string(), style),
                Span::styled(pad.clone(), Style::default()), // right padding
            ]));
            lines.push(Line::from(""));
        }
        // Padding before hints
        lines.push(Line::from("")); lines.push(Line::from(""));
        // Hints at bottom (centered)
        lines.push(Line::from(vec![
            Span::styled(" Enter ", Style::default().fg(Theme::GREEN)),
            Span::styled(" Save  ", Style::default().fg(Theme::COMMENT)),
            Span::styled(" Esc ", Style::default().fg(Theme::DIM)),
            Span::styled(" Cancel  ", Style::default().fg(Theme::COMMENT)),
            Span::styled(" Tab ", Style::default().fg(Theme::CYAN)),
            Span::styled(" Next field", Style::default().fg(Theme::COMMENT)),
        ]).centered());

        let p = Paragraph::new(lines)
            .block(Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                .title(Line::from(" Edit Profile ").centered())
                .border_style(Style::default().fg(Theme::CYAN)));
        f.render_widget(Clear, popup);
        f.render_widget(p, popup);
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
        if self.edit_form.is_some() { self.render_edit_form(f, area); }
    }

    fn handle_key(&mut self, code: KeyCode) -> bool {
        // Edit form mode
        if let Some(ref mut f) = self.edit_form {
            let field = &mut f.fields[f.focused];
            let cur = &mut f.cursors[f.focused];
            match code {
                KeyCode::Esc => { self.edit_form = None; }
                KeyCode::Enter => { self.commit_edit(); }
                KeyCode::Tab => { f.focused = (f.focused + 1) % 5; }
                KeyCode::BackTab => { f.focused = if f.focused == 0 { 4 } else { f.focused - 1 }; }
                KeyCode::Left => { *cur = cur.saturating_sub(1); }
                KeyCode::Right => { *cur = (*cur + 1).min(field.len()); }
                KeyCode::Home => { *cur = 0; }
                KeyCode::End => { *cur = field.len(); }
                KeyCode::Backspace => {
                    if *cur > 0 { *cur -= 1; field.remove(*cur); }
                }
                KeyCode::Delete => {
                    if *cur < field.len() { field.remove(*cur); }
                }
                KeyCode::Char(c) => {
                    field.insert(*cur, c); *cur += 1;
                }
                _ => {}
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
                KeyCode::Enter => { if self.confirm_button == 0 { match self.confirm_action { Some(ProviderAction::Switch) => self.do_switch(), Some(ProviderAction::Delete) => self.do_delete(), _ => {} } } self.confirm_action = None; self.confirm_button = 0; }
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
                self.do_edit();
            }
            KeyCode::Char('/') => { self.is_searching = true; }
            _ => return false,
        }
        true
    }
}

struct VisSlice { text: String, skip: usize }

/// Show a windowed slice of text centered around cursor position
fn slice_value(text: &str, cursor: usize, max_w: usize) -> VisSlice {
    if text.len() <= max_w || max_w < 4 {
        return VisSlice { text: text.to_string(), skip: 0 };
    }
    // Try to center cursor
    let half = max_w / 2;
    let mut start = if cursor > half { cursor - half } else { 0 };
    let end = (start + max_w).min(text.len());
    // Adjust if we hit the end
    if end == text.len() && end > max_w {
        start = end - max_w;
    }
    VisSlice { text: text[start..end].to_string(), skip: start }
}

fn centered_rect(w: u16, h: u16, r: Rect) -> Rect {
    Rect { x: r.x + (r.width.saturating_sub(w)) / 2, y: r.y + (r.height.saturating_sub(h)) / 2, width: w.min(r.width), height: h.min(r.height) }
}

