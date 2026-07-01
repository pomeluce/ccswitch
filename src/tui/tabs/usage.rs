use super::super::theme::Theme;
use super::super::widgets::shared::{render_shortcut_bar as shared_shortcuts, render_search_box as shared_search};
use super::TabContent;
use crate::core::config::ConfigManager;
use crate::db::usage::{ScanContext, ScanEvent, UsageSummary};
use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph},
    Frame,
};
use std::sync::mpsc;
use std::sync::Arc;

/// Background scan state, updated by poll_scan_events()
enum ScanState {
    Idle,
    Scanning {
        files_done: usize,
        files_total: usize,
        records: usize,
    },
}

pub struct UsageTab {
    mgr: Arc<ConfigManager>,
    pub summaries: Vec<UsageSummary>,
    pub state: ListState,
    pub selected_index: usize,
    pub range: String,
    pub search_query: String,
    pub is_searching: bool,
    chart_scroll: usize,
    /// Background scan receiver + state
    scan_rx: Option<mpsc::Receiver<ScanEvent>>,
    scan_state: ScanState,
}

impl UsageTab {
    pub fn new(mgr: Arc<ConfigManager>) -> Self {
        let scan_state;
        let scan_rx;

        // Check if this is first launch (no usage data yet)
        let is_first_launch = {
            let db = mgr.usage_db();
            let count: i64 = db.conn()
                .query_row("SELECT COUNT(*) FROM usage_logs", [], |r| r.get(0))
                .unwrap_or(0);
            count == 0
        };

        // Prepare scan context on main thread (DB access only, fast) then spawn background parser
        {
            let (tx, rx) = mpsc::channel();
            let ctx = match mgr.usage_db().prepare_scan_context() {
                Ok(c) => {
                    tracing::info!("Scan prep: {} known msg IDs, {} files in index", c.known_msg_ids.len(), c.file_index.len());
                    c
                }
                Err(e) => {
                    tracing::error!("Failed to prepare scan context: {}", e);
                    ScanContext { known_msg_ids: Vec::new(), file_index: std::collections::HashMap::new() }
                }
            };
            // Always spawn background thread — it does its own file collection
            std::thread::spawn(move || {
                crate::db::usage::parse_files_in_background(ctx, 10, tx);
            });
            scan_rx = Some(rx);
            if is_first_launch {
                scan_state = ScanState::Scanning { files_done: 0, files_total: 0, records: 0 };
            } else {
                scan_state = ScanState::Idle;
            }
        }

        let summaries = mgr.usage_db().query_usage("all").unwrap_or_default();
        let mut state = ListState::default();
        if !summaries.is_empty() {
            state.select(Some(0));
        }
        UsageTab {
            mgr,
            summaries,
            state,
            selected_index: 0,
            range: "all".into(),
            search_query: String::new(),
            is_searching: false,
            chart_scroll: 0,
            scan_rx,
            scan_state,
        }
    }

    fn token_total(s: &UsageSummary) -> i64 {
        s.total_prompt + s.total_completion + s.total_cache_read + s.total_cache_create
    }
    fn total_tokens(&self) -> i64 {
        self.summaries.iter().map(|s| Self::token_total(s)).sum()
    }
    fn max_tokens(&self) -> i64 {
        self.summaries.iter().map(|s| Self::token_total(s)).max().unwrap_or(1)
    }

    /// Called every event-loop tick — process at most one Batch to stay responsive.
    /// Progress and Done events drain eagerly (they're instant, no DB writes).
    pub fn poll_scan_events(&mut self) {
        let mut processed_batch = false;
        loop {
            // Extract one event, ending the borrow on self.scan_rx before mutating self
            let event = match &self.scan_rx {
                Some(rx) => match rx.try_recv() {
                    Ok(e) => e,
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        self.scan_state = ScanState::Idle;
                        self.scan_rx = None;
                        break;
                    }
                },
                None => return,
            };

            // Now `event` is owned — no borrow on self
            match event {
                ScanEvent::Batch { .. } if processed_batch => break,
                ScanEvent::Batch { sid, file_path, records } => {
                    processed_batch = true;
                    if !records.is_empty() {
                        if let Err(e) = self.mgr.usage_db().insert_usage_batch(&sid, &file_path, &records) {
                            tracing::error!("Failed to insert usage batch: {}", e);
                        }
                    }
                }
                ScanEvent::Progress { files_done, files_total, records } => {
                    if matches!(self.scan_state, ScanState::Scanning { .. }) {
                        self.scan_state = ScanState::Scanning { files_done, files_total, records };
                    }
                }
                ScanEvent::Done {} => {
                    tracing::info!("Usage scan complete");
                    self.scan_state = ScanState::Idle;
                    self.scan_rx = None;
                    self.summaries = self.mgr.usage_db().query_usage(&self.range).unwrap_or_default();
                    if !self.summaries.is_empty() && self.state.selected().is_none() {
                        self.state.select(Some(0));
                    }
                    break;
                }
            }
        }
    }
}

impl TabContent for UsageTab {
    fn render(&mut self, f: &mut Frame, area: Rect) {
        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Length(4), Constraint::Min(3)])
            .split(main[0]);

        // Search box
        self.render_search_box(f, left[0]);
        // Summary cards
        self.render_summary_cards(f, left[1]);
        // Profile ranking
        self.render_profile_list(f, left[2]);

        // Right: daily chart (or scan progress) + shortcut bar
        let sc_lines = usage_shortcut_lines(main[1].width);
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(2 + sc_lines as u16),
            ])
            .split(main[1]);

        match &self.scan_state {
            ScanState::Scanning { files_done, files_total, records } => {
                self.render_scan_progress(f, right[0], *files_done, *files_total, *records);
            }
            ScanState::Idle => {
                self.render_daily_chart(f, right[0]);
            }
        }
        self.render_shortcut_bar(f, right[1]);
    }

    fn handle_key(&mut self, code: KeyCode) -> bool {
        if self.is_searching {
            match code {
                KeyCode::Esc => {
                    self.is_searching = false;
                    self.search_query.clear();
                }
                KeyCode::Enter => {
                    self.is_searching = false;
                }
                KeyCode::Backspace | KeyCode::Delete => {
                    self.search_query.pop();
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                }
                _ => {}
            }
            return true;
        }
        match code {
            KeyCode::Tab | KeyCode::BackTab => return false,
            KeyCode::Char('j') | KeyCode::Down => {
                if self.selected_index + 1 < self.summaries.len() {
                    self.selected_index += 1;
                    self.state.select(Some(self.selected_index));
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                    self.state.select(Some(self.selected_index));
                }
            }
            KeyCode::Char('t') | KeyCode::Char('T') => {
                self.range = match self.range.as_str() {
                    "day" => "week",
                    "week" => "month",
                    _ => "day",
                }
                .into();
                self.summaries = self.mgr.usage_db().query_usage(&self.range).unwrap_or_default();
            }
            KeyCode::Char('/') => {
                self.is_searching = true;
            }
            KeyCode::PageUp => {
                self.chart_scroll = self.chart_scroll.saturating_sub(5);
            }
            KeyCode::PageDown => {
                self.chart_scroll = self.chart_scroll.saturating_add(5);
            }
            _ => return false,
        }
        true
    }
}

impl UsageTab {
    fn render_shortcut_bar(&self, f: &mut Frame, area: Rect) {
        let groups: Vec<Vec<(String, Color)>> = vec![
            vec![(" J/K ".into(), Theme::CYAN), ("Nav".into(), Theme::COMMENT)],
            vec![(" / ".into(), Theme::CYAN), ("Search".into(), Theme::COMMENT)],
            vec![(" T ".into(), Theme::GREEN), ("Toggle".into(), Theme::COMMENT)],
            vec![(" PgUp/Dn ".into(), Theme::PURPLE), ("Scroll".into(), Theme::COMMENT)],
            vec![(" Q ".into(), Theme::ORANGE), ("Quit".into(), Theme::COMMENT)],
        ];
        shared_shortcuts(f, area, &groups);
    }

    fn render_search_box(&self, f: &mut Frame, area: Rect) {
        shared_search(f, area, &self.search_query, self.is_searching);
    }

    fn render_summary_cards(&self, f: &mut Frame, area: Rect) {
        let cards = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Ratio(1, 4); 4]).split(area);

        let (today, week, all, reqs) = if let Some(s) = self.summaries.get(self.selected_index) {
            let t = Self::token_total(s);
            (t / 7, t, t * 4, s.request_count)
        } else {
            (0, 0, 0, 0)
        };

        let card_data = [
            ("Today", &format_tokens(today), Theme::GREEN),
            ("Week", &format_tokens(week), Theme::CYAN),
            ("Total", &format_tokens(all), Theme::PURPLE),
            ("Reqs", &format!("{}", reqs), Theme::YELLOW),
        ];

        for (i, (label, value, color)) in card_data.iter().enumerate() {
            let lines = vec![
                Line::from(Span::styled(*label, Style::default().fg(Theme::COMMENT))).centered(),
                Line::from(Span::styled(value.to_string(), Style::default().fg(*color))).centered(),
            ];
            let p = Paragraph::new(lines).block(
                Block::bordered()
                    .border_set(ratatui::symbols::border::ROUNDED)
                    .border_style(Style::default().fg(Theme::DIM)),
            );
            f.render_widget(p, cards[i]);
        }
    }

    fn render_profile_list(&mut self, f: &mut Frame, area: Rect) {
        let max = self.max_tokens();
        let items: Vec<ListItem> = self
            .summaries
            .iter()
            .filter(|s| Self::token_total(s) > 0)
            .enumerate()
            .map(|(i, s)| {
                let total = Self::token_total(s);
                let pct = if max > 0 { (total as f64 / max as f64 * 100.0) as usize } else { 0 };
                let bar_len = if total > 0 { (pct / 4).max(1).min(20) } else { 0 };
                let bar = "\u{2500}".repeat(bar_len);
                let label = title_case(&s.model);
                let is_sel = i == self.selected_index;
                let arrow = if is_sel { "\u{276f} " } else { "  " };
                let tc = if is_sel { Theme::CYAN } else { Theme::FG };
                let bar_text = if total > 0 { format!("{} {}%", bar, pct) } else { String::new() };
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(format!("{}{}", arrow, label), Style::default().fg(tc)),
                        Span::styled(format!("  {}", format_tokens(total)), Style::default().fg(Theme::DIM)),
                    ]),
                    Line::from(vec![Span::styled("  ", Style::default()), Span::styled(bar_text, Style::default().fg(Theme::PURPLE))]),
                    Line::from(""),
                ])
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::bordered()
                    .border_set(ratatui::symbols::border::ROUNDED)
                    .title(format!("Models — \u{3a3} {}", format_tokens(self.total_tokens())))
                    .border_style(Style::default().fg(Theme::DIM)),
            )
            .highlight_style(Style::default());
        f.render_stateful_widget(list, area, &mut self.state);
    }

    fn render_daily_chart(&mut self, f: &mut Frame, area: Rect) {
        if let Some(s) = self.summaries.get(self.selected_index) {
            let label = title_case(&s.model);
            let daily = self.mgr.usage_db().query_daily_usage(&s.model).unwrap_or_default();
            let today_date = chrono::Local::now().format("%Y-%m-%d").to_string();

            let days: Vec<(String, i64, i64, i64, i64, bool)> = (0..7)
                .filter_map(|offset| {
                    let d = chrono::Local::now() - chrono::Duration::days(offset);
                    let date_str = d.format("%Y-%m-%d").to_string();
                    let (in_tok, out_tok, cr_tok, cc_tok) = daily
                        .iter()
                        .find(|(dt, _, _, _, _)| dt == &date_str)
                        .map(|(_, i, o, cr, cc)| (*i, *o, *cr, *cc))
                        .unwrap_or((0, 0, 0, 0));
                    let total = in_tok + out_tok + cr_tok + cc_tok;
                    if total == 0 {
                        None
                    } else {
                        Some((d.format("%m-%d").to_string(), in_tok, out_tok, cr_tok, cc_tok, date_str == today_date))
                    }
                })
                .collect();

            let max_val = days.iter().map(|(_, i, o, cr, cc, _)| i + o + cr + cc).max().unwrap_or(1).max(1);
            let lines: Vec<Line> = days
                .iter()
                .flat_map(|(date, in_tok, out_tok, cr_tok, cc_tok, is_today)| {
                    let total = in_tok + out_tok + cr_tok + cc_tok;
                    let w = if max_val > 0 { (total as f64 / max_val as f64 * 30.0) as usize } else { 0 };
                    let w = if total > 0 { w.max(1) } else { 0 };
                    let bar = "\u{2500}".repeat(w.min(35));
                    let color = if *is_today { Theme::ORANGE } else { Theme::PURPLE };
                    let indent = "       ";
                    let detail_lines: Vec<Line> = if total > 0 {
                        let text = format!(
                            "input {}  output {}  cache read {}  cache create {}",
                            format_tokens(*in_tok),
                            format_tokens(*out_tok),
                            format_tokens(*cr_tok),
                            format_tokens(*cc_tok)
                        );
                        let max_w = (area.width as usize).saturating_sub(indent.len() + 2).max(10);
                        let mut result = vec![Line::from(vec![
                            Span::styled(indent, Style::default()),
                            Span::styled(text.chars().take(max_w).collect::<String>(), Style::default().fg(Theme::COMMENT)),
                        ])];
                        let remainder: String = text.chars().skip(max_w).collect();
                        for chunk in remainder.chars().collect::<Vec<_>>().chunks(max_w) {
                            let cont: String = chunk.iter().collect();
                            if !cont.is_empty() {
                                result.push(Line::from(Span::styled(format!("{}{}", indent, cont), Style::default().fg(Theme::COMMENT))));
                            }
                        }
                        result
                    } else {
                        vec![]
                    };
                    let mut day_lines = vec![Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(format!("{}  ", date), Style::default().fg(Theme::COMMENT)),
                        Span::styled(bar, Style::default().fg(color)),
                        Span::styled(format!(" {}", format_tokens(total)), Style::default().fg(if *is_today { Theme::ORANGE } else { Theme::DIM })),
                    ])];
                    day_lines.extend(detail_lines);
                    day_lines.push(Line::from(""));
                    day_lines
                })
                .collect();

            let visible = (area.height as usize).saturating_sub(2);
            let max_scroll = lines.len().saturating_sub(visible);
            self.chart_scroll = self.chart_scroll.min(max_scroll);
            let lines: Vec<Line> = lines.into_iter().skip(self.chart_scroll).take(visible).collect();

            let p = Paragraph::new(lines).block(
                Block::bordered()
                    .border_set(ratatui::symbols::border::ROUNDED)
                    .title(format!("{} — This Week", label))
                    .border_style(Style::default().fg(Theme::DIM)),
            );
            f.render_widget(p, area);
        } else {
            let p = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled("  No usage data yet", Style::default().fg(Theme::COMMENT))).centered(),
                Line::from(""),
                Line::from(Span::styled("  Scan starts automatically on first launch", Style::default().fg(Theme::DIM))).centered(),
            ]).block(
                Block::bordered()
                    .border_set(ratatui::symbols::border::ROUNDED)
                    .title(" Usage ")
                    .border_style(Style::default().fg(Theme::DIM)),
            );
            f.render_widget(p, area);
        }
    }

    /// Render scan progress in the right panel（扫描进度视图）
    fn render_scan_progress(&self, f: &mut Frame, area: Rect, files_done: usize, files_total: usize, records: usize) {
        let pct = if files_total > 0 {
            (files_done as f64 / files_total as f64 * 100.0) as usize
        } else {
            0
        };
        let bar_w = if files_total > 0 {
            ((files_done as f64 / files_total.max(1) as f64) * 30.0) as usize
        } else {
            0
        };
        let bar_w = bar_w.min(30);
        let filled = "\u{2588}".repeat(bar_w);
        let empty = "\u{2591}".repeat(30usize.saturating_sub(bar_w));
        let bar = format!("{}{}", filled, empty);

        let spinner = ["\u{280b}", "\u{2819}", "\u{2839}", "\u{2833}", "\u{2827}", "\u{280f}", "\u{281f}", "\u{283f}"][files_done % 8];

        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(format!("    {}  Scanning Claude Code sessions...", spinner), Style::default().fg(Theme::CYAN))).centered(),
            Line::from(""),
            Line::from(Span::styled(format!("    {} {}  {} / {} files", bar, pct, files_done, files_total), Style::default().fg(Theme::PURPLE))).centered(),
            Line::from(""),
            Line::from(Span::styled(format!("    {} records imported", records), Style::default().fg(Theme::COMMENT))).centered(),
            Line::from(""),
            Line::from(Span::styled("    Data refreshes automatically when complete", Style::default().fg(Theme::DIM))).centered(),
        ];

        let p = Paragraph::new(lines).block(
            Block::bordered()
                .border_set(ratatui::symbols::border::ROUNDED)
                .title(" Scanning ")
                .border_style(Style::default().fg(Theme::PURPLE)),
        );
        f.render_widget(p, area);
    }
}

fn title_case(s: &str) -> String {
    let mut result = String::new();
    let mut upper = true;
    for c in s.chars() {
        if c == '-' || c == '.' || c == '_' {
            upper = true;
            result.push(c);
        } else if upper {
            result.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Pre-calculate shortcut bar lines to match shared_shortcuts actual rendering
fn usage_shortcut_lines(available_width: u16) -> usize {
    let widths = [9usize, 10, 10, 17, 8];
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

fn format_tokens(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
