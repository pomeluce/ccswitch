use std::path::PathBuf;
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
    pub app_type: String,
    /// 30s polling channel — receives true when JSONL files change
    poll_rx: Option<mpsc::Receiver<bool>>,
}

impl App {
    pub fn new(db_path: PathBuf, defaults_path: PathBuf) -> anyhow::Result<Self> {
        let mgr = Arc::new(ConfigManager::new(&db_path, Some(&defaults_path))?);
        let providers_tab = ProvidersTab::new(mgr.clone());
        let usage_tab = UsageTab::new(mgr.clone());
        let history_tab = HistoryTab::new(mgr.clone());
        let settings_tab = SettingsTab::new(mgr.clone());
        // Start 30s background file watcher for live incremental updates
        let poll_rx = Some(super::file_watcher::spawn_polling_thread(30));

        Ok(App {
            mgr,
            active_tab: Tab::Providers,
            providers_tab,
            usage_tab,
            history_tab,
            settings_tab,
            should_quit: false,
            app_type: "claude".to_string(),
            poll_rx,
        })
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        let mut terminal = ratatui::init();
        let result = self.event_loop(&mut terminal);
        ratatui::restore();
        // Gracefully wait for background threads
        self.usage_tab.shutdown();
        result
    }

    fn event_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> anyhow::Result<()> {
        while !self.should_quit {
            // Poll background scan events every tick (for smooth progress bar)
            self.usage_tab.poll_scan_events();

            // Check 30s file watcher for live incremental updates
            self.poll_file_changes();

            terminal.draw(|f| self.render(f))?;

            // Non-blocking poll: 100ms timeout so scan progress updates without keypresses
            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        self.handle_key(key.code);
                    }
                }
            }
            // Handle terminal suspend for external process (claude)
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

    /// Check 30s polling channel. If files changed, run incremental imports
    /// and refresh data for the active tab.
    fn poll_file_changes(&mut self) {
        if let Some(rx) = &self.poll_rx {
            match rx.try_recv() {
                Ok(true) => {
                    tracing::info!("File watcher: changes detected, running incremental imports");
                    // Incremental session import (updates existing + imports new)
                    if let Err(e) = crate::core::import::import_claude_sessions(self.mgr.db()) {
                        tracing::warn!("Polling session import failed: {}", e);
                    } else {
                        // Refresh history tab data
                        let sessions = self
                            .mgr
                            .db()
                            .query_sessions(&self.app_type, None, None, 200)
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|s| s.size_bytes > 0)
                            .collect::<Vec<_>>();
                        self.history_tab.all_sessions = sessions;
                        self.history_tab.refresh();
                    }
                    // Trigger usage scan for changed files (calls existing background scan)
                    if !self.usage_tab.is_scanning() {
                        self.usage_tab.trigger_incremental_scan();
                    }
                }
                Ok(false) => {} // No changes
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.poll_rx = None;
                }
            }
        }
    }

    fn handle_key(&mut self, code: KeyCode) {
        // Let active tab handle keys first
        let handled = match self.active_tab {
            Tab::Providers => self.providers_tab.handle_key(code),
            Tab::Usage => self.usage_tab.handle_key(code),
            Tab::History => self.history_tab.handle_key(code),
            Tab::Settings => self.settings_tab.handle_key(code),
        };
        if handled {
            return;
        }

        // Tab/Shift+Tab: sidebar tab navigation
        match code {
            KeyCode::Tab => { self.next_tab(); return; }
            KeyCode::BackTab => { self.prev_tab(); return; }
            _ => {}
        }

        // Space / Shift+Space: switch app type
        match code {
            KeyCode::Char(' ') => {
                self.app_type = super::widgets::app_bar::toggle_app_type(&self.app_type).to_string();
                return;
            }
            _ => {}
        }

        match code {
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
        use super::widgets::app_bar::render_app_bar;
        use super::widgets::shared::render_shortcut_bar;
        use super::widgets::sidebar::render_sidebar;
        use ratatui::layout::{Constraint, Direction, Layout};

        let area = f.area();

        // Calculate shortcut bar height for the active tab (width = main area, ~sidebar 14 cols)
        let main_width = area.width.saturating_sub(16);
        let sc_lines = match self.active_tab {
            Tab::Providers => self.providers_tab.shortcut_lines(main_width),
            Tab::Usage => self.usage_tab.shortcut_lines(main_width),
            Tab::History => self.history_tab.shortcut_lines(main_width),
            Tab::Settings => self.settings_tab.shortcut_lines(main_width),
        };

        // Level 1: sidebar | main
        let [sidebar_area, main_area] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(16), Constraint::Min(20)])
            .areas(area);

        // Level 2: main → app_bar | content | shortcuts
        let [app_bar_area, content_area, sc_area] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(3),
                Constraint::Length(2 + sc_lines as u16),
            ])
            .areas(main_area);

        let is_proxy = self.mgr.get_setting("proxy_mode").map(|v| v == "true").unwrap_or(false);
        render_sidebar(f, sidebar_area, self.active_tab, is_proxy);
        render_app_bar(f, app_bar_area, &self.app_type);

        match self.active_tab {
            Tab::Providers => self.providers_tab.render(f, content_area, &self.app_type),
            Tab::Usage => self.usage_tab.render(f, content_area, &self.app_type),
            Tab::History => self.history_tab.render(f, content_area, &self.app_type),
            Tab::Settings => self.settings_tab.render(f, content_area, &self.app_type),
        }

        let groups = match self.active_tab {
            Tab::Providers => self.providers_tab.shortcut_groups(),
            Tab::Usage => self.usage_tab.shortcut_groups(),
            Tab::History => self.history_tab.shortcut_groups(),
            Tab::Settings => self.settings_tab.shortcut_groups(),
        };
        render_shortcut_bar(f, sc_area, &groups);
    }
}
