use ratatui::style::{Color, Modifier, Style};

/// Copper & Cobalt color scheme derived from a mechanical control panel reference.
#[allow(dead_code)]
pub struct Theme {
    /// Aged Copper base — root background
    pub bg: Color,
    /// Light Copper/Bronze — primary text
    pub fg: Color,
    /// Cobalt Blue — labels, titles, tabs, assistant blocks
    pub accent: Color,
    /// Deep Cobalt — highlight backgrounds
    pub selected_bg: Color,
    /// Paler Bronze — selection markers
    pub selected_fg: Color,
    /// Dimmed Copper — hints, inactive text, borders
    pub dim_fg: Color,
    /// Verdigris/Green patina — success, user blocks, running status
    pub success: Color,
    /// Cobalt (same as accent) — info indicators
    pub info: Color,
    /// Oxidized Copper Red — errors, failures
    pub error: Color,
    /// Warm Amber — pending states, attention indicators
    pub warning: Color,
    /// Burnished Bronze — command mode
    pub command: Color,
    /// Bright Golden Amber — active status signals (Waiting, AUTO badge)
    pub alert: Color,
}

#[allow(dead_code)]
impl Theme {
    pub fn copper_cobalt() -> Self {
        Self {
            bg: Color::Rgb(36, 32, 29),
            fg: Color::Rgb(212, 207, 198),
            accent: Color::Rgb(27, 79, 129),
            selected_bg: Color::Rgb(24, 65, 119),
            selected_fg: Color::Rgb(230, 225, 217),
            dim_fg: Color::Rgb(166, 146, 124),
            success: Color::Rgb(166, 189, 169),
            info: Color::Rgb(27, 79, 129),
            error: Color::Rgb(94, 58, 49),
            warning: Color::Rgb(183, 135, 78),
            command: Color::Rgb(147, 112, 79),
            alert: Color::Rgb(215, 165, 55),
        }
    }

    pub fn copper_cobalt_boosted() -> Self {
        Self {
            bg: Color::Rgb(40, 34, 30),
            fg: Color::Rgb(245, 232, 214),
            accent: Color::Rgb(51, 133, 224),
            selected_bg: Color::Rgb(0, 51, 102),
            selected_fg: Color::Rgb(249, 240, 225),
            dim_fg: Color::Rgb(176, 159, 140),
            success: Color::Rgb(170, 214, 186),
            info: Color::Rgb(51, 133, 224),
            error: Color::Rgb(127, 79, 66),
            warning: Color::Rgb(210, 160, 80),
            command: Color::Rgb(180, 140, 90),
            alert: Color::Rgb(240, 190, 60),
        }
    }

    pub fn terminal() -> Self {
        Self {
            bg: Color::Reset,
            fg: Color::White,
            accent: Color::Cyan,
            selected_bg: Color::DarkGray,
            selected_fg: Color::White,
            dim_fg: Color::DarkGray,
            success: Color::Green,
            info: Color::Cyan,
            error: Color::Red,
            warning: Color::Yellow,
            command: Color::Magenta,
            alert: Color::Yellow,
        }
    }

    pub fn by_name(name: &str) -> Self {
        match name {
            "copper-cobalt-boosted" => Self::copper_cobalt_boosted(),
            "terminal" => Self::terminal(),
            _ => Self::copper_cobalt(),
        }
    }

    pub fn theme_names() -> &'static [&'static str] {
        &["copper-cobalt-dimmed", "copper-cobalt-boosted", "terminal"]
    }

    pub fn theme_labels() -> &'static [&'static str] {
        &["Copper Cobalt Dimmed", "Copper Cobalt Boosted", "Terminal"]
    }

    pub fn root(&self) -> Style {
        Style::default().fg(self.fg).bg(self.bg)
    }

    pub fn text(&self) -> Style {
        Style::default().fg(self.fg)
    }

    pub fn text_dim(&self) -> Style {
        Style::default().fg(self.dim_fg)
    }

    pub fn highlight(&self) -> Style {
        Style::default().fg(self.selected_fg).bg(self.selected_bg)
    }

    pub fn success_style(&self) -> Style {
        Style::default().fg(self.success)
    }

    pub fn error_style(&self) -> Style {
        Style::default().fg(self.error)
    }

    pub fn warning_style(&self) -> Style {
        Style::default().fg(self.warning)
    }

    pub fn accent_bold(&self) -> Style {
        Style::default().fg(self.accent).add_modifier(Modifier::BOLD)
    }

    pub fn success_bold(&self) -> Style {
        Style::default().fg(self.success).add_modifier(Modifier::BOLD)
    }

    pub fn error_bold(&self) -> Style {
        Style::default().fg(self.error).add_modifier(Modifier::BOLD)
    }

    pub fn warning_bold(&self) -> Style {
        Style::default().fg(self.warning).add_modifier(Modifier::BOLD)
    }

    pub fn selected_bold(&self) -> Style {
        Style::default().fg(self.selected_fg).add_modifier(Modifier::BOLD)
    }

    pub fn dim_italic(&self) -> Style {
        Style::default().fg(self.dim_fg).add_modifier(Modifier::ITALIC)
    }

    pub fn alert_bold(&self) -> Style {
        Style::default().fg(self.alert).add_modifier(Modifier::BOLD)
    }
}
