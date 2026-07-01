use std::sync::Arc;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph},
    Frame,
};
use crossterm::event::KeyCode;
use crate::core::config::ConfigManager;
use crate::db::usage::UsageSummary;
use super::super::theme::Theme;
use super::TabContent;

pub struct UsageTab {
    mgr: Arc<ConfigManager>,
    pub summaries: Vec<UsageSummary>,
    pub state: ListState,
    pub selected_index: usize,
    pub range: String,
    pub search_query: String,
    pub is_searching: bool,
}

impl UsageTab {
    pub fn new(mgr: Arc<ConfigManager>) -> Self {
        // Scan local files for usage
        match mgr.usage_db().scan_local_usage() {
            Ok(n) if n > 0 => tracing::info!("Imported usage from {} sessions", n),
            _ => {}
        }
        let summaries = mgr.usage_db().query_usage("all").unwrap_or_default();
        let mut state = ListState::default();
        if !summaries.is_empty() { state.select(Some(0)); }
        UsageTab {
            mgr, summaries, state,
            selected_index: 0, range: "all".into(),
            search_query: String::new(), is_searching: false,
        }
    }

    fn token_total(s: &UsageSummary) -> i64 { s.total_prompt + s.total_completion + s.total_cache_read + s.total_cache_create }
    fn total_tokens(&self) -> i64 { self.summaries.iter().map(|s| Self::token_total(s)).sum() }
    fn max_tokens(&self) -> i64 { self.summaries.iter().map(|s| Self::token_total(s)).max().unwrap_or(1) }
}

impl TabContent for UsageTab {
    fn render(&mut self, f: &mut Frame, area: Rect) {
        let main = Layout::default().direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);

        let left = Layout::default().direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Length(4), Constraint::Min(3)])
            .split(main[0]);

        // Search box
        self.render_search_box(f, left[0]);
        // Summary cards (all profiles)
        self.render_summary_cards(f, left[1]);
        // Profile ranking
        self.render_profile_list(f, left[2]);

        // Right: daily chart with profile summary inline
        self.render_daily_chart(f, main[1]);
    }

    fn handle_key(&mut self, code: KeyCode) -> bool {
        if self.is_searching {
            match code {
                KeyCode::Esc => { self.is_searching = false; self.search_query.clear(); }
                KeyCode::Enter => { self.is_searching = false; }
                KeyCode::Backspace | KeyCode::Delete => { self.search_query.pop(); }
                KeyCode::Char(c) => { self.search_query.push(c); }
                _ => {}
            }
            return true;
        }
        match code {
            KeyCode::Tab | KeyCode::BackTab => return false,
            KeyCode::Char('j') | KeyCode::Down => {
                if self.selected_index + 1 < self.summaries.len() { self.selected_index += 1; self.state.select(Some(self.selected_index)); }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.selected_index > 0 { self.selected_index -= 1; self.state.select(Some(self.selected_index)); }
            }
            KeyCode::Char('t') => {
                self.range = match self.range.as_str() { "day" => "week", "week" => "month", _ => "day" }.into();
                self.summaries = self.mgr.usage_db().query_usage(&self.range).unwrap_or_default();
            }
            KeyCode::Char('/') => { self.is_searching = true; }
            _ => return false,
        }
        true
    }
}

impl UsageTab {
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

    fn render_summary_cards(&self, f: &mut Frame, area: Rect) {
        let cards = Layout::default().direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1,4); 4]).split(area);

        // Summary cards show selected profile's data
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
            let p = Paragraph::new(lines)
                .block(Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                    .border_style(Style::default().fg(Theme::DIM)));
            f.render_widget(p, cards[i]);
        }
    }

    fn render_profile_list(&mut self, f: &mut Frame, area: Rect) {
        let max = self.max_tokens();
        let items: Vec<ListItem> = self.summaries.iter()
            .filter(|s| Self::token_total(s) > 0)
            .enumerate().map(|(i, s)| {
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
                Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(bar_text, Style::default().fg(Theme::PURPLE)),
                ]),
                Line::from(""),
            ])
        }).collect();

        let list = List::new(items)
            .block(Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                .title(format!("Models — \u{3a3} {}", format_tokens(self.total_tokens())))
                .border_style(Style::default().fg(Theme::DIM)))
            .highlight_style(Style::default());
        f.render_stateful_widget(list, area, &mut self.state);
    }

    fn render_daily_chart(&self, f: &mut Frame, area: Rect) {
        if let Some(s) = self.summaries.get(self.selected_index) {
            let label = title_case(&s.model);
            let daily = self.mgr.usage_db().query_daily_usage(&s.model).unwrap_or_default();
            let today_date = chrono::Local::now().format("%Y-%m-%d").to_string();

            // Build days with actual usage data (skip zero-token days, max 7)
            let days: Vec<(String, i64, i64, i64, i64, bool)> = (0..7).rev().filter_map(|offset| {
                let d = chrono::Local::now() - chrono::Duration::days(offset);
                let date_str = d.format("%Y-%m-%d").to_string();
                let (in_tok, out_tok, cr_tok, cc_tok) = daily.iter()
                    .find(|(dt, _, _, _, _)| dt == &date_str)
                    .map(|(_, i, o, cr, cc)| (*i, *o, *cr, *cc))
                    .unwrap_or((0, 0, 0, 0));
                let total = in_tok + out_tok + cr_tok + cc_tok;
                if total == 0 { None } else {
                    Some((d.format("%m-%d").to_string(), in_tok, out_tok, cr_tok, cc_tok, date_str == today_date))
                }
            }).collect();

            let max_val = days.iter().map(|(_, i, o, cr, cc, _)| i + o + cr + cc).max().unwrap_or(1).max(1);
            let lines: Vec<Line> = days.iter().flat_map(|(date, in_tok, out_tok, cr_tok, cc_tok, is_today)| {
                let total = in_tok + out_tok + cr_tok + cc_tok;
                let w = if max_val > 0 { (total as f64 / max_val as f64 * 30.0) as usize } else { 0 };
                let bar = "\u{2500}".repeat(w.min(35));
                let color = if *is_today { Theme::CYAN } else { Theme::PURPLE };
                let indent = "       ";
                let metric = |label: &str, val: &str| -> Span {
                    Span::styled(format!("{}{}  ", label, val), Style::default().fg(Theme::COMMENT))
                };
                let detail_lines: Vec<Line> = if total > 0 {
                    vec![
                        Line::from(vec![
                            Span::styled(indent, Style::default()),
                            metric("input ", &format_tokens(*in_tok)),
                            metric("output ", &format_tokens(*out_tok)),
                        ]),
                        Line::from(vec![
                            Span::styled(indent, Style::default()),
                            metric("cache read ", &format_tokens(*cr_tok)),
                            metric("cache create ", &format_tokens(*cc_tok)),
                        ]),
                    ]
                } else { vec![] };
                let mut day_lines = vec![
                    Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(format!("{}  ", date), Style::default().fg(Theme::COMMENT)),
                        Span::styled(bar, Style::default().fg(color)),
                        Span::styled(format!(" {}", format_tokens(total)), Style::default().fg(if *is_today { Theme::CYAN } else { Theme::DIM })),
                    ]),
                ];
                day_lines.extend(detail_lines);
                day_lines
            }).collect();

            let p = Paragraph::new(lines)
                .block(Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                    .title(format!("{} — This Week", label))
                    .border_style(Style::default().fg(Theme::DIM)));
            f.render_widget(p, area);
        }
    }
}

fn title_case(s: &str) -> String {
    let mut result = String::new();
    let mut upper = true;
    for c in s.chars() {
        if c == '-' || c == '.' || c == '_' { upper = true; result.push(c); }
        else if upper { result.push(c.to_ascii_uppercase()); upper = false; }
        else { result.push(c); }
    }
    result
}

fn format_tokens(n: i64) -> String {
    if n >= 1_000_000 { format!("{:.1}M", n as f64 / 1_000_000.0) }
    else if n >= 1_000 { format!("{:.1}k", n as f64 / 1_000.0) }
    else { n.to_string() }
}
