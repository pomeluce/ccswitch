use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem},
    Frame,
};
use crossterm::event::KeyCode;
use crate::core::config::ConfigManager;
use crate::db::usage::UsageSummary;
use super::super::widgets::bar_chart::render_bar_chart;
use super::super::theme::Theme;
use super::TabContent;

pub struct UsageTab {
    pub summaries: Vec<UsageSummary>,
    pub selected_index: usize,
    pub range: String,
}

impl UsageTab {
    pub fn new(mgr: &ConfigManager) -> Self {
        let summaries = mgr.db().query_usage("week").unwrap_or_default();
        UsageTab {
            summaries,
            selected_index: 0,
            range: "week".into(),
        }
    }

    #[allow(dead_code)]
    pub fn refresh(&mut self, mgr: &ConfigManager) {
        self.summaries = mgr.db().query_usage(&self.range).unwrap_or_default();
    }
}

impl TabContent for UsageTab {
    fn render(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(area);

        // Left: summary list
        let items: Vec<ListItem> = self
            .summaries
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let total = s.total_prompt + s.total_completion;
                let label = format!("{} / {}", s.provider_id, s.profile_id);
                let line = Line::from(vec![
                    Span::styled(
                        format!("{}  ", label),
                        Style::default().fg(Theme::CYAN),
                    ),
                    Span::styled(
                        format!("{}", total),
                        Style::default().fg(Theme::GREEN),
                    ),
                    Span::styled(
                        format!("  ({} reqs)", s.request_count),
                        Style::default().fg(Theme::PURPLE),
                    ),
                ]);
                if i == self.selected_index {
                    ListItem::new(line).style(Style::default().add_modifier(Modifier::REVERSED))
                } else {
                    ListItem::new(line)
                }
            })
            .collect();

        let total_tokens: i64 = self
            .summaries
            .iter()
            .map(|s| s.total_prompt + s.total_completion)
            .sum();
        let list = List::new(items)
            .block(
                Block::bordered().border_set(ratatui::symbols::border::ROUNDED)
                    .title(format!("Usage ({}) — Σ {}", self.range, total_tokens))
                    .border_style(Style::default().fg(Theme::DIM)),
            );

        f.render_widget(list, chunks[0]);

        // Right: bar chart for selected profile
        if let Some(s) = self.summaries.get(self.selected_index) {
            let label = format!("{} / {}", s.provider_id, s.profile_id);
            // Mock daily data — in real impl, query per-day breakdown
            let chart_data: Vec<(String, i64, bool)> = vec![
                ("Mon".into(), s.total_prompt / 7 + 100, false),
                ("Tue".into(), s.total_prompt / 7 + 200, false),
                ("Wed".into(), s.total_prompt / 7 + 500, true),
                ("Thu".into(), s.total_prompt / 7 + 50, false),
                ("Fri".into(), s.total_prompt / 7, false),
                ("Sat".into(), s.total_prompt / 14, false),
                ("Sun".into(), s.total_prompt / 10, false),
            ];
            render_bar_chart(f, chunks[1], &chart_data, &format!("{} — This Week", label));
        }
    }

    fn handle_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('j') | KeyCode::Down
                if self.selected_index < self.summaries.len().saturating_sub(1) =>
            {
                self.selected_index += 1;
            }
            KeyCode::Char('k') | KeyCode::Up
                if self.selected_index > 0 =>
            {
                self.selected_index -= 1;
            }
            KeyCode::Char('t') => {
                // Toggle range
                self.range = match self.range.as_str() {
                    "day" => "week".into(),
                    "week" => "month".into(),
                    _ => "day".into(),
                };
            }
            _ => {}
        }
    }
}
