use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::BorderType;

use crate::config::ConsoleThemeName;

use super::view_models::{BodyLineKind, BodySpanRole};

#[derive(Debug, Clone, Copy)]
struct ThemePalette {
    fg_base: Color,
    fg_muted: Color,
    fg_emphasis: Color,
    fg_accent: Color,
    fg_success: Color,
    fg_warning: Color,
    fg_error: Color,
    bg_base: Color,
    bg_surface: Color,
    bg_overlay: Color,
}

#[derive(Debug, Clone, Copy)]
struct ThemeChrome {
    focused_border: BorderType,
    unfocused_border: BorderType,
    focused_border_modifier: Modifier,
    title_modifier: Modifier,
    selection_modifier: Modifier,
}

#[derive(Debug, Clone, Copy)]
pub struct ConsoleTheme {
    palette: ThemePalette,
    chrome: ThemeChrome,
    no_color: bool,
}

impl ConsoleTheme {
    pub fn from_name(name: ConsoleThemeName) -> Self {
        Self::resolve(name, std::env::var_os("NO_COLOR").is_some())
    }

    pub fn resolve(name: ConsoleThemeName, no_color: bool) -> Self {
        let (palette, chrome) = match name {
            ConsoleThemeName::GoldBand => (
                ThemePalette {
                    fg_base: Color::Gray,
                    fg_muted: Color::DarkGray,
                    fg_emphasis: Color::White,
                    fg_accent: Color::LightYellow,
                    fg_success: Color::Green,
                    fg_warning: Color::Yellow,
                    fg_error: Color::LightRed,
                    bg_base: Color::Black,
                    bg_surface: Color::Rgb(18, 18, 18),
                    bg_overlay: Color::Rgb(28, 28, 28),
                },
                ThemeChrome {
                    focused_border: BorderType::Double,
                    unfocused_border: BorderType::Plain,
                    focused_border_modifier: Modifier::BOLD,
                    title_modifier: Modifier::BOLD,
                    selection_modifier: Modifier::BOLD,
                },
            ),
            ConsoleThemeName::Nord => (
                ThemePalette {
                    fg_base: Color::Rgb(216, 222, 233),
                    fg_muted: Color::Rgb(143, 188, 187),
                    fg_emphasis: Color::Rgb(236, 239, 244),
                    fg_accent: Color::Rgb(136, 192, 208),
                    fg_success: Color::Rgb(163, 190, 140),
                    fg_warning: Color::Rgb(235, 203, 139),
                    fg_error: Color::Rgb(191, 97, 106),
                    bg_base: Color::Rgb(46, 52, 64),
                    bg_surface: Color::Rgb(59, 66, 82),
                    bg_overlay: Color::Rgb(76, 86, 106),
                },
                ThemeChrome {
                    focused_border: BorderType::Double,
                    unfocused_border: BorderType::Plain,
                    focused_border_modifier: Modifier::BOLD,
                    title_modifier: Modifier::BOLD,
                    selection_modifier: Modifier::BOLD,
                },
            ),
            ConsoleThemeName::Dracula => (
                ThemePalette {
                    fg_base: Color::Rgb(248, 248, 242),
                    fg_muted: Color::Rgb(139, 233, 253),
                    fg_emphasis: Color::Rgb(255, 255, 255),
                    fg_accent: Color::Rgb(255, 184, 108),
                    fg_success: Color::Rgb(80, 250, 123),
                    fg_warning: Color::Rgb(241, 250, 140),
                    fg_error: Color::Rgb(255, 85, 85),
                    bg_base: Color::Rgb(40, 42, 54),
                    bg_surface: Color::Rgb(48, 52, 70),
                    bg_overlay: Color::Rgb(68, 71, 90),
                },
                ThemeChrome {
                    focused_border: BorderType::Double,
                    unfocused_border: BorderType::Plain,
                    focused_border_modifier: Modifier::BOLD,
                    title_modifier: Modifier::BOLD,
                    selection_modifier: Modifier::BOLD,
                },
            ),
            ConsoleThemeName::Cyber => (
                ThemePalette {
                    fg_base: Color::Rgb(215, 245, 255),
                    fg_muted: Color::Rgb(106, 165, 196),
                    fg_emphasis: Color::Rgb(242, 252, 255),
                    fg_accent: Color::Rgb(0, 240, 255),
                    fg_success: Color::Rgb(72, 255, 163),
                    fg_warning: Color::Rgb(255, 209, 102),
                    fg_error: Color::Rgb(255, 92, 138),
                    bg_base: Color::Rgb(7, 11, 20),
                    bg_surface: Color::Rgb(13, 20, 34),
                    bg_overlay: Color::Rgb(20, 30, 52),
                },
                ThemeChrome {
                    focused_border: BorderType::Double,
                    unfocused_border: BorderType::Plain,
                    focused_border_modifier: Modifier::BOLD,
                    title_modifier: Modifier::BOLD,
                    selection_modifier: Modifier::BOLD,
                },
            ),
            ConsoleThemeName::Onyx => (
                ThemePalette {
                    fg_base: Color::Rgb(231, 226, 214),
                    fg_muted: Color::Rgb(164, 151, 126),
                    fg_emphasis: Color::Rgb(255, 249, 237),
                    fg_accent: Color::Rgb(232, 196, 120),
                    fg_success: Color::Rgb(126, 196, 158),
                    fg_warning: Color::Rgb(240, 188, 96),
                    fg_error: Color::Rgb(216, 106, 106),
                    bg_base: Color::Rgb(14, 14, 16),
                    bg_surface: Color::Rgb(24, 24, 27),
                    bg_overlay: Color::Rgb(34, 34, 38),
                },
                ThemeChrome {
                    focused_border: BorderType::Double,
                    unfocused_border: BorderType::Plain,
                    focused_border_modifier: Modifier::BOLD,
                    title_modifier: Modifier::BOLD,
                    selection_modifier: Modifier::BOLD,
                },
            ),
            ConsoleThemeName::Mist => (
                ThemePalette {
                    fg_base: Color::Rgb(221, 226, 235),
                    fg_muted: Color::Rgb(146, 160, 181),
                    fg_emphasis: Color::Rgb(242, 245, 250),
                    fg_accent: Color::Rgb(163, 190, 230),
                    fg_success: Color::Rgb(164, 214, 188),
                    fg_warning: Color::Rgb(232, 204, 154),
                    fg_error: Color::Rgb(214, 139, 156),
                    bg_base: Color::Rgb(41, 46, 56),
                    bg_surface: Color::Rgb(53, 59, 71),
                    bg_overlay: Color::Rgb(67, 74, 88),
                },
                ThemeChrome {
                    focused_border: BorderType::Rounded,
                    unfocused_border: BorderType::Plain,
                    focused_border_modifier: Modifier::BOLD,
                    title_modifier: Modifier::BOLD,
                    selection_modifier: Modifier::BOLD,
                },
            ),
            ConsoleThemeName::HighContrast => (
                ThemePalette {
                    fg_base: Color::White,
                    fg_muted: Color::Gray,
                    fg_emphasis: Color::Rgb(255, 255, 255),
                    fg_accent: Color::Yellow,
                    fg_success: Color::LightGreen,
                    fg_warning: Color::LightYellow,
                    fg_error: Color::LightRed,
                    bg_base: Color::Black,
                    bg_surface: Color::Rgb(8, 8, 8),
                    bg_overlay: Color::Rgb(18, 18, 18),
                },
                ThemeChrome {
                    focused_border: BorderType::Thick,
                    unfocused_border: BorderType::Plain,
                    focused_border_modifier: Modifier::BOLD,
                    title_modifier: Modifier::BOLD,
                    selection_modifier: Modifier::BOLD,
                },
            ),
        };
        Self {
            palette,
            chrome,
            no_color,
        }
    }

    fn color(self, color: Color) -> Color {
        if self.no_color { Color::Reset } else { color }
    }

    pub fn header_style(self) -> Style {
        Style::default().fg(self.color(self.palette.fg_accent)).bg(self.color(self.palette.bg_surface))
    }

    pub fn body_style(self) -> Style {
        Style::default().fg(self.color(self.palette.fg_base)).bg(self.color(self.palette.bg_base))
    }

    pub fn detail_style(self) -> Style {
        Style::default().fg(self.color(self.palette.fg_emphasis)).bg(self.color(self.palette.bg_base))
    }

    pub fn input_placeholder_style(self) -> Style {
        Style::default().fg(self.color(self.palette.fg_muted)).bg(self.color(self.palette.bg_surface))
    }

    pub fn input_value_style(self) -> Style {
        Style::default().fg(self.color(self.palette.fg_accent)).bg(self.color(self.palette.bg_surface))
    }

    pub fn footer_style(self) -> Style {
        Style::default().fg(self.color(self.palette.fg_muted)).bg(self.color(self.palette.bg_surface))
    }

    pub fn overlay_style(self) -> Style {
        Style::default().fg(self.color(self.palette.fg_emphasis)).bg(self.color(self.palette.bg_overlay))
    }

    pub fn focused_border_style(self) -> Style {
        Style::default().fg(self.color(self.palette.fg_accent)).add_modifier(self.chrome.focused_border_modifier)
    }

    pub fn unfocused_border_style(self) -> Style {
        Style::default().fg(self.color(self.palette.fg_muted))
    }

    pub fn focused_border_type(self) -> BorderType {
        self.chrome.focused_border
    }

    pub fn unfocused_border_type(self) -> BorderType {
        self.chrome.unfocused_border
    }

    pub fn title_style(self) -> Style {
        Style::default().fg(self.color(self.palette.fg_accent)).add_modifier(self.chrome.title_modifier)
    }

    pub fn line_style(self, kind: BodyLineKind) -> Style {
        match kind {
            BodyLineKind::Normal => self.body_style(),
            BodyLineKind::Muted => Style::default().fg(self.color(self.palette.fg_muted)).bg(self.color(self.palette.bg_base)),
            BodyLineKind::Success => Style::default().fg(self.color(self.palette.fg_success)).bg(self.color(self.palette.bg_base)),
            BodyLineKind::Warning => Style::default().fg(self.color(self.palette.fg_warning)).bg(self.color(self.palette.bg_base)),
            BodyLineKind::Error => Style::default().fg(self.color(self.palette.fg_error)).bg(self.color(self.palette.bg_base)).add_modifier(Modifier::BOLD),
        }
    }

    pub fn span_style(self, role: BodySpanRole) -> Style {
        match role {
            BodySpanRole::Normal => self.body_style(),
            BodySpanRole::Muted => Style::default().fg(self.color(self.palette.fg_muted)).bg(self.color(self.palette.bg_base)),
            BodySpanRole::Accent => Style::default().fg(self.color(self.palette.fg_accent)).bg(self.color(self.palette.bg_base)).add_modifier(self.chrome.selection_modifier),
            BodySpanRole::Success => Style::default().fg(self.color(self.palette.fg_success)).bg(self.color(self.palette.bg_base)).add_modifier(Modifier::BOLD),
            BodySpanRole::Warning => Style::default().fg(self.color(self.palette.fg_warning)).bg(self.color(self.palette.bg_base)),
            BodySpanRole::Error => Style::default().fg(self.color(self.palette.fg_error)).bg(self.color(self.palette.bg_base)).add_modifier(Modifier::BOLD),
            BodySpanRole::PickerBorder => Style::default().fg(self.color(self.palette.fg_muted)).bg(self.color(self.palette.bg_base)),
            BodySpanRole::PickerTitle => Style::default().fg(self.color(self.palette.fg_emphasis)).bg(self.color(self.palette.bg_base)).add_modifier(Modifier::BOLD),
            BodySpanRole::PickerSelection => Style::default().fg(self.color(self.palette.fg_accent)).bg(self.color(self.palette.bg_base)).add_modifier(self.chrome.selection_modifier),
            BodySpanRole::PickerMeta => Style::default().fg(self.color(self.palette.fg_muted)).bg(self.color(self.palette.bg_base)),
            BodySpanRole::PickerReasonLabel => Style::default().fg(self.color(self.palette.fg_warning)).bg(self.color(self.palette.bg_base)).add_modifier(Modifier::BOLD),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ConsoleTheme;
    use crate::config::ConsoleThemeName;
    use ratatui::style::{Color, Modifier};

    #[test]
    fn resolves_named_themes_to_distinct_accent_colors() {
        let gold_band = ConsoleTheme::resolve(ConsoleThemeName::GoldBand, false).header_style();
        let nord = ConsoleTheme::resolve(ConsoleThemeName::Nord, false).header_style();
        let dracula = ConsoleTheme::resolve(ConsoleThemeName::Dracula, false).header_style();
        let cyber = ConsoleTheme::resolve(ConsoleThemeName::Cyber, false).header_style();
        let onyx = ConsoleTheme::resolve(ConsoleThemeName::Onyx, false).header_style();
        let mist = ConsoleTheme::resolve(ConsoleThemeName::Mist, false).header_style();
        let high_contrast = ConsoleTheme::resolve(ConsoleThemeName::HighContrast, false).header_style();

        assert_eq!(gold_band.fg, Some(Color::LightYellow));
        assert_eq!(nord.fg, Some(Color::Rgb(136, 192, 208)));
        assert_eq!(dracula.fg, Some(Color::Rgb(255, 184, 108)));
        assert_eq!(cyber.fg, Some(Color::Rgb(0, 240, 255)));
        assert_eq!(onyx.fg, Some(Color::Rgb(232, 196, 120)));
        assert_eq!(mist.fg, Some(Color::Rgb(163, 190, 230)));
        assert_eq!(high_contrast.fg, Some(Color::Yellow));
    }

    #[test]
    fn no_color_keeps_typography_but_resets_colors() {
        let style = ConsoleTheme::resolve(ConsoleThemeName::Nord, true).focused_border_style();
        assert_eq!(style.fg, Some(Color::Reset));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }
}
