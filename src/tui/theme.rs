use ratatui::style::Color;

pub struct Theme;

impl Theme {
    pub const FG: Color           = Color::Rgb(0xFC, 0xFC, 0xFA);
    pub const RED: Color          = Color::Rgb(0xFF, 0x61, 0x88);
    #[allow(dead_code)]
    pub const ORANGE: Color       = Color::Rgb(0xFC, 0x98, 0x67);
    pub const YELLOW: Color       = Color::Rgb(0xFF, 0xD8, 0x66);
    pub const GREEN: Color        = Color::Rgb(0xA9, 0xDC, 0x76);
    pub const CYAN: Color         = Color::Rgb(0x78, 0xDC, 0xE8);
    pub const PURPLE: Color       = Color::Rgb(0xAB, 0x9D, 0xF2);
    pub const DIM: Color          = Color::Rgb(0x93, 0x92, 0x93);
    pub const COMMENT: Color      = Color::Rgb(0x72, 0x70, 0x72);
}
