//! Linear & Attio inspired dark design system for Meridian Hub.
//!
//! Obsidian, Charcoal, Emerald, Electric Blue palette with pill borders and crisp styling.

use iced::{border, widget::{button, container, text_input}, Border, Color, Font, Shadow, Vector};

// Embedded Fonts
pub const FONT_REGULAR: Font = Font::with_name("Geist-Regular");
pub const FONT_MEDIUM: Font = Font::with_name("Geist-Medium");
pub const FONT_SEMIBOLD: Font = Font::with_name("Geist-SemiBold");
pub const FONT_MONO: Font = Font::with_name("GeistMono-Regular");

// Brand & State Colors
pub const BG_OBSIDIAN: Color = Color::from_rgb(0.043, 0.047, 0.055);       // #0B0C0E
pub const BG_SURFACE: Color = Color::from_rgb(0.078, 0.082, 0.098);        // #141519
pub const BG_SURFACE_HOVER: Color = Color::from_rgb(0.11, 0.118, 0.137);  // #1C1E23
pub const BORDER_SUBTLE: Color = Color::from_rgb(0.133, 0.137, 0.169);     // #22232B
pub const BORDER_FOCUS: Color = Color::from_rgb(0.231, 0.51, 0.965);      // #3B82F6

pub const TEXT_PRIMARY: Color = Color::from_rgb(0.96, 0.96, 0.98);        // #F4F4F6
pub const TEXT_SECONDARY: Color = Color::from_rgb(0.55, 0.56, 0.62);      // #8C8F9E
pub const TEXT_MUTED: Color = Color::from_rgb(0.38, 0.39, 0.44);          // #616470

pub const ACCENT_EMERALD: Color = Color::from_rgb(0.063, 0.725, 0.506);    // #10B981
pub const ACCENT_BLUE: Color = Color::from_rgb(0.231, 0.51, 0.965);       // #3B82F6
pub const ACCENT_ROSE: Color = Color::from_rgb(0.937, 0.267, 0.267);       // #EF4444
pub const ACCENT_AMBER: Color = Color::from_rgb(0.96, 0.62, 0.04);        // #F59E0B

// Container Styles
pub fn style_window_background(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(BG_OBSIDIAN.into()),
        border: Border {
            color: BORDER_SUBTLE,
            width: 1.0,
            radius: border::Radius::from(12.0),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.6),
            offset: Vector::new(0.0, 8.0),
            blur_radius: 24.0,
        },
        text_color: Some(TEXT_PRIMARY),
        snap: false,
    }
}

pub fn style_card(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(BG_SURFACE.into()),
        border: Border {
            color: BORDER_SUBTLE,
            width: 1.0,
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
        button::Status::Hovered => Color::from_rgb(0.1, 0.8, 0.58),
        button::Status::Pressed => Color::from_rgb(0.05, 0.65, 0.45),
        _ => ACCENT_EMERALD,
    };
    button::Style {
        background: Some(base_bg.into()),
        text_color: Color::from_rgb(0.02, 0.1, 0.06),
        border: Border {
            radius: border::Radius::from(9999.0),
            ..Default::default()
        },
        shadow: Shadow {
            color: Color::from_rgba(0.063, 0.725, 0.506, 0.25),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 8.0,
        },
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
        shadow: Shadow {
            color: Color::from_rgba(0.937, 0.267, 0.267, 0.2),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 6.0,
        },
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
            width: 1.0,
            radius: border::Radius::from(8.0),
        },
        icon: TEXT_MUTED,
        placeholder: TEXT_MUTED,
        value: TEXT_PRIMARY,
        selection: Color::from_rgba(0.231, 0.51, 0.965, 0.3),
    }
}
