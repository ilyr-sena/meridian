//! Custom frameless title bar with drag support, status pill, and window controls.

use iced::{
    alignment,
    widget::{button, container, row, svg, text, MouseArea, Space},
    Alignment, Element, Length,
};
use crate::ui::theme::*;

#[derive(Debug, Clone)]
pub enum TitleBarAction {
    Drag,
    Minimize,
    Maximize,
    Close,
}

use std::sync::LazyLock;

/// Brand logo SVG (repo-root asset), embedded at build time and stretched 1px
/// wider (28 -> 29): the viewport is widened with preserveAspectRatio="none"
/// so the 28x19 viewBox art is drawn across 29x19 — countering the optical
/// horizontal narrowness at titlebar size. The asset file itself is untouched
/// (single source of truth for other consumers).
static LOGO_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| {
    const RAW: &str = include_str!("../../../../assets/meridian-logoicon.svg");
    RAW.replace(
        "<svg width=\"28\" height=\"19\" viewBox=\"0 0 28 19\"",
        "<svg width=\"29\" height=\"19\" viewBox=\"0 0 28 19\" preserveAspectRatio=\"none\"",
    )
    .into_bytes()
});

pub fn view_titlebar<'a, Message>(
    tunnel_online: bool,
    latency_ms: u32,
    on_action: impl Fn(TitleBarAction) -> Message + 'a + Copy,
) -> Element<'a, Message>
where
    Message: 'a + Clone,
{
    // 1. App branding: the Meridian logo SVG, embedded at build time — drawn
    //    at 29x19 (the viewport patch above stretches it 1px wider).
    let logo = svg::Svg::new(svg::Handle::from_memory(LOGO_SVG.clone()))
        .width(29.0)
        .height(19.0);

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

    // Draggable area: everything except window controls
    let draggable_content = row![
        logo,
        Space::new().width(16),
        tunnel_indicator,
        Space::new().width(Length::Fill),
    ]
    .align_y(Alignment::Center);

    let drag_area = MouseArea::new(draggable_content)
        .on_press(on_action(TitleBarAction::Drag));

    container(
        row![
            drag_area,
            controls,
        ]
        .align_y(Alignment::Center)
        // Zero bottom padding: the content panel wrapper below insets the
        // panel 4px, and matching that with 4px on top centers the titlebar
        // content exactly between the shell's top edge and the content
        // panel's top edge (equal 4px above and below).
        .padding(iced::Padding {
            top: 4.0,
            right: 14.0,
            bottom: 0.0,
            left: 14.0,
        })
    )
    .into()
}
