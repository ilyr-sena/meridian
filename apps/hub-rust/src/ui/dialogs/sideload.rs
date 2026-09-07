//! Sideload Modal Dialog: clean UI with native file picker and progress indicator.

use std::path::PathBuf;
use iced::{
    alignment,
    widget::{button, column, container, progress_bar, row, text, text_input, Space},
    Alignment, Element, Length,
};
use crate::ui::theme::*;

#[derive(Debug, Clone)]
pub struct SideloadDialogState {
    pub is_open: bool,
    pub udid: String,
    pub ipa_path: Option<PathBuf>,
    pub apple_id: String,
    pub password: String,
    pub progress: f32,
    pub status_message: String,
    pub is_busy: bool,
}

pub fn view_sideload_modal<'a, Message>(
    state: &'a SideloadDialogState,
    on_browse_file: Message,
    on_apple_id_change: impl Fn(String) -> Message + 'a + Copy,
    on_password_change: impl Fn(String) -> Message + 'a + Copy,
    on_submit: Message,
    on_cancel: Message,
) -> Element<'a, Message>
where
    Message: 'a + Clone,
{
    if !state.is_open {
        return Space::new().into();
    }

    let ipa_label = state
        .ipa_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "MeridianRunner-unsigned.ipa".to_string());

    let file_row = row![
        container(
            text(ipa_label)
                .font(FONT_MONO)
                .size(12)
                .color(TEXT_PRIMARY)
        )
        .padding([8, 12])
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(BG_SURFACE.into()),
            border: iced::Border {
                color: BORDER_SUBTLE,
                width: 1.0,
                radius: iced::border::Radius::from(8.0),
            },
            ..Default::default()
        }),
        button(text("Browse...").font(FONT_MEDIUM).size(12))
            .padding([8, 14])
            .on_press(on_browse_file)
            .style(style_button_secondary),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let progress_section = if state.is_busy {
        column![
            progress_bar(0.0..=1.0, state.progress),
            Space::new().height(6),
            text(&state.status_message)
                .font(FONT_REGULAR)
                .size(11)
                .color(ACCENT_BLUE),
        ]
    } else {
        column![]
    };

    let actions = row![
        button(text("Cancel").font(FONT_MEDIUM).size(12))
            .padding([7, 16])
            .on_press(on_cancel)
            .style(style_button_secondary),
        Space::new().width(Length::Fill),
        button(text("Sign & Install").font(FONT_MEDIUM).size(12))
            .padding([7, 20])
            .on_press(on_submit)
            .style(style_button_primary),
    ]
    .align_y(Alignment::Center);

    let dialog_box = container(
        column![
            text("Sideload MeridianRunner")
                .font(FONT_SEMIBOLD)
                .size(16)
                .color(TEXT_PRIMARY),
            Space::new().height(4),
            text(format!("Target iPhone: {}", state.udid))
                .font(FONT_MONO)
                .size(11)
                .color(TEXT_MUTED),
            Space::new().height(16),
            text("App Package (.ipa):").font(FONT_REGULAR).size(12).color(TEXT_SECONDARY),
            Space::new().height(4),
            file_row,
            Space::new().height(12),
            text("Apple ID:").font(FONT_REGULAR).size(12).color(TEXT_SECONDARY),
            Space::new().height(4),
            text_input("developer@apple.com", &state.apple_id)
                .on_input(on_apple_id_change)
                .padding(8)
                .size(12)
                .style(style_text_input),
            Space::new().height(12),
            text("Password / App-Specific Password:").font(FONT_REGULAR).size(12).color(TEXT_SECONDARY),
            Space::new().height(4),
            text_input("••••••••••••", &state.password)
                .on_input(on_password_change)
                .padding(8)
                .size(12)
                .secure(true)
                .style(style_text_input),
            Space::new().height(14),
            progress_section,
            Space::new().height(16),
            actions,
        ]
        .padding(24)
        .width(440)
    )
    .style(|_| container::Style {
        background: Some(BG_OBSIDIAN.into()),
        border: iced::Border {
            color: BORDER_SUBTLE,
            width: 1.0,
            radius: iced::border::Radius::from(12.0),
        },
        shadow: iced::Shadow {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.8),
            offset: iced::Vector::new(0.0, 16.0),
            blur_radius: 36.0,
        },
        ..Default::default()
    });

    container(dialog_box)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(alignment::Horizontal::Center)
        .align_y(alignment::Vertical::Center)
        .style(|_| container::Style {
            background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.75).into()),
            ..Default::default()
        })
        .into()
}
