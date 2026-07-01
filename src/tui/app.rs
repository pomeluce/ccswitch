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
        let providers_tab = ProvidersTab::new(mgr.clone());
        let usage_tab = UsageTab::new(mgr.clone());
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
            // Poll background scan events every tick (for smooth progress bar)
            self.usage_tab.poll_scan_events();

            terminal.draw(|f| self.render(f))?;

            // Non-blocking poll: 100ms timeout so scan progress updates without keypresses
            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) {
                            continue;
                        }
                        self.handle_key(key.code);
                    }
                }
            }
            // Handle terminal suspend for external process (claude)
            if self.history_tab.needs_terminal_reinit {
                ratatui::restore();
                if let Some(ref project) = self.history_tab.launch_project.take() {
                    println!("\n  Launching Claude Code in {}\n", project);
                    let _ = std::process::Command::new("claude")
                        .current_dir(project)
                        .status();
                    print!("\n  Returning to CCSwitch...\n");
                }
                *terminal = ratatui::init();
                self.history_tab.needs_terminal_reinit = false;
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, code: KeyCode) {
        // Let active tab handle Tab/BackTab first (for confirm popups etc.)
        let handled = match self.active_tab {
            Tab::Providers => self.providers_tab.handle_key(code),
            Tab::Usage => self.usage_tab.handle_key(code),
            Tab::History => self.history_tab.handle_key(code),
        };
        if handled { return; }

        match code {
            KeyCode::Tab => self.next_tab(),
            KeyCode::BackTab => self.prev_tab(),
            KeyCode::Char('1') => self.active_tab = Tab::Providers,
            KeyCode::Char('2') => self.active_tab = Tab::Usage,
            KeyCode::Char('3') => self.active_tab = Tab::History,
            KeyCode::Char('q') | KeyCode::Char('Q') => self.should_quit = true,
            _ => {}
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
            style::Style,
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
                        Span::styled(*label, Style::default().fg(Theme::CYAN)),
                    ]
                } else {
                    vec![
                        Span::styled(*label, Style::default().fg(Theme::DIM)),
                    ]
                }
            })
            .collect();

        // Calculate widths
        let left_label = " ccswitch ";
        let left_width = left_label.len() as u16;
        let mode_label = if self.proxy_running { " 模式: proxy " } else { " 模式: local " };
        let mode_width = mode_label.len() as u16;
        let tabs_total_width: u16 = tab_spans.iter().map(|s| s.width() as u16).sum();

        // Available space for centering
        let inner_width = area.width.saturating_sub(left_width + tabs_total_width + mode_width + 4); // +4 for border padding
        let pad_left = inner_width / 2;
        let pad_right = inner_width - pad_left;

        let mut all_spans: Vec<Span> = Vec::new();
        all_spans.push(Span::styled(left_label, Style::default().fg(Theme::DIM)));
        all_spans.push(Span::styled(" ".repeat(pad_left as usize), Style::default()));
        all_spans.extend(tab_spans);
        all_spans.push(Span::styled(" ".repeat(pad_right as usize), Style::default()));
        all_spans.push(Span::styled(mode_label, if self.proxy_running {
            Style::default().fg(Theme::GREEN)
        } else {
            Style::default().fg(Theme::DIM)
        }));

        let p = Paragraph::new(Line::from(all_spans)).block(
            Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                .border_style(Style::default().fg(Theme::DIM))
        );
        f.render_widget(p, area);
    }
}
