//! Devices Tab: real-time cards of attached iPhones with session controls.
//!
//! The card's look and actions are driven by the *combinable* device health
//! (installed × developer-mode × locked × paired) plus the app-level session
//! phase. Every combination resolves to a coherent pill, message and button set.

use iced::{
    alignment,
    widget::{button, column, container, row, scrollable, text, Space},
    Alignment, Color, Element, Length,
};
use crate::device::models::{DeviceReport, DeviceState, SessionPhase};
use crate::ui::theme::*;

pub fn view_devices<'a, Message>(
    devices: &'a [DeviceReport],
    mask_sensitive: bool,
    on_start: impl Fn(String) -> Message + 'a + Copy,
    on_stop: impl Fn(String) -> Message + 'a + Copy,
    on_sideload: impl Fn(String) -> Message + 'a + Copy,
) -> Element<'a, Message>
where
    Message: 'a + Clone,
{
    if devices.is_empty() {
        return view_empty_state();
    }

    let cards: Vec<Element<'a, Message>> = devices
        .iter()
        .map(|d| view_device_card(d, mask_sensitive, on_start, on_stop, on_sideload))
        .collect();

    let content = column(cards).spacing(12);

    scrollable(content)
        .height(Length::Fill)
        .into()
}

/// Map a capability state to the pill dot color + short label.
fn status_pill(state: DeviceState) -> (Color, &'static str) {
    match state {
        DeviceState::Ready => (ACCENT_EMERALD, "Ready to Stream"),
        DeviceState::NeedsSideload => (ACCENT_AMBER, "Runner Not Installed"),
        DeviceState::DeveloperModeOff => (ACCENT_AMBER, "Developer Mode Off"),
        DeviceState::Locked => (ACCENT_AMBER, "Locked"),
        DeviceState::Unpaired => (ACCENT_BLUE, "Trust Required"),
        DeviceState::Error => (ACCENT_ROSE, "Error"),
        _ => (TEXT_MUTED, "Checking..."),
    }
}

fn view_device_card<'a, Message>(
    dev: &'a DeviceReport,
    mask_sensitive: bool,
    on_start: impl Fn(String) -> Message + 'a + Copy,
    on_stop: impl Fn(String) -> Message + 'a + Copy,
    on_sideload: impl Fn(String) -> Message + 'a + Copy,
) -> Element<'a, Message>
where
    Message: 'a + Clone,
{
    // Status pill: a live session overrides the capability state.
    let (dot_color, status_text) = match dev.session_phase.label() {
        Some(label) => match dev.session_phase {
            SessionPhase::Running => (ACCENT_EMERALD, label),
            _ => (ACCENT_AMBER, label),
        },
        None => status_pill(dev.state),
    };

    let status_pill = container(
        row![
            container(Space::new().width(6).height(6))
                .style(move |_| container::Style {
                    background: Some(dot_color.into()),
                    border: iced::Border {
                        radius: iced::border::Radius::from(9999.0),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            text(status_text)
                .font(FONT_MEDIUM)
                .size(11)
                .color(TEXT_SECONDARY),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
    )
    .padding([3, 10])
    .style(style_pill_badge);

    let slot_pill = container(
        text(format!("Slot {}", dev.ports.slot))
            .font(FONT_MONO)
            .size(11)
            .color(TEXT_MUTED)
    )
    .padding([3, 8])
    .style(style_pill_badge);

    // Header: Name & Model
    let header = row![
        column![
            text(dev.masked_name(mask_sensitive))
                .font(FONT_SEMIBOLD)
                .size(15)
                .color(TEXT_PRIMARY),
            text(format!("{} • {}", dev.model, dev.os_version))
                .font(FONT_REGULAR)
                .size(12)
                .color(TEXT_SECONDARY),
        ]
        .spacing(2),
        Space::new().width(Length::Fill),
        slot_pill,
        status_pill,
    ]
    .align_y(Alignment::Center);

    // Port Badges Row
    let ports_row = row![
        container(
            text(format!("WDA :{}", dev.ports.wda))
                .font(FONT_MONO)
                .size(11)
                .color(TEXT_SECONDARY)
        )
        .padding([2, 8])
        .style(style_pill_badge),
        container(
            text(format!("STREAM :{}", dev.ports.stream))
                .font(FONT_MONO)
                .size(11)
                .color(TEXT_SECONDARY)
        )
        .padding([2, 8])
        .style(style_pill_badge),
        container(
            text(format!("TOUCH :{}", dev.ports.bridge))
                .font(FONT_MONO)
                .size(11)
                .color(TEXT_SECONDARY)
        )
        .padding([2, 8])
        .style(style_pill_badge),
        Space::new().width(Length::Fill),
        text(format!("UDID: {}", dev.masked_udid(mask_sensitive)))
            .font(FONT_MONO)
            .size(11)
            .color(TEXT_MUTED),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // Action Controls — driven strictly by the session phase first, then by
    // the combinable capability state. There is NO "Re-Sideload" button: once
    // the runner is installed the only action is Start; uninstalling it on the
    // device flips the card back to "Sideload Runner" in real time.
    let actions: Element<'a, Message> = match dev.session_phase {
        SessionPhase::Running => {
            let btn_stop = button(text("Stop Session").font(FONT_MEDIUM).size(12))
                .padding([6, 16])
                .on_press(on_stop(dev.udid.clone()))
                .style(style_button_danger);

            row![btn_stop].spacing(8).align_y(Alignment::Center).into()
        }
        SessionPhase::Starting => {
            let btn_starting = button(text("Starting...").font(FONT_MEDIUM).size(12))
                .padding([6, 16])
                .style(style_button_secondary);

            row![btn_starting]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
        }
        SessionPhase::Idle => match dev.state {
            DeviceState::Ready | DeviceState::Error => {
                // Ready → Start; Error → Retry Start (message explains why).
                let label = if dev.state == DeviceState::Error {
                    "Retry Start"
                } else {
                    "Start Session"
                };
                let btn_start = button(text(label).font(FONT_MEDIUM).size(12))
                    .padding([6, 18])
                    .on_press(on_start(dev.udid.clone()))
                    .style(style_button_primary);

                row![btn_start].spacing(8).align_y(Alignment::Center).into()
            }
            DeviceState::NeedsSideload => {
                let btn_sideload = button(text("Sideload Runner").font(FONT_MEDIUM).size(12))
                    .padding([6, 20])
                    .on_press(on_sideload(dev.udid.clone()))
                    .style(style_button_primary);

                row![btn_sideload]
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .into()
            }
            DeviceState::DeveloperModeOff | DeviceState::Locked | DeviceState::Unpaired => {
                // Actionable path shown as a disabled control; the message below
                // tells the user exactly what to do.
                let btn_disabled = button(text("Start Session").font(FONT_MEDIUM).size(12))
                    .padding([6, 18])
                    .style(style_button_secondary);

                row![btn_disabled]
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .into()
            }
            _ => {
                // Still diagnosing (Connected) — nothing to offer yet.
                Space::new().into()
            }
        },
    };

    let message = if dev.status_message.is_empty() {
        None
    } else {
        Some(
            container(
                text(&dev.status_message)
                    .font(FONT_REGULAR)
                    .size(11)
                    .color(TEXT_MUTED),
            )
            .width(Length::Fill)
            .padding([6, 10])
            .style(style_pill_badge),
        )
    };

    let mut card_children = column![header, Space::new().height(10), ports_row];
    if let Some(m) = message {
        card_children = card_children.push(Space::new().height(8)).push(m);
    }
    card_children = card_children
        .push(Space::new().height(12))
        .push(row![Space::new().width(Length::Fill), actions]);

    container(card_children.padding(16))
        .style(style_card)
        .into()
}

fn view_empty_state<'a, Message: 'a>() -> Element<'a, Message> {
    container(
        column![
            text("No iPhones detected over USB")
                .font(FONT_SEMIBOLD)
                .size(15)
                .color(TEXT_PRIMARY),
            Space::new().height(4),
            text("Connect an iPhone running iOS with a verified USB cable.\nEnsure the device is unlocked and the computer is trusted.")
                .font(FONT_REGULAR)
                .size(12)
                .color(TEXT_MUTED)
                .align_x(alignment::Horizontal::Center),
        ]
        .align_x(Alignment::Center)
        .padding(48)
    )
    .width(Length::Fill)
    .align_x(alignment::Horizontal::Center)
    .style(style_card)
    .into()
}