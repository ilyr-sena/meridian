//! Custom frameless title bar with drag support, status pill, and window controls.

use iced::{
    alignment,
    widget::{button, container, row, text, Space},
    Alignment, Element, Length,
};
use crate::ui::theme::*;

#[derive(Debug, Clone)]
pub enum TitleBarAction {
    Minimize,
    Maximize,
    Close,
}

pub fn view_titlebar<'a, Message>(
    tunnel_online: bool,
    latency_ms: u32,
    on_action: impl Fn(TitleBarAction) -> Message + 'a + Copy,
) -> Element<'a, Message>
where
    Message: 'a + Clone,
{
    // 1. App branding
    let title_badge = row![
        text("MERIDIAN")
            .font(FONT_SEMIBOLD)
            .size(13)
            .color(TEXT_PRIMARY),
        container(
            text("HUB")
                .font(FONT_MONO)
                .size(10)
                .color(ACCENT_BLUE)
        )
        .padding([2, 6])
        .style(style_pill_badge),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // 2. Tunnel status pill with latency
    let tunnel_indicator = if tunnel_online {
        let latency_color = if latency_ms < 80 {
            ACCENT_EMERALD
        } else if latency_ms < 150 {
            ACCENT_BLUE
        } else {
            ACCENT_AMBER
        };
        let pill_text = if latency_ms > 0 {
            format!("{}ms", latency_ms)
        } else {
            "Online".to_string()
        };
        container(
            row![
                container(Space::new().width(6).height(6))
                    .style(move |_| container::Style {
                        background: Some(latency_color.into()),
                        border: iced::Border {
                            radius: iced::border::Radius::from(9999.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                text(pill_text)
                    .font(FONT_MONO)
                    .size(11)
                    .color(TEXT_SECONDARY),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
        )
        .padding([3, 10])
        .style(style_pill_badge)
    } else {
        container(
            row![
                container(Space::new().width(6).height(6))
                    .style(|_| container::Style {
                        background: Some(ACCENT_AMBER.into()),
                        border: iced::Border {
                            radius: iced::border::Radius::from(9999.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                text("Tunnel Offline")
                    .font(FONT_MONO)
                    .size(11)
                    .color(TEXT_MUTED),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
        )
        .padding([3, 10])
        .style(style_pill_badge)
    };

    // 3. Window Control Buttons
    let btn_min = button(
        text("—")
            .size(11)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
    )
    .on_press(on_action(TitleBarAction::Minimize))
    .width(28)
    .height(24)
    .style(style_button_secondary);

    let btn_close = button(
        text("✕")
            .size(11)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
    )
    .on_press(on_action(TitleBarAction::Close))
    .width(28)
    .height(24)
    .style(style_button_danger);

    let controls = row![btn_min, btn_close]
        .spacing(6)
        .align_y(Alignment::Center);

    container(
        row![
            title_badge,
            Space::new().width(16),
            tunnel_indicator,
            Space::new().width(Length::Fill),
            controls,
        ]
        .align_y(Alignment::Center)
        .padding([8, 14])
    )
    .into()
}
