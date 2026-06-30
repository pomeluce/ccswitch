use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState},
    Frame,
};
use crate::core::models::Provider;
use super::super::theme::Theme;

pub struct ProviderTree {
    pub items: Vec<TreeItem>,
    pub state: ListState,
    pub collapsed: std::collections::HashSet<String>,
}

#[derive(Debug, Clone)]
pub enum TreeItem {
    Provider {
        provider: Provider,
        #[allow(dead_code)]
        index: usize,
    },
    Profile {
        provider_id: String,
        profile_index: usize,
        profile_name: String,
        is_active: bool,
        is_default: bool,
    },
}

impl ProviderTree {
    pub fn new(
        providers: &[Provider],
        active_provider: Option<&str>,
        active_profile: Option<&str>,
    ) -> Self {
        let mut items = vec![];
        for (pi, p) in providers.iter().enumerate() {
            items.push(TreeItem::Provider {
                provider: p.clone(),
                index: pi,
            });
            for (pri, pr) in p.profiles.iter().enumerate() {
                let is_active =
                    active_provider == Some(&p.id) && active_profile == Some(&pr.id);
                items.push(TreeItem::Profile {
                    provider_id: p.id.clone(),
                    profile_index: pri,
                    profile_name: pr.name.clone(),
                    is_active,
                    is_default: pr.default,
                });
            }
        }
        let mut state = ListState::default();
        state.select(Some(0));
        ProviderTree {
            items,
            state,
            collapsed: std::collections::HashSet::new(),
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.state.selected()
    }

    pub fn selected_item(&self) -> Option<&TreeItem> {
        self.state.selected().and_then(|i| self.items.get(i))
    }

    pub fn move_up(&mut self) {
        let i = self.state.selected().unwrap_or(0);
        if i > 0 {
            self.state.select(Some(i - 1));
        }
    }

    pub fn move_down(&mut self) {
        let i = self.state.selected().unwrap_or(0);
        let max = self.items.len().saturating_sub(1);
        if i < max {
            self.state.select(Some(i + 1));
        }
    }

    pub fn toggle_collapse(&mut self, provider_id: &str) {
        if self.collapsed.contains(provider_id) {
            self.collapsed.remove(provider_id);
        } else {
            self.collapsed.insert(provider_id.to_string());
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .items
            .iter()
            .filter_map(|item| match item {
                TreeItem::Provider { provider, .. } => {
                    let icon = if provider.source.can_delete() {
                        "\u{1f464}"
                    } else {
                        "\u{1f512}"
                    };
                    let expand = if self.collapsed.contains(&provider.id) {
                        "\u{25b8}"
                    } else {
                        "\u{25be}"
                    };
                    Some(ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{} {} {}", icon, expand, provider.name),
                            Style::default().fg(Theme::YELLOW),
                        ),
                        Span::styled(
                            format!("  [{}]", provider.id),
                            Style::default().fg(Theme::COMMENT),
                        ),
                    ])))
                }
                TreeItem::Profile {
                    provider_id,
                    profile_name,
                    is_active,
                    is_default,
                    ..
                } => {
                    if self.collapsed.contains(provider_id) {
                        return None;
                    }
                    let star = if *is_default { "\u{2605}" } else { " " };
                    let marker = if *is_active { "\u{25cf}" } else { " " };
                    let style = if *is_active {
                        Style::default().fg(Theme::CYAN)
                    } else {
                        Style::default().fg(Theme::FG)
                    };
                    Some(ListItem::new(Line::from(vec![Span::styled(
                        format!("    {} {} {}", star, marker, profile_name),
                        style,
                    )])))
                }
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::bordered()
                    .title("Providers")
                    .border_style(Style::default().fg(Theme::DIM)),
            )
            .highlight_style(Style::default().bg(Theme::BG_SELECTED))
            .style(Style::default().bg(Theme::BG_PANEL));
        f.render_stateful_widget(list, area, &mut self.state);
    }
}
