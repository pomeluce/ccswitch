use super::super::lang;
use super::super::theme::{self, THEMES};
use super::super::widgets::shared::{display_width, pad_label};
use super::TabContent;
use crate::core::config::ConfigManager;
use crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};
use std::sync::Arc;

const MODES: &[&str] = &["local", "proxy"];

pub struct SettingsTab {
    mgr: Arc<ConfigManager>,
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

        let mode_idx = if mgr.get_setting("proxy_mode").map(|v| v == "true").unwrap_or(false) { 1 } else { 0 };
        SettingsTab {
            mgr,
            selected: 0,
            theme_idx,
            mode_idx,
            lang_idx,
        }
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
        } else if self.theme_idx == 0 {
            THEMES.len() - 1
        } else {
            self.theme_idx - 1
        };
        theme::set_theme(THEMES[self.theme_idx]);
        if let Err(e) = self.mgr.set_setting("theme", THEMES[self.theme_idx]) {
            tracing::warn!("Failed to save theme: {}", e);
        }
    }

    fn cycle_mode(&mut self, forward: bool) {
        self.mode_idx = if forward {
            (self.mode_idx + 1) % MODES.len()
        } else if self.mode_idx == 0 {
            MODES.len() - 1
        } else {
            self.mode_idx - 1
        };
        let is_proxy = self.mode_idx == 1;
        if let Err(e) = self.mgr.set_setting("proxy_mode", &is_proxy.to_string()) {
            tracing::warn!("Failed to save mode: {}", e);
        }

        // Immediately apply the mode change to settings.json if a profile is active
        let mode = if is_proxy {
            crate::core::models::SwitchMode::Proxy
        } else {
            crate::core::models::SwitchMode::Local
        };
        if let (Some(prov_id), Some(prof_id)) = (
            self.mgr.get_setting("active_provider"),
            self.mgr.get_setting("active_profile"),
        ) {
            if let Err(e) = crate::core::switcher::switch_profile(
                &self.mgr, &prov_id, &prof_id, mode, None,
            ) {
                tracing::warn!("Failed to apply mode switch: {}", e);
            }
        }
    }

    fn cycle_lang(&mut self, forward: bool) {
        let n = lang::LANGS.len();
        self.lang_idx = if forward {
            (self.lang_idx + 1) % n
        } else if self.lang_idx == 0 {
            n - 1
        } else {
            self.lang_idx - 1
        };
        let name = lang::LANGS[self.lang_idx].0;
        lang::set_lang(name);
        if let Err(e) = self.mgr.set_setting("language", name) {
            tracing::warn!("Failed to save language: {}", e);
        }
    }
}

impl TabContent for SettingsTab {
    fn render(&mut self, f: &mut Frame, area: Rect) {
        let l = lang::current();
        let items = self.items();

        // Calculate max label display-width for : alignment
        let max_label_dw = items.iter().map(|(label, _)| display_width(label)).max().unwrap_or(0);

        // Calculate max value display-width (including < > brackets)
        let max_value_dw = items.iter().map(|(_, v)| display_width(v) + 2).max().unwrap_or(0);

        // Content width = label: + value<>, pad_w centres this in the inner area
        let inner_w = area.width.saturating_sub(2) as usize;
        let content_w = max_label_dw + 3 + max_value_dw;
        let pad_w = inner_w.saturating_sub(content_w) / 2;
        let pad = " ".repeat(pad_w);

        let mut lines: Vec<Line> = vec![Line::from(""), Line::from("")];
        for (i, (label, value)) in items.iter().enumerate() {
            let is_sel = i == self.selected;
            let label_color = if is_sel { theme::current().cyan } else { theme::current().fg };
            let value_style = if is_sel {
                Style::default().fg(theme::current().purple)
            } else {
                Style::default().fg(theme::current().dim)
            };

            lines.push(Line::from(vec![
                Span::styled(pad.clone(), Style::default()),
                Span::styled(pad_label(label, max_label_dw), Style::default().fg(label_color)),
                Span::styled(format!("<{}>", value), value_style),
                Span::styled(pad.clone(), Style::default()),
            ]));

            if i < items.len() - 1 {
                lines.push(Line::from(""));
            }
        }

        let block = Block::bordered()
            .border_set(ratatui::symbols::border::ROUNDED)
            .title(l.settings_title)
            .border_style(Style::default().fg(theme::current().dim));

        let p = Paragraph::new(lines).block(block);
        f.render_widget(p, area);
    }

    fn handle_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Tab | KeyCode::BackTab => return false,
            KeyCode::Char('j') | KeyCode::Down => {
                let max = self.items().len().saturating_sub(1);
                self.selected = (self.selected + 1).min(max);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Char('l') | KeyCode::Right => match self.selected {
                0 => self.cycle_theme(true),
                1 => self.cycle_mode(true),
                2 => self.cycle_lang(true),
                _ => {}
            },
            KeyCode::Char('h') | KeyCode::Left => match self.selected {
                0 => self.cycle_theme(false),
                1 => self.cycle_mode(false),
                2 => self.cycle_lang(false),
                _ => {}
            },
            _ => return false,
        }
        true
    }

    fn shortcut_groups(&self) -> Vec<Vec<(String, Color)>> {
        let l = lang::current();
        vec![
            vec![(" J/K ".into(), theme::current().comment), (l.sc_nav.into(), theme::current().comment)],
            vec![(" H/L ".into(), theme::current().comment), (l.sc_toggle.into(), theme::current().comment)],
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
