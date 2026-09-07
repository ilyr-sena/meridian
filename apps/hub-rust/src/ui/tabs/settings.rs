//! Settings Tab: Tailscale dynamic key URLs, sensitive data masking, and sideload configs.

use iced::{
    widget::{button, checkbox, column, container, row, scrollable, text, text_input, Space},
    Alignment, Element, Length,
};
use crate::ui::theme::*;

#[derive(Debug, Clone)]
pub struct SettingsState {
    pub mask_sensitive: bool,
    pub tailscale_key_url: String,
    pub tailscale_auth_key: String,
    pub anisette_url: String,
    pub apple_id: String,
    pub is_saving: bool,
}

pub fn view_settings<'a, Message>(
    state: &'a SettingsState,
    on_toggle_mask: impl Fn(bool) -> Message + 'a + Copy,
    on_key_url_change: impl Fn(String) -> Message + 'a + Copy,
    on_auth_key_change: impl Fn(String) -> Message + 'a + Copy,
    on_anisette_change: impl Fn(String) -> Message + 'a + Copy,
    on_apple_id_change: impl Fn(String) -> Message + 'a + Copy,
    on_refresh_key: Message,
    on_save: Message,
) -> Element<'a, Message>
where
    Message: 'a + Clone,
{
    // 1. Privacy Section
    let mask_checkbox = row![
        checkbox(state.mask_sensitive)
            .on_toggle(on_toggle_mask)
            .size(16),
        text("Mask sensitive device data (hide full UDID, serials, and names)")
            .font(FONT_REGULAR)
            .size(13)
            .color(TEXT_PRIMARY),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let privacy_section = container(
        column![
            text("PRIVACY & DISPLAY")
                .font(FONT_SEMIBOLD)
                .size(11)
                .color(TEXT_MUTED),
            Space::new().height(10),
            mask_checkbox,
        ]
        .padding(16)
    )
    .style(style_card);

    // 2. Tailscale Mesh Section
    let tailscale_section = container(
        column![
            text("TAILSCALE MESH CONFIGURATION")
                .font(FONT_SEMIBOLD)
                .size(11)
                .color(TEXT_MUTED),
            Space::new().height(10),
            text("Dynamic Key Source URL (auto-fetched to avoid 90-day expiry):")
                .font(FONT_REGULAR)
                .size(12)
                .color(TEXT_SECONDARY),
            Space::new().height(4),
            row![
                text_input("https://meridianhub.cc/api/mesh/authkey", &state.tailscale_key_url)
                    .on_input(on_key_url_change)
                    .padding(8)
                    .size(12)
                    .style(style_text_input),
                button(text("Fetch Key Now").font(FONT_MEDIUM).size(11))
                    .padding([8, 14])
                    .on_press(on_refresh_key)
                    .style(style_button_secondary),
            ]
            .spacing(8),
            Space::new().height(12),
            text("Manual Tailscale Auth Key (optional local override):")
                .font(FONT_REGULAR)
                .size(12)
                .color(TEXT_SECONDARY),
            Space::new().height(4),
            text_input("tskey-auth-...", &state.tailscale_auth_key)
                .on_input(on_auth_key_change)
                .padding(8)
                .size(12)
                .secure(true)
                .style(style_text_input),
        ]
        .padding(16)
    )
    .style(style_card);

    // 3. Sideloading Section
    let sideload_section = container(
        column![
            text("SIDELOADING & DEVELOPER PORTAL")
                .font(FONT_SEMIBOLD)
                .size(11)
                .color(TEXT_MUTED),
            Space::new().height(10),
            text("Anisette Server URL:")
                .font(FONT_REGULAR)
                .size(12)
                .color(TEXT_SECONDARY),
            Space::new().height(4),
            text_input("http://100.51.75.20:6969", &state.anisette_url)
                .on_input(on_anisette_change)
                .padding(8)
                .size(12)
                .style(style_text_input),
            Space::new().height(12),
            text("Saved Apple ID (used for development provisioning):")
                .font(FONT_REGULAR)
                .size(12)
                .color(TEXT_SECONDARY),
            Space::new().height(4),
            text_input("developer@apple.com", &state.apple_id)
                .on_input(on_apple_id_change)
                .padding(8)
                .size(12)
                .style(style_text_input),
        ]
        .padding(16)
    )
    .style(style_card);

    // 4. Save Button
    let btn_save = button(text("Save Preferences").font(FONT_MEDIUM).size(13))
        .padding([8, 24])
        .on_press(on_save)
        .style(style_button_primary);

    scrollable(
        column![
            privacy_section,
            tailscale_section,
            sideload_section,
            row![Space::new().width(Length::Fill), btn_save],
        ]
        .spacing(14)
    )
    .into()
}
