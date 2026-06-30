pub mod providers;

use ratatui::Frame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Providers,
    Usage,
    History,
}

pub trait TabContent {
    fn render(&mut self, f: &mut Frame, area: ratatui::layout::Rect);
    fn handle_key(&mut self, code: ratatui::crossterm::event::KeyCode);
}
