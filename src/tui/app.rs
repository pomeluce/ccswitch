use std::sync::{mpsc, Arc};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{backend::CrosstermBackend, Frame, Terminal};

use crate::core::config::ConfigManager;

use super::tabs::{history::HistoryTab, providers::ProvidersTab, settings::SettingsTab, usage::UsageTab, Tab, TabContent};

pub struct App {
    pub mgr: Arc<ConfigManager>,
    pub active_tab: Tab,
    pub providers_tab: ProvidersTab,
    pub usage_tab: UsageTab,
    pub history_tab: HistoryTab,
    pub settings_tab: SettingsTab,
    pub should_quit: bool,
    /// 30s polling channel — receives true when JSONL files change
    poll_rx: Option<mpsc::Receiver<bool>>,
}

impl App {
    pub fn new(db_path: &std::path::Path, defaults_path: Option<&std::path::Path>) -> anyhow::Result<Self> {
        let mgr = Arc::new(ConfigManager::new(db_path, defaults_path)?);
        let providers_tab = ProvidersTab::new(mgr.clone());
        let usage_tab = UsageTab::new(mgr.clone());
        let history_tab = HistoryTab::new(mgr.clone());
        let settings_tab = SettingsTab::new(mgr.clone());
        let poll_rx = Some(super::file_watcher::spawn_polling_thread(30));

        Ok(App {
            mgr,
            active_tab: Tab::Providers,
            providers_tab,
            usage_tab,
            history_tab,
            settings_tab,
            should_quit: false,
            poll_rx,
        })
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        let mut terminal = ratatui::init();
        let result = self.event_loop(&mut terminal);
        ratatui::restore();
        self.usage_tab.shutdown();
        result
    }

    fn event_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> anyhow::Result<()> {
        while !self.should_quit {
            self.usage_tab.poll_scan_events();
            self.poll_file_changes();

            terminal.draw(|f| self.render(f))?;

            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        self.handle_key(key.code);
                    }
                }
            }
            if self.history_tab.needs_terminal_reinit {
                ratatui::restore();
                if let Some(ref project) = self.history_tab.launch_project.take() {
                    let sid = self.history_tab.launch_session_id.take().unwrap_or_default();
                    println!("\n  Launching Claude Code session {} in {}\n", sid, project);
                    let mut cmd = std::process::Command::new("claude");
                    cmd.current_dir(project);
                    if !sid.is_empty() {
                        cmd.args(["--resume", &sid]);
                    }
                    if let Err(e) = cmd.status() {
                        eprintln!("Failed to launch Claude: {}", e);
                    }
                    print!("\n  Returning to CCSwitch...\n");
                }
                *terminal = ratatui::init();
                self.history_tab.needs_terminal_reinit = false;
            }
        }
        Ok(())
    }

    fn poll_file_changes(&mut self) {
        if let Some(rx) = &self.poll_rx {
            match rx.try_recv() {
                Ok(true) => {
                    tracing::info!("File watcher: changes detected, running incremental imports");
                    if let Err(e) = crate::core::import::import_claude_sessions(self.mgr.db()) {
                        tracing::warn!("Polling session import failed: {}", e);
                    } else {
                        let sessions = self
                            .mgr
                            .db()
                            .query_sessions("claude", None, None, 200)
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|s| s.size_bytes > 0)
                            .collect::<Vec<_>>();
                        self.history_tab.all_sessions = sessions;
                        self.history_tab.refresh();
                    }
                    if !self.usage_tab.is_scanning() {
                        self.usage_tab.trigger_incremental_scan();
                    }
                }
                Ok(false) => {}
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.poll_rx = None;
                }
            }
        }
    }

    fn handle_key(&mut self, code: KeyCode) {
        let handled = match self.active_tab {
            Tab::Providers => self.providers_tab.handle_key(code),
            Tab::Usage => self.usage_tab.handle_key(code),
            Tab::History => self.history_tab.handle_key(code),
            Tab::Settings => self.settings_tab.handle_key(code),
        };
        if handled {
            return;
        }

        match code {
            KeyCode::Tab => { self.next_tab(); }
            KeyCode::BackTab => { self.prev_tab(); }
            KeyCode::Char('q') | KeyCode::Char('Q') => self.should_quit = true,
            _ => {}
        }
    }

    fn next_tab(&mut self) {
        self.active_tab = match self.active_tab {
            Tab::Providers => Tab::Usage,
            Tab::Usage => Tab::History,
            Tab::History => Tab::Settings,
            Tab::Settings => Tab::Providers,
        };
    }

    fn prev_tab(&mut self) {
        self.active_tab = match self.active_tab {
            Tab::Providers => Tab::Settings,
            Tab::Settings => Tab::History,
            Tab::Usage => Tab::Providers,
            Tab::History => Tab::Usage,
        };
    }

    fn render(&mut self, f: &mut Frame) {
        use super::widgets::shared::render_shortcut_bar;
        use crate::tui::lang;
        use ratatui::layout::{Constraint, Direction, Layout};
        use ratatui::style::Style;
        use ratatui::text::{Line, Span};
        use ratatui::widgets::Paragraph;

        let area = f.area();

        let main_width = area.width.saturating_sub(2);
        let sc_lines = match self.active_tab {
            Tab::Providers => self.providers_tab.shortcut_lines(main_width),
            Tab::Usage => self.usage_tab.shortcut_lines(main_width),
            Tab::History => self.history_tab.shortcut_lines(main_width),
            Tab::Settings => self.settings_tab.shortcut_lines(main_width),
        };

        // Layout: tab_bar | content | shortcuts
        let [tab_bar_area, content_area, sc_area] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(2 + sc_lines as u16),
            ])
            .areas(area);

        // ── Tab bar ──
        let tabs: [(&str, Tab); 4] = [
            (lang::current().tab_providers, Tab::Providers),
            (lang::current().tab_usage, Tab::Usage),
            (lang::current().tab_history, Tab::History),
            (lang::current().tab_settings, Tab::Settings),
        ];
        let tab_spans: Vec<Span> = tabs
            .iter()
            .flat_map(|(label, tab)| {
                let style = if *tab == self.active_tab {
                    Style::default().fg(super::theme::current().cyan)
                } else {
                    Style::default().fg(super::theme::current().dim)
                };
                vec![
                    Span::styled(" ", Style::default()),
                    Span::styled(*label, style),
                ]
            })
            .collect();
        let tab_bar = Paragraph::new(Line::from(tab_spans));
        f.render_widget(tab_bar, tab_bar_area);

        // ── Content ──
        match self.active_tab {
            Tab::Providers => self.providers_tab.render(f, content_area),
            Tab::Usage => self.usage_tab.render(f, content_area),
            Tab::History => self.history_tab.render(f, content_area),
            Tab::Settings => self.settings_tab.render(f, content_area),
        }

        // ── Shortcut bar ──
        let groups = match self.active_tab {
            Tab::Providers => self.providers_tab.shortcut_groups(),
            Tab::Usage => self.usage_tab.shortcut_groups(),
            Tab::History => self.history_tab.shortcut_groups(),
            Tab::Settings => self.settings_tab.shortcut_groups(),
        };
        render_shortcut_bar(f, sc_area, &groups);
    }
}
