//! Status Tab: live connection metrics, Tailscale VPN status, and port table.

use iced::{
    widget::{column, container, row, scrollable, text, Space},
    Alignment, Color, Element, Length,
};
use crate::device::models::DeviceReport;
use crate::remote::mesh::MeshStatus;
use crate::ui::theme::*;

pub fn view_status<'a, Message>(
    mesh: &'a MeshStatus,
    devices: &'a [DeviceReport],
    uptime_secs: u64,
) -> Element<'a, Message>
where
    Message: 'a + Clone,
{
    let uptime_str = format!(
        "{:02}h {:02}m {:02}s",
        uptime_secs / 3600,
        (uptime_secs % 3600) / 60,
        uptime_secs % 60
    );

    // 1. Mesh Card
    let mesh_ip_text = mesh.mesh_ip.as_deref().unwrap_or("Not assigned");
    let mesh_card = container(
        column![
            text("TAILSCALE MESH OVERLAY")
                .font(FONT_SEMIBOLD)
                .size(11)
                .color(TEXT_MUTED),
            Space::new().height(8),
            row![
                column![
                    text("Mesh IP Address").font(FONT_REGULAR).size(12).color(TEXT_SECONDARY),
                    text(mesh_ip_text).font(FONT_MONO).size(15).color(TEXT_PRIMARY),
                ],
                Space::new().width(Length::Fill),
                column![
                    text("Connection State").font(FONT_REGULAR).size(12).color(TEXT_SECONDARY),
                    text(&mesh.status_text).font(FONT_MEDIUM).size(14).color(
                        if mesh.is_running { ACCENT_EMERALD } else { ACCENT_AMBER }
                    ),
                ],
                Space::new().width(Length::Fill),
                column![
                    text("Hub Uptime").font(FONT_REGULAR).size(12).color(TEXT_SECONDARY),
                    text(uptime_str).font(FONT_MONO).size(14).color(TEXT_PRIMARY),
                ],
            ]
        ]
        .padding(16)
    )
    .style(style_card);

    // 2. Active Sessions Table
    let mut table_rows: Vec<Element<'a, Message>> = Vec::new();

    table_rows.push(
        row![
            text("SLOT").font(FONT_SEMIBOLD).size(11).color(TEXT_MUTED).width(60),
            text("DEVICE").font(FONT_SEMIBOLD).size(11).color(TEXT_MUTED).width(160),
            text("WDA").font(FONT_SEMIBOLD).size(11).color(TEXT_MUTED).width(80),
            text("STREAM").font(FONT_SEMIBOLD).size(11).color(TEXT_MUTED).width(80),
            text("BRIDGE").font(FONT_SEMIBOLD).size(11).color(TEXT_MUTED).width(80),
            text("STATE").font(FONT_SEMIBOLD).size(11).color(TEXT_MUTED).width(100),
        ]
        .padding([4, 8])
        .into()
    );

    for d in devices {
        table_rows.push(
            container(
                row![
                    text(format!("#{}", d.ports.slot)).font(FONT_MONO).size(12).color(TEXT_SECONDARY).width(60),
                    text(&d.name).font(FONT_MEDIUM).size(12).color(TEXT_PRIMARY).width(160),
                    text(format!(":{}", d.ports.wda)).font(FONT_MONO).size(12).color(ACCENT_BLUE).width(80),
                    text(format!(":{}", d.ports.stream)).font(FONT_MONO).size(12).color(ACCENT_EMERALD).width(80),
                    text(format!(":{}", d.ports.bridge)).font(FONT_MONO).size(12).color(TEXT_SECONDARY).width(80),
                    text(d.state.label()).font(FONT_REGULAR).size(12).color(TEXT_SECONDARY).width(100),
                ]
                .align_y(Alignment::Center)
                .padding([6, 8])
            )
            .style(|_| container::Style {
                background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.02).into()),
                border: iced::Border {
                    radius: iced::border::Radius::from(6.0),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
        );
    }

    let sessions_card = container(
        column![
            text("PORT MAPPINGS MATRIX")
                .font(FONT_SEMIBOLD)
                .size(11)
                .color(TEXT_MUTED),
            Space::new().height(8),
            column(table_rows).spacing(4),
        ]
        .padding(16)
    )
    .style(style_card);

    scrollable(
        column![mesh_card, sessions_card]
            .spacing(14)
    )
    .into()
}
