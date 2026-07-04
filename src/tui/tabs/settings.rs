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

const LANGUAGES: &[&str] = &["中文", "English"];
const MODES: &[&str] = &["local", "proxy"];

pub struct SettingsTab {
    mgr: Arc<ConfigManager>,
    pub state: ListState,
    selected: usize, // 0=Theme, 1=Mode, 2=Language
    theme_idx: usize,
    mode_idx: usize,
    lang_idx: usize,
}

impl SettingsTab {
    pub fn new(mgr: Arc<ConfigManager>) -> Self {
        // Restore theme from DB (persisted across restarts)
        let saved_theme = mgr.db().get_setting("theme").unwrap_or_default();
        if !saved_theme.is_empty() {
            theme::set_theme(&saved_theme);
        }
        let current_theme = theme::current_theme();
        let theme_idx = THEMES.iter().position(|&t| t == current_theme).unwrap_or(0);

        let mode_idx = if mgr.db().get_setting("proxy_mode").map(|v| v == "true").unwrap_or(false) {
            1
        } else {
            0
        };
        let mut state = ListState::default();
        state.select(Some(0));
        SettingsTab {
            mgr,
            state,
            selected: 0,
            theme_idx,
            mode_idx,
            lang_idx: 0,
        }
    }

    fn items(&self) -> Vec<(&str, String)> {
        vec![
            ("Theme", THEMES[self.theme_idx].to_string()),
            ("Mode", MODES[self.mode_idx].to_string()),
            ("Language", format!("{} (coming soon)", LANGUAGES[self.lang_idx])),
        ]
    }

    fn cycle_theme(&mut self, forward: bool) {
        if forward {
            self.theme_idx = (self.theme_idx + 1) % THEMES.len();
        } else {
            self.theme_idx = if self.theme_idx == 0 { THEMES.len() - 1 } else { self.theme_idx - 1 };
        }
        theme::set_theme(THEMES[self.theme_idx]);
        self.mgr.db().set_setting("theme", THEMES[self.theme_idx]).ok();
    }

    fn cycle_mode(&mut self, forward: bool) {
        if forward {
            self.mode_idx = (self.mode_idx + 1) % MODES.len();
        } else {
            self.mode_idx = if self.mode_idx == 0 { MODES.len() - 1 } else { self.mode_idx - 1 };
        }
        let val = (self.mode_idx == 1).to_string();
        self.mgr.db().set_setting("proxy_mode", &val).ok();
    }

    fn cycle_lang(&mut self, _forward: bool) {
        // Placeholder — language switching not yet implemented
    }
}

impl TabContent for SettingsTab {
    fn render(&mut self, f: &mut Frame, area: Rect, _app_type: &str) {
        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(area);

        // Left: setting name list
        let items: Vec<ListItem> = self
            .items()
            .iter()
            .enumerate()
            .map(|(i, (name, _))| {
                let is_sel = i == self.selected;
                let arrow = if is_sel { "❯ " } else { "  " };
                let color = if is_sel { theme::current().cyan } else { theme::current().fg };
                ListItem::new(vec![Line::from(Span::styled(format!("{}{}", arrow, name), Style::default().fg(color))), Line::from("")])
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::bordered()
                    .border_set(ratatui::symbols::border::ROUNDED)
                    .title("Settings")
                    .border_style(Style::default().fg(theme::current().dim)),
            )
            .highlight_style(Style::default());
        f.render_stateful_widget(list, main[0], &mut self.state);

        // Right: current value + navigation hints
        let (name, value) = &self.items()[self.selected];
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(format!("  {}", name), Style::default().fg(theme::current().cyan))),
            Line::from(""),
            Line::from(Span::styled(format!("  <  {}  >", value), Style::default().fg(theme::current().purple))),
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled("  ←/→ or Enter to toggle", Style::default().fg(theme::current().dim))),
        ];

        let p = Paragraph::new(lines).block(
            Block::bordered()
                .border_set(ratatui::symbols::border::ROUNDED)
                .title(format!(" {} ", name))
                .border_style(Style::default().fg(theme::current().dim)),
        );
        f.render_widget(p, main[1]);
    }

    fn handle_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Tab | KeyCode::BackTab => return false,
            KeyCode::Char('j') | KeyCode::Down => {
                self.selected = (self.selected + 1).min(2);
                self.state.select(Some(self.selected));
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                self.state.select(Some(self.selected));
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => match self.selected {
                0 => self.cycle_theme(true),
                1 => self.cycle_mode(true),
                2 => self.cycle_lang(true),
                _ => {}
            },
            KeyCode::Left | KeyCode::Char('h') => match self.selected {
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
        vec![
            vec![(" J/K ".into(), theme::current().comment), ("Nav".into(), theme::current().comment)],
            vec![(" ←/→ ".into(), theme::current().comment), ("Toggle".into(), theme::current().comment)],
            vec![(" Q ".into(), theme::current().comment), ("Quit".into(), theme::current().comment)],
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

