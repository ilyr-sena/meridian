//! Meridian Hub — High-Performance Native iOS Device Desktop Orchestrator.
//!
//! Pure Rust 2024 implementation with zero Python dependencies,
//! hardware-accelerated GUI, usbmuxd multiplexing, and Tailscale mesh sidecar.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(dead_code)]

mod app;
mod core;
mod device;
mod remote;
mod sideload;
mod ui;

use iced::{window, Size};
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::app::MeridianApp;
use crate::core::firewall::ensure_firewall_rules;
use crate::core::privilege::ensure_admin_or_relaunch;

// Embedded Fonts
const FONT_GEIST_REGULAR: &[u8] = include_bytes!("../assets/fonts/Geist-Regular.ttf");
const FONT_GEIST_MEDIUM: &[u8] = include_bytes!("../assets/fonts/Geist-Medium.ttf");
const FONT_GEIST_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Geist-SemiBold.ttf");
const FONT_GEIST_MONO: &[u8] = include_bytes!("../assets/fonts/GeistMono-Regular.ttf");

// Embedded App Icon
const APP_ICON_BYTES: &[u8] = include_bytes!("../assets/meridian-icon.png");

fn main() -> iced::Result {
    // 1. Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,meridian_hub=info,winit=warn,iced_winit=warn,wgpu=warn")),
        )
        .init();

    info!("Starting Meridian Hub v0.4.0 (Pure Rust)...");

    // 2. Ensure admin elevation & configure firewall rules
    if !ensure_admin_or_relaunch() {
        info!("Running in unprivileged user mode");
    }
    ensure_firewall_rules();

    // 3. Load embedded app icon for the OS taskbar and window
    let icon = load_app_icon();

    // 4. Run application
    iced::application(
        MeridianApp::boot,
        MeridianApp::update,
        MeridianApp::view,
    )
    .title(app_title)
    .theme(app_theme)
    .subscription(MeridianApp::subscription)
    .window(window::Settings {
        size: Size::new(880.0, 640.0),
        min_size: Some(Size::new(760.0, 520.0)),
        decorations: false, // Custom Linear/Attio style titlebar
        transparent: true,
        icon,
        ..Default::default()
    })
    .font(FONT_GEIST_REGULAR)
    .font(FONT_GEIST_MEDIUM)
    .font(FONT_GEIST_SEMIBOLD)
    .font(FONT_GEIST_MONO)
    .run()
}

fn app_title(_: &MeridianApp) -> String {
    "Meridian Hub".to_string()
}

fn app_theme(_: &MeridianApp) -> iced::Theme {
    iced::Theme::Dark
}

impl MeridianApp {
    pub fn boot() -> (Self, iced::Task<crate::app::Message>) {
        Self::new()
    }
}

fn load_app_icon() -> Option<window::Icon> {
    if let Ok(img) = image::load_from_memory(APP_ICON_BYTES) {
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        window::icon::from_rgba(rgba.into_raw(), width, height).ok()
    } else {
        None
    }
}
