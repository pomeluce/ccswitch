use std::path::PathBuf;
use std::sync::Arc;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{Frame, Terminal, backend::CrosstermBackend};

use crate::core::config::ConfigManager;

use super::tabs::{history::HistoryTab, providers::ProvidersTab, usage::UsageTab, Tab, TabContent};
use super::theme::Theme;

#[allow(dead_code)]
pub struct App {
    pub mgr: Arc<ConfigManager>,
    pub active_tab: Tab,
    pub providers_tab: ProvidersTab,
    pub usage_tab: UsageTab,
    pub history_tab: HistoryTab,
    pub should_quit: bool,
    pub status_message: String,
    pub proxy_running: bool,
}

impl App {
    pub fn new(db_path: PathBuf, defaults_path: PathBuf) -> anyhow::Result<Self> {
        let mgr = Arc::new(ConfigManager::new(&db_path, Some(&defaults_path))?);
        let proxy_running = mgr
            .db()
            .get_setting("proxy_mode")
            .map(|v| v == "true")
            .unwrap_or(false);
        let providers_tab = ProvidersTab::new(&*mgr);
        let usage_tab = UsageTab::new(&*mgr);
        let history_tab = HistoryTab::new(mgr.clone());
        Ok(App {
            mgr,
            active_tab: Tab::Providers,
            providers_tab,
            usage_tab,
            history_tab,
            should_quit: false,
            status_message: String::new(),
            proxy_running,
        })
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        let mut terminal = ratatui::init();
        let result = self.event_loop(&mut terminal);
        ratatui::restore();
        result
    }

    fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> anyhow::Result<()> {
        while !self.should_quit {
            terminal.draw(|f| self.render(f))?;
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    self.handle_key(key.code);
                }
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Tab => self.next_tab(),
            KeyCode::BackTab => self.prev_tab(),
            KeyCode::Char('1') => self.active_tab = Tab::Providers,
            KeyCode::Char('2') => self.active_tab = Tab::Usage,
            KeyCode::Char('3') => self.active_tab = Tab::History,
            _ => match self.active_tab {
                Tab::Providers => self.providers_tab.handle_key(code),
                Tab::Usage => self.usage_tab.handle_key(code),
                Tab::History => self.history_tab.handle_key(code),
            },
        }
    }

    fn next_tab(&mut self) {
        self.active_tab = match self.active_tab {
            Tab::Providers => Tab::Usage,
            Tab::Usage => Tab::History,
            Tab::History => Tab::Providers,
        };
    }

    fn prev_tab(&mut self) {
        self.active_tab = match self.active_tab {
            Tab::Providers => Tab::History,
            Tab::Usage => Tab::Providers,
            Tab::History => Tab::Usage,
        };
    }

    fn render(&mut self, f: &mut Frame) {
        use ratatui::layout::{Constraint, Direction, Layout};

        let area = f.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        // Tab bar
        self.render_tab_bar(f, chunks[0]);
        // Content
        match self.active_tab {
            Tab::Providers => self.providers_tab.render(f, chunks[1]),
            Tab::Usage => self.usage_tab.render(f, chunks[1]),
            Tab::History => self.history_tab.render(f, chunks[1]),
        }
    }

    fn render_tab_bar(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        use ratatui::{
            style::{Color, Style},
            text::{Line, Span},
            widgets::{Block, Paragraph},
        };
        use super::tabs::Tab;

        let tabs = [
            (Tab::Providers, " 模型 "),
            (Tab::Usage, " 用量 "),
            (Tab::History, " 会话 "),
        ];

        // Build tab spans: active = cyan block, inactive = dim text
        let tab_spans: Vec<Span> = tabs
            .iter()
            .flat_map(|(tab, label)| {
                if *tab == self.active_tab {
                    vec![
                        Span::styled(*label, Style::default().fg(Color::Black).bg(Theme::CYAN)),
                        Span::styled(" ", Style::default()),
                    ]
                } else {
                    vec![
                        Span::styled(*label, Style::default().fg(Theme::DIM)),
                        Span::styled(" ", Style::default()),
                    ]
                }
            })
            .collect();

        // Left: app name
        let left = Span::styled(" ccswitch ", Style::default().fg(Theme::DIM));
        // Right: mode
        let mode = if self.proxy_running {
            Span::styled(" 代理模式 ", Style::default().fg(Theme::GREEN))
        } else {
            Span::styled(" 本地模式 ", Style::default().fg(Theme::DIM))
        };

        // Fill space between tabs and mode
        let fill_width = area.width.saturating_sub(
            10 + // " ccswitch "
            tab_spans.iter().map(|s| s.width()).sum::<usize>() as u16 +
            10 // " 代理模式 " / " 本地模式 "
        );
        let fill = if fill_width > 0 {
            Span::styled(" ".repeat(fill_width as usize), Style::default())
        } else {
            Span::styled(" ", Style::default())
        };

        let line = Line::from(
            std::iter::once(left)
                .chain(tab_spans.into_iter())
                .chain(std::iter::once(fill))
                .chain(std::iter::once(mode))
                .collect::<Vec<_>>(),
        );

        let p = Paragraph::new(line).block(
            Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                .border_style(Style::default().fg(Theme::DIM))
        );
        f.render_widget(p, area);
    }
}
