//! Linear & Attio inspired dark design system for Meridian Hub.
//!
//! Indigo-tinted dark palette (blue-violet shell + surfaces), Emerald/Blue/Amber
//! state colors, pill borders and crisp styling.

use iced::{
    border,
    font::{Family, Weight},
    widget::{button, container, text_input},
    Border, Color, Font, Shadow, Vector,
};

// Embedded Fonts. cosmic-text/fontdb resolve faces by TYPOGRAPHIC family name
// (name-table ID 16), not by file name: the three Geist weights all register
// family "Geist" (picked apart by weight), and GeistMono registers as
// "Geist Mono". The previous "Geist-Regular"-style lookups matched no
// registered family and silently fell back to the system sans-serif.
pub const FONT_REGULAR: Font = Font {
    family: Family::Name("Geist"),
    weight: Weight::Normal,
    ..Font::DEFAULT
};
pub const FONT_MEDIUM: Font = Font {
    family: Family::Name("Geist"),
    weight: Weight::Medium,
    ..Font::DEFAULT
};
pub const FONT_SEMIBOLD: Font = Font {
    family: Family::Name("Geist"),
    weight: Weight::Semibold,
    ..Font::DEFAULT
};
pub const FONT_MONO: Font = Font::with_name("Geist Mono");
/// Chip-only mono variant with tightened tracking (~-5%) — the embedded
/// GeistMono face patched at build time (see `ui::fonts`). Registered as a
/// separate family so plain mono text (logs, UDIDs, endpoints) is untouched.
pub const FONT_MONO_TIGHT: Font = Font::with_name("Geist Mono Tight");

// Brand & State Colors — the whole base family shares the same blue-violet cast
// (dominant blue channel), so cards/pills/inputs sit coherently on the window.
pub const BG_OBSIDIAN: Color = Color::from_rgb8(0x0B, 0x0B, 0x13);       // #0B0B13 — deep press state + modal card
pub const BG_WINDOW: Color = Color::from_rgb8(0x16, 0x13, 0x1E);         // #16131E
pub const BG_CONTENT: Color = Color::from_rgba8(0x0D, 0x0A, 0x13, 0.9);      // #0D0A13 @ 90%
pub const BORDER_HAIRLINE: Color = Color::from_rgba8(0xF5, 0xF5, 0xF5, 0.15); // #F5F5F5 @ 15%
pub const BG_SURFACE: Color = Color::from_rgb8(0x0F, 0x0F, 0x18);        // #0F0F18 — card surface (was #141519)
pub const BG_SURFACE_HOVER: Color = Color::from_rgb8(0x17, 0x17, 0x26);  // #171726 (was #1C1E23)
pub const BORDER_SUBTLE: Color = Color::from_rgb8(0x26, 0x26, 0x39);     // #262639 (was #22232B)
pub const BORDER_FOCUS: Color = Color::from_rgb(0.231, 0.51, 0.965);      // #3B82F6

pub const TEXT_PRIMARY: Color = Color::from_rgb(0.96, 0.96, 0.98);        // #F4F4F6
pub const TEXT_SECONDARY: Color = Color::from_rgb(0.55, 0.56, 0.62);      // #8C8F9E
pub const TEXT_MUTED: Color = Color::from_rgb(0.38, 0.39, 0.44);          // #616470

pub const ACCENT_EMERALD: Color = Color::from_rgb(0.063, 0.725, 0.506);    // #10B981
pub const ACCENT_BRAND: Color = Color::from_rgb8(0x32, 0x4B, 0xED);    // #324BED — Meridian brand blue (the logo mark)
pub const ACCENT_BLUE: Color = Color::from_rgb(0.231, 0.51, 0.965);       // #3B82F6
pub const ACCENT_ROSE: Color = Color::from_rgb(0.937, 0.267, 0.267);       // #EF4444
pub const ACCENT_AMBER: Color = Color::from_rgb(0.96, 0.62, 0.04);        // #F59E0B

// Container Styles
pub fn style_window_background(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(BG_WINDOW.into()),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: border::Radius::from(12.0),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
            offset: Vector::new(0.0, 6.0),
            blur_radius: 16.0,
        },
        text_color: Some(TEXT_PRIMARY),
        snap: false,
    }
}

/// Content surface: the panel the tab bar + tab bodies live on, nested
/// inside the window shell below the titlebar.
pub fn style_content_panel(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(BG_CONTENT.into()),
        border: Border {
            color: BORDER_HAIRLINE,
            width: 0.5,
            radius: border::Radius {
                top_left: 12.0,
                top_right: 12.0,
                bottom_right: 8.0,
                bottom_left: 8.0,
            },
        },
        shadow: Shadow::default(),
        text_color: Some(TEXT_PRIMARY),
        snap: false,
    }
}

pub fn style_card(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(BG_SURFACE.into()),
        border: Border {
            color: BORDER_SUBTLE,
            width: 0.5,
            radius: border::Radius::from(10.0),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.2),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 8.0,
        },
        text_color: Some(TEXT_PRIMARY),
        snap: false,
    }
}

pub fn style_pill_badge(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.05).into()),
        border: Border {
            color: BORDER_SUBTLE,
            width: 1.0,
            radius: border::Radius::from(9999.0),
        },
        text_color: Some(TEXT_SECONDARY),
        shadow: Shadow::default(),
        snap: false,
    }
}

// Button Styles (Full Pill)
pub fn style_button_primary(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let base_bg = match status {
        button::Status::Hovered => Color::from_rgb8(0x4B, 0x61, 0xEF),   // brand blue +12% white
        button::Status::Pressed => Color::from_rgb8(0x2B, 0x40, 0xC9),   // brand blue -15%
        _ => ACCENT_BRAND,
    };
    button::Style {
        background: Some(base_bg.into()),
        text_color: Color::from_rgb(0.96, 0.96, 0.98),
        border: Border {
            radius: border::Radius::from(9999.0),
            ..Default::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

pub fn style_button_danger(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let base_bg = match status {
        button::Status::Hovered => Color::from_rgb(0.98, 0.35, 0.35),
        button::Status::Pressed => Color::from_rgb(0.8, 0.2, 0.2),
        _ => ACCENT_ROSE,
    };
    button::Style {
        background: Some(base_bg.into()),
        text_color: Color::WHITE,
        border: Border {
            radius: border::Radius::from(9999.0),
            ..Default::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

pub fn style_button_secondary(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => BG_SURFACE_HOVER,
        button::Status::Pressed => BG_OBSIDIAN,
        _ => BG_SURFACE,
    };
    button::Style {
        background: Some(bg.into()),
        text_color: TEXT_PRIMARY,
        border: Border {
            color: BORDER_SUBTLE,
            width: 1.0,
            radius: border::Radius::from(9999.0),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

pub fn style_tab_button(_theme: &iced::Theme, status: button::Status, active: bool) -> button::Style {
    let (bg, text) = if active {
        (Color::from_rgba(1.0, 1.0, 1.0, 0.1), TEXT_PRIMARY)
    } else {
        match status {
            button::Status::Hovered => (Color::from_rgba(1.0, 1.0, 1.0, 0.05), TEXT_PRIMARY),
            _ => (Color::TRANSPARENT, TEXT_SECONDARY),
        }
    };

    button::Style {
        background: Some(bg.into()),
        text_color: text,
        border: Border {
            radius: border::Radius::from(9999.0),
            ..Default::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

// Window Control Buttons (Titlebar)
//
// Perfectly circular; the background appears only on hover/press — the idle
// state is fully transparent with no border, keeping the titlebar clean.
pub fn style_window_control_secondary(
    _theme: &iced::Theme,
    status: button::Status,
) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Hovered => (Some(BG_SURFACE_HOVER.into()), TEXT_PRIMARY),
        button::Status::Pressed => (Some(BG_SURFACE.into()), TEXT_PRIMARY),
        _ => (None, TEXT_SECONDARY),
    };

    button::Style {
        background,
        text_color,
        border: Border {
            radius: border::Radius::from(9999.0),
            ..Default::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

pub fn style_window_control_danger(
    _theme: &iced::Theme,
    status: button::Status,
) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Hovered => (Some(ACCENT_ROSE.into()), Color::WHITE),
        button::Status::Pressed => (Some(Color::from_rgb(0.8, 0.2, 0.2).into()), Color::WHITE),
        _ => (None, TEXT_SECONDARY),
    };

    button::Style {
        background,
        text_color,
        border: Border {
            radius: border::Radius::from(9999.0),
            ..Default::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

// Input Field Style
pub fn style_text_input(_theme: &iced::Theme, status: text_input::Status) -> text_input::Style {
    let border_color = match status {
        text_input::Status::Focused { .. } => BORDER_FOCUS,
        text_input::Status::Hovered => Color::from_rgb(0.2, 0.22, 0.28),
        _ => BORDER_SUBTLE,
    };

    text_input::Style {
        background: BG_SURFACE.into(),
        border: Border {
            color: border_color,
            width: 0.5,
            radius: border::Radius::from(8.0),
        },
        icon: TEXT_MUTED,
        placeholder: TEXT_MUTED,
        value: TEXT_PRIMARY,
        selection: Color::from_rgba(0.231, 0.51, 0.965, 0.3),
    }
}
