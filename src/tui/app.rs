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
}

impl App {
    pub fn new(db_path: PathBuf, defaults_path: PathBuf) -> anyhow::Result<Self> {
        let mgr = Arc::new(ConfigManager::new(&db_path, Some(&defaults_path))?);
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

    fn render(&mut self, f: &mut Frame) {
        use ratatui::layout::{Constraint, Direction, Layout};

        let area = f.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
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
            style::Style,
            text::{Line, Span},
            widgets::Paragraph,
        };
        use super::tabs::Tab;

        let tabs = [
            (Tab::Providers, "[1] 模型"),
            (Tab::Usage, "[2] 用量"),
            (Tab::History, "[3] 会话"),
        ];

        let spans: Vec<Span> = tabs
            .iter()
            .flat_map(|(tab, label)| {
                let style = if *tab == self.active_tab {
                    Style::default().fg(Theme::CYAN)
                } else {
                    Style::default().fg(Theme::DIM)
                };
                vec![
                    Span::styled(*label, style),
                    Span::styled("  ", Style::default()),
                ]
            })
            .collect();

        let p = Paragraph::new(Line::from(spans));
        f.render_widget(p, area);
    }
}
