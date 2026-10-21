//! Status Tab: live connection metrics, rathole tunnel status, and port table.
//!
//! Three cards:
//!   1. Rathole control channel state (the tunnel itself),
//!   2. **Internet Connection** — the live `NetHealthSnapshot` (grade, verdict,
//!      latency, jitter, loss, download throughput, per-anchor detail),
//!   3. Port mappings matrix for attached devices.

use chrono::Local;
use iced::{
    widget::{column, container, row, scrollable, text, Space},
    Alignment, Color, Element, Length,
};
use crate::device::models::DeviceReport;
use crate::net::{AnchorHealth, NetGrade, NetHealthSnapshot, SPEED_FLOOR_MBPS};
use crate::remote::mesh::TunnelStatus;
use crate::ui::theme::*;

pub fn view_status<'a, Message>(
    tunnel: &'a TunnelStatus,
    net: &'a NetHealthSnapshot,
    devices: &'a [DeviceReport],
    uptime_secs: u64,
    mask_sensitive: bool,
) -> Element<'a, Message>
where
    Message: 'a + Clone + 'static,
{
    let uptime_str = format!(
        "{:02}h {:02}m {:02}s",
        uptime_secs / 3600,
        (uptime_secs % 3600) / 60,
        uptime_secs % 60
    );

    // 1. Tunnel Card (rathole control channel state)
    let tunnel_card = container(
        column![
            text("RATHOLE DIRECT TUNNEL")
                .font(FONT_SEMIBOLD)
                .size(11)
                .color(TEXT_MUTED),
            Space::new().height(8),
            row![
                column![
                    text("Tunnel State").font(FONT_REGULAR).size(12).color(TEXT_SECONDARY),
                    text(&tunnel.status_text).font(FONT_MEDIUM).size(14).color(
                        if tunnel.is_running { ACCENT_EMERALD } else { ACCENT_AMBER }
                    ),
                ],
                Space::new().width(Length::Fill),
                column![
                    text("VPS Endpoint").font(FONT_REGULAR).size(12).color(TEXT_SECONDARY),
                    text("98.84.189.148:2333").font(FONT_MONO).size(14).color(TEXT_PRIMARY),
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

    // 2. Internet Connection Health Card (live net monitor data)
    let net_card = view_internet_card(net);

    // 3. Active Sessions Table
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
        let state_label = match d.session_phase.label() {
            Some(l) => l,
            None => d.state.label(),
        };
        table_rows.push(
            container(
                row![
                    text(format!("#{}", d.ports.slot)).font(FONT_MONO_TIGHT).size(12).color(TEXT_SECONDARY).width(60),
                    text(d.masked_name(mask_sensitive)).font(FONT_MEDIUM).size(12).color(TEXT_PRIMARY).width(160),
                    text(format!(":{}", d.ports.wda)).font(FONT_MONO_TIGHT).size(12).color(ACCENT_BLUE).width(80),
                    text(format!(":{}", d.ports.stream)).font(FONT_MONO_TIGHT).size(12).color(ACCENT_EMERALD).width(80),
                    text(format!(":{}", d.ports.bridge)).font(FONT_MONO_TIGHT).size(12).color(TEXT_SECONDARY).width(80),
                    text(state_label).font(FONT_REGULAR).size(12).color(TEXT_SECONDARY).width(100),
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
        column![tunnel_card, net_card, sessions_card]
            .spacing(14)
    )
    .into()
}

/// A small colored status dot (same visual language as the device pills).
fn dot<Message: 'static>(color: Color) -> iced::widget::Container<'static, Message> {
    container(Space::new().width(6).height(6)).style(move |_| container::Style {
        background: Some(color.into()),
        border: iced::Border {
            radius: iced::border::Radius::from(9999.0),
            ..Default::default()
        },
        ..Default::default()
    })
}

/// The live Internet Connection card, rendered from the net monitor snapshot.
fn view_internet_card<'a, Message>(net: &'a NetHealthSnapshot) -> Element<'a, Message>
where
    Message: 'a + Clone + 'static,
{
    // -- Grade badge & fault verdict --
    let grade_color = match net.grade {
        NetGrade::Excellent | NetGrade::Good => ACCENT_EMERALD,
        NetGrade::Fair => ACCENT_BLUE,
        NetGrade::Poor => ACCENT_AMBER,
        NetGrade::Bad | NetGrade::Down => ACCENT_ROSE,
        NetGrade::Unknown => TEXT_MUTED,
    };
    let grade_pill = container(
        row![
            dot(grade_color),
            text(net.grade.label())
                .font(FONT_MEDIUM)
                .size(13)
                .color(grade_color),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    )
    .padding([3, 10])
    .style(style_pill_badge);

    // -- Headline numbers --
    let latency_color = match net.current_latency_ms {
        Some(ms) if ms < 80 => ACCENT_EMERALD,
        Some(ms) if ms < 150 => ACCENT_BLUE,
        Some(ms) if ms < 300 => ACCENT_AMBER,
        Some(_) => ACCENT_ROSE,
        None => TEXT_MUTED,
    };
    let jitter_color = if net.jitter_ms < 5.0 {
        ACCENT_EMERALD
    } else if net.jitter_ms < 10.0 {
        ACCENT_BLUE
    } else if net.jitter_ms < 20.0 {
        ACCENT_AMBER
    } else {
        ACCENT_ROSE
    };
    let loss_color = if net.loss_pct < 2.0 {
        ACCENT_EMERALD
    } else if net.loss_pct < 5.0 {
        ACCENT_AMBER
    } else {
        ACCENT_ROSE
    };

    let fmt_ms = |v: Option<u32>| match v {
        Some(ms) => format!("{ms}ms"),
        None => "—".to_string(),
    };
    let stat = |label: &str, value: String, color: Color, big: bool| {
        column![
            text(label.to_string()).font(FONT_REGULAR).size(12).color(TEXT_SECONDARY),
            text(value)
                .font(FONT_MONO)
                .size(if big { 20 } else { 16 })
                .color(color),
        ]
    };

    let headline_row = row![
        stat("Latency", fmt_ms(net.current_latency_ms), latency_color, true),
        Space::new().width(Length::Fill),
        stat("Median p50", fmt_ms(net.median_latency_ms), TEXT_PRIMARY, false),
        Space::new().width(Length::Fill),
        stat("P95", fmt_ms(net.p95_latency_ms), ACCENT_AMBER, false),
        Space::new().width(Length::Fill),
        stat("Jitter", format!("{:.1}ms", net.jitter_ms), jitter_color, false),
        Space::new().width(Length::Fill),
        stat("Loss", format!("{:.1}%", net.loss_pct), loss_color, false),
        Space::new().width(Length::Fill),
        stat("Min", fmt_ms(net.min_latency_ms), ACCENT_EMERALD, false),
        Space::new().width(Length::Fill),
        stat("Avg", fmt_ms(net.avg_latency_ms), TEXT_PRIMARY, false),
    ];

    // -- Download throughput --
    let (speed_text, speed_color) = if net.speed_running {
        ("Measuring throughput...".to_string(), TEXT_SECONDARY)
    } else if net.speed_paused {
        ("Paused — live session streaming".to_string(), TEXT_MUTED)
    } else if let Some(sample) = &net.download {
        let age_min = (Local::now() - sample.at).num_minutes().max(0);
        let when = if age_min < 1 {
            "just now".to_string()
        } else {
            format!("{age_min} min ago")
        };
        let color = if sample.mbps < SPEED_FLOOR_MBPS {
            ACCENT_AMBER
        } else if sample.mbps >= 20.0 {
            ACCENT_EMERALD
        } else {
            TEXT_PRIMARY
        };
        (format!("{:.1} Mbps ({when})", sample.mbps), color)
    } else {
        ("Pending first measurement".to_string(), TEXT_MUTED)
    };

    let speed_row = row![
        text("Download").font(FONT_REGULAR).size(12).color(TEXT_SECONDARY),
        Space::new().width(8),
        text(speed_text).font(FONT_MONO).size(14).color(speed_color),
    ];

    // -- Per-anchor detail table --
    let mut anchor_rows: Vec<Element<'a, Message>> = vec![
        row![
            text("TARGET").font(FONT_SEMIBOLD).size(11).color(TEXT_MUTED).width(230),
            text("STATE").font(FONT_SEMIBOLD).size(11).color(TEXT_MUTED).width(110),
            text("RTT").font(FONT_SEMIBOLD).size(11).color(TEXT_MUTED).width(70),
            text("P50").font(FONT_SEMIBOLD).size(11).color(TEXT_MUTED).width(70),
            text("LOSS").font(FONT_SEMIBOLD).size(11).color(TEXT_MUTED).width(70),
        ]
        .padding([2, 2])
        .into(),
    ];

    for anchor in &net.anchors {
        let (health_label, health_color) = match anchor.health {
            AnchorHealth::Healthy => ("Healthy", ACCENT_EMERALD),
            AnchorHealth::Degraded => ("Degraded", ACCENT_AMBER),
            AnchorHealth::Unreachable => ("Unreachable", ACCENT_ROSE),
            AnchorHealth::Quarantined => ("Blocked", TEXT_MUTED),
        };
        anchor_rows.push(
            row![
                text(format!("{} ({})", anchor.label, anchor.addr))
                    .font(FONT_REGULAR)
                    .size(12)
                    .color(TEXT_PRIMARY)
                    .width(230),
                text(health_label)
                    .font(FONT_MEDIUM)
                    .size(11)
                    .color(health_color)
                    .width(110),
                text(fmt_ms(anchor.current_rtt_ms))
                    .font(FONT_MONO)
                    .size(11)
                    .color(TEXT_SECONDARY)
                    .width(70),
                text(fmt_ms(anchor.median_rtt_ms))
                    .font(FONT_MONO)
                    .size(11)
                    .color(TEXT_SECONDARY)
                    .width(70),
                text(format!("{:.1}%", anchor.loss_pct))
                    .font(FONT_MONO)
                    .size(11)
                    .color(TEXT_SECONDARY)
                    .width(70),
            ]
            .padding([3, 2])
            .into(),
        );
    }

    container(
        column![
            row![
                column![
                    text("INTERNET CONNECTION")
                        .font(FONT_SEMIBOLD)
                        .size(11)
                        .color(TEXT_MUTED),
                    Space::new().height(2),
                    text(net.verdict.label())
                        .font(FONT_REGULAR)
                        .size(12)
                        .color(TEXT_SECONDARY),
                ],
                Space::new().width(Length::Fill),
                grade_pill,
            ]
            .align_y(Alignment::Center),
            Space::new().height(10),
            headline_row,
            Space::new().height(10),
            speed_row,
            Space::new().height(12),
            column(anchor_rows).spacing(3),
        ]
        .padding(16)
    )
    .style(style_card)
    .into()
}
