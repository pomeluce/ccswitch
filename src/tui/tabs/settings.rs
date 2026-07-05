use super::super::lang;
use super::super::theme::{self, THEMES};
use super::TabContent;
use crate::core::config::ConfigManager;
use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph},
    Frame,
};
use std::sync::Arc;

const MODES: &[&str] = &["local", "proxy"];

pub struct SettingsTab {
    mgr: Arc<ConfigManager>,
    pub state: ListState,
    selected: usize,
    theme_idx: usize,
    mode_idx: usize,
    lang_idx: usize,
}

impl SettingsTab {
    pub fn new(mgr: Arc<ConfigManager>) -> Self {
        // Restore theme from DB
        let saved_theme = mgr.get_setting("theme").unwrap_or_default();
        if !saved_theme.is_empty() {
            theme::set_theme(&saved_theme);
        }
        let current_theme = theme::current_theme();
        let theme_idx = THEMES.iter().position(|&t| t == current_theme).unwrap_or(0);

        // Restore language from DB
        let saved_lang = mgr.get_setting("language").unwrap_or_default();
        let lang_idx = if saved_lang.is_empty() {
            0
        } else {
            lang::set_lang(&saved_lang);
            lang::LANGS.iter().position(|(n, _)| *n == saved_lang).unwrap_or(0)
        };

        let mode_idx = if mgr.get_setting("proxy_mode").map(|v| v == "true").unwrap_or(false) {
            1
        } else {
            0
        };
        let mut state = ListState::default();
        state.select(Some(0));
        SettingsTab { mgr, state, selected: 0, theme_idx, mode_idx, lang_idx }
    }

    fn items(&self) -> Vec<(&str, String)> {
        let l = lang::current();
        vec![
            (l.setting_theme, THEMES[self.theme_idx].to_string()),
            (l.setting_mode, MODES[self.mode_idx].to_string()),
            (l.setting_language, lang::current_lang().to_string()),
        ]
    }

    fn cycle_theme(&mut self, forward: bool) {
        self.theme_idx = if forward {
            (self.theme_idx + 1) % THEMES.len()
        } else if self.theme_idx == 0 { THEMES.len() - 1 } else { self.theme_idx - 1 };
        theme::set_theme(THEMES[self.theme_idx]);
        if let Err(e) = self.mgr.set_setting("theme", THEMES[self.theme_idx]) { tracing::warn!("Failed to save theme: {}", e); }
    }

    fn cycle_mode(&mut self, forward: bool) {
        self.mode_idx = if forward {
            (self.mode_idx + 1) % MODES.len()
        } else if self.mode_idx == 0 { MODES.len() - 1 } else { self.mode_idx - 1 };
        if let Err(e) = self.mgr.set_setting("proxy_mode", &(self.mode_idx == 1).to_string()) { tracing::warn!("Failed to save mode: {}", e); }
    }

    fn cycle_lang(&mut self, forward: bool) {
        let n = lang::LANGS.len();
        self.lang_idx = if forward {
            (self.lang_idx + 1) % n
        } else if self.lang_idx == 0 { n - 1 } else { self.lang_idx - 1 };
        let name = lang::LANGS[self.lang_idx].0;
        lang::set_lang(name);
        if let Err(e) = self.mgr.set_setting("language", name) { tracing::warn!("Failed to save language: {}", e); }
    }
}

impl TabContent for SettingsTab {
    fn render(&mut self, f: &mut Frame, area: Rect, _app_type: &str) {
        let l = lang::current();
        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(area);

        let items: Vec<ListItem> = self.items().iter().enumerate().map(|(i, (name, _))| {
            let is_sel = i == self.selected;
            let arrow = if is_sel { "❯ " } else { "  " };
            let color = if is_sel { theme::current().cyan } else { theme::current().fg };
            ListItem::new(vec![Line::from(Span::styled(format!("{}{}", arrow, name), Style::default().fg(color))), Line::from("")])
        }).collect();

        let list = List::new(items).block(Block::bordered()
            .border_set(ratatui::symbols::border::ROUNDED)
            .title(l.settings_title)
            .border_style(Style::default().fg(theme::current().dim)))
            .highlight_style(Style::default());
        f.render_stateful_widget(list, main[0], &mut self.state);

        let (name, value) = &self.items()[self.selected];
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(format!("  {}", name), Style::default().fg(theme::current().cyan))),
            Line::from(""),
            Line::from(Span::styled(format!("  <  {}  >", value), Style::default().fg(theme::current().purple))),
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled(format!("  {}", l.setting_toggle_hint), Style::default().fg(theme::current().dim))),
        ];
        let p = Paragraph::new(lines).block(Block::bordered()
            .border_set(ratatui::symbols::border::ROUNDED)
            .title(format!(" {} ", name))
            .border_style(Style::default().fg(theme::current().dim)));
        f.render_widget(p, main[1]);
    }

    fn handle_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Tab | KeyCode::BackTab => return false,
            KeyCode::Char('j') | KeyCode::Down => { self.selected = (self.selected + 1).min(2); self.state.select(Some(self.selected)); }
            KeyCode::Char('k') | KeyCode::Up => { self.selected = self.selected.saturating_sub(1); self.state.select(Some(self.selected)); }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => match self.selected {
                0 => self.cycle_theme(true), 1 => self.cycle_mode(true), 2 => self.cycle_lang(true), _ => {}
            },
            KeyCode::Left | KeyCode::Char('h') => match self.selected {
                0 => self.cycle_theme(false), 1 => self.cycle_mode(false), 2 => self.cycle_lang(false), _ => {}
            },
            _ => return false,
        }
        true
    }

    fn shortcut_groups(&self) -> Vec<Vec<(String, Color)>> {
        let l = lang::current();
        vec![
            vec![(" J/K ".into(), theme::current().comment), (l.sc_nav.into(), theme::current().comment)],
            vec![(" ←/→ ".into(), theme::current().comment), (l.sc_toggle.into(), theme::current().comment)],
            vec![(" Q ".into(), theme::current().comment), (l.sc_quit.into(), theme::current().comment)],
        ]
    }

    fn shortcut_lines(&self, available_width: u16) -> usize {
        let widths = [9usize, 10, 8];
        let sep = 2usize;
        let w = available_width.saturating_sub(2).max(10) as usize;
        let mut lines = 1usize;
        let mut cur = 0usize;
        for gw in &widths {
            if cur + gw > w && cur > 0 { lines += 1; cur = 0; }
            if cur > 0 { cur += sep; }
            cur += gw;
        }
        lines
    }
}
