use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    Frame,
};
use crossterm::event::KeyCode;
use crate::core::config::ConfigManager;
use crate::core::models::Provider;
use super::super::widgets::detail_panel::DetailPanel;
use super::super::widgets::provider_tree::{ProviderTree, TreeItem};
use super::super::widgets::status_bar::render_status_bar;
use super::TabContent;

pub struct ProvidersTab {
    tree: ProviderTree,
    providers: Vec<Provider>,
    active_provider: String,
    active_profile: String,
    pub proxy_running: bool,
    pub proxy_port: u16,
}

impl ProvidersTab {
    pub fn new(mgr: &ConfigManager) -> Self {
        let providers = mgr.list_providers().unwrap_or_default();
        let active_provider = mgr
            .db()
            .get_setting("active_provider")
            .unwrap_or_default();
        let active_profile = mgr
            .db()
            .get_setting("active_profile")
            .unwrap_or_default();
        let proxy_running = mgr
            .db()
            .get_setting("proxy_mode")
            .map(|v| v == "true")
            .unwrap_or(false);
        let proxy_port = mgr
            .db()
            .get_setting("proxy_port")
            .and_then(|s| s.parse().ok())
            .unwrap_or(15721);

        let tree = ProviderTree::new(
            &providers,
            (!active_provider.is_empty()).then_some(active_provider.as_str()),
            (!active_profile.is_empty()).then_some(active_profile.as_str()),
        );

        ProvidersTab {
            tree,
            providers,
            active_provider,
            active_profile,
            proxy_running,
            proxy_port,
        }
    }

    /// Find a profile given a TreeItem::Profile reference, returning the
    /// (Provider, Profile) pair.
    fn find_selected_profile(&self, item: &TreeItem) -> Option<(&Provider, &crate::core::models::Profile)> {
        match item {
            TreeItem::Profile {
                provider_id,
                profile_index,
                ..
            } => {
                let provider = self.providers.iter().find(|p| p.id == *provider_id)?;
                let profile = provider.profiles.get(*profile_index)?;
                Some((provider, profile))
            }
            _ => None,
        }
    }

    fn is_active_selection(&self, item: &TreeItem) -> bool {
        match item {
            TreeItem::Profile {
                provider_id,
                profile_index,
                ..
            } => {
                if let Some(provider) = self.providers.iter().find(|p| p.id == *provider_id) {
                    if let Some(profile) = provider.profiles.get(*profile_index) {
                        return self.active_provider == provider.id
                            && self.active_profile == profile.id;
                    }
                }
                false
            }
            _ => false,
        }
    }
}

impl TabContent for ProvidersTab {
    fn render(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(area);

        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(chunks[0]);

        self.tree.render(f, main[0]);

        // Always render detail panel (with or without selection)
        if let Some(item) = self.tree.selected_item() {
            if let Some((provider, profile)) = self.find_selected_profile(item) {
                let is_active = self.is_active_selection(item);
                DetailPanel::render_profile_detail(
                    f,
                    main[1],
                    &provider.name,
                    profile,
                    &provider.api_url,
                    &provider.api_key,
                    is_active,
                    provider.source.can_delete(),
                );
            } else {
                DetailPanel::render_empty(f, main[1], "Select a profile");
            }
        } else {
            DetailPanel::render_empty(f, main[1], "Select a profile");
        }

        let active_provider = if self.active_provider.is_empty() {
            "?"
        } else {
            &self.active_provider
        };
        let active_profile = if self.active_profile.is_empty() {
            "?"
        } else {
            &self.active_profile
        };

        render_status_bar(
            f,
            chunks[1],
            active_provider,
            active_profile,
            self.proxy_running,
            self.proxy_port,
        );
    }

    fn handle_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char('j') | KeyCode::Down => self.tree.move_down(),
            KeyCode::Char('k') | KeyCode::Up => self.tree.move_up(),
            KeyCode::Char('h') | KeyCode::Left => {
                // Collapse provider
                let id = self
                    .tree
                    .items
                    .get(self.tree.selected_index().unwrap_or(0))
                    .and_then(|item| {
                        if let TreeItem::Provider { provider, .. } = item {
                            Some(provider.id.clone())
                        } else {
                            None
                        }
                    });
                if let Some(id) = id {
                    self.tree.toggle_collapse(&id);
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                // Expand provider
                let id = self
                    .tree
                    .items
                    .get(self.tree.selected_index().unwrap_or(0))
                    .and_then(|item| {
                        if let TreeItem::Provider { provider, .. } = item {
                            Some(provider.id.clone())
                        } else {
                            None
                        }
                    });
                if let Some(id) = id {
                    // Only expand if collapsed
                    if self.tree.collapsed.contains(&id) {
                        self.tree.toggle_collapse(&id);
                    }
                }
            }
            _ => {}
        }

        true
    }
}
