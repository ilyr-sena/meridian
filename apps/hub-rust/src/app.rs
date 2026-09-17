//! Main Iced application lifecycle and state orchestration.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use iced::{
    widget::{button, column, container, row, stack, text},
    window, Element, Length, Subscription, Task,
};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::core::slots::SlotManager;
use crate::core::vault::Vault;
use crate::device::actions::WdaClient;
use crate::device::health::{health_state, os_major, probe_health, status_message};
use crate::device::launcher::{kill_meridian_runner, launch_meridian_runner};
use crate::device::models::{DeviceReport, DeviceState, SessionPhase};
use crate::device::monitor::{DeviceEvent, DeviceMonitor};
use crate::device::tunnel::{start_tunnel, ActiveTunnel};
use crate::remote::bridge::BridgeServer;
use crate::remote::heartbeat::{send_offline_sync, sync_session_state, HeartbeatWorker};
use crate::remote::mesh::{TunnelStatus, TunnelSupervisor};
use crate::sideload::sideloader::{
    SideloadOptions, Sideloader, TwoFactorPrompt, next_sideload_progress, next_two_factor_prompt,
    sideload_progress_tx, two_factor_tx,
};
use crate::ui::dialogs::sideload::{view_sideload_modal, SideloadDialogState};
use crate::ui::tabs::{
    devices::view_devices,
    logs::{view_logs, LogEntry, LogLevel},
    settings::{view_settings, SettingsState},
    status::view_status,
};
use crate::ui::theme::*;
use crate::ui::titlebar::{view_titlebar, TitleBarAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Devices,
    Status,
    Logs,
    Settings,
}

#[derive(Debug, Clone)]
pub enum Message {
    TabSelected(Tab),
    DeviceEvent(DeviceEvent),
    StartDevice(String),
    DeviceStarted(String, Vec<Arc<ActiveTunnel>>),
    DeviceLaunchFailed(String, String, Vec<Arc<ActiveTunnel>>),
    StopDevice(String),
    OpenSideload(String),
    SideloadAppleIdChanged(String),
    SideloadPasswordChanged(String),
    SideloadTwoFactorCodeChanged(String),
    SideloadTwoFactor(TwoFactorPrompt),
    SubmitTwoFactor,
    SubmitSideload,
    SideloadProgress(f32, String),
    SideloadFinished(Result<(), String>),
    CloseSideload,
    ToggleMask(bool),
    AnisetteChanged(String),
    AppleIdChanged(String),
    SavePreferences,
    SelectLogLevel(LogLevel),
    LogSearchChanged(String),
    ClearLogs,
    CopyLogs,
    AddLog(LogLevel, String),
    Tick,
    TunnelUpdated(TunnelStatus),
    TitleAction(TitleBarAction),
}

pub struct MeridianApp {
    active_tab: Tab,
    devices: Vec<DeviceReport>,
    active_tunnels: HashMap<String, Vec<Arc<ActiveTunnel>>>,
    active_heartbeats: HashMap<String, Arc<HeartbeatWorker>>,
    active_bridges: HashMap<String, Arc<BridgeServer>>,
    active_watchdogs: HashMap<String, Arc<AtomicBool>>,
    slot_mgr: SlotManager,
    vault: Vault,
    tunnel_supervisor: Arc<TunnelSupervisor>,
    tunnel_status: TunnelStatus,
    logs: Vec<LogEntry>,
    log_level: LogLevel,
    log_search: String,
    sideload: SideloadDialogState,
    settings: SettingsState,
    uptime_secs: u64,
}

impl MeridianApp {
    pub fn new() -> (Self, Task<Message>) {
        let vault = Vault::default_location();
        let vault_data = vault.load();

        let slot_mgr = SlotManager::new();
        let tunnel_supervisor = Arc::new(TunnelSupervisor::new(vault.clone()));
        tunnel_supervisor.start();

        let settings = SettingsState {
            mask_sensitive: vault_data.sensitive_data_masked,
            anisette_url: vault_data.anisette_url.unwrap_or_else(|| "http://98.84.189.148:6969".to_string()),
            apple_id: vault_data.apple_id.unwrap_or_default(),
            is_saving: false,
        };

        let sideload = SideloadDialogState {
            is_open: false,
            udid: String::new(),
            apple_id: settings.apple_id.clone(),
            password: String::new(),
            two_factor_code: String::new(),
            two_factor_hint: None,
            two_factor_slot: None,
            progress: 0.0,
            status_message: String::new(),
            is_busy: false,
        };

        let app = Self {
            active_tab: Tab::Devices,
            devices: Vec::new(),
            active_tunnels: HashMap::new(),
            active_heartbeats: HashMap::new(),
            active_bridges: HashMap::new(),
            active_watchdogs: HashMap::new(),
            slot_mgr,
            vault,
            tunnel_supervisor,
            tunnel_status: TunnelStatus::offline(),
            logs: vec![
                LogEntry {
                    level: LogLevel::Info,
                    timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                    message: "Meridian Hub v0.4.0 (Pure Rust) initialized".to_string(),
                }
            ],
            log_level: LogLevel::All,
            log_search: String::new(),
            sideload,
            settings,
            uptime_secs: 0,
        };

        (app, Task::none())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabSelected(tab) => {
                self.active_tab = tab;
            }
            Message::DeviceEvent(event) => match event {
                DeviceEvent::Attached(report) => {
                    info!("Device attached in UI: {}", report.udid);
                    self.add_log(LogLevel::Info, format!("Attached iPhone: {} (UDID: {})", report.name, report.udid));
                    if let Some(pos) = self.devices.iter().position(|d| d.udid == report.udid) {
                        self.devices[pos] = report.clone();
                    } else {
                        self.devices.push(report.clone());
                    }
                    self.sync_tunnel_slots();
                    // Start continuous cloud presence heartbeat worker for attached device
                    let session_active = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let hb = Arc::new(HeartbeatWorker::start(
                        report.clone(),
                        session_active,
                        None,
                    ));
                    self.active_heartbeats.insert(report.udid.clone(), hb);

                    // Start Action Bridge immediately so apps/icons are available on port 9001
                    let bridge = Arc::new(BridgeServer::start(
                        report.ports.bridge,
                        report.ports.wda,
                        report.udid.clone(),
                        report.device_id,
                    ));
                    self.active_bridges.insert(report.udid.clone(), bridge);
                }
                DeviceEvent::Updated(report) => {
                    if let Some(pos) = self.devices.iter().position(|d| d.udid == report.udid) {
                        // Merge health/capability state only — never clobber the
                        // app-level session phase or a live session's message.
                        self.devices[pos].apply_health(&report);
                    }
                }
                DeviceEvent::Detached(udid) => {
                    info!("Device detached in UI: {}", udid);
                    self.add_log(LogLevel::Warn, format!("Detached iPhone: {}", udid));
                    self.devices.retain(|d| d.udid != udid);
                    self.sync_tunnel_slots();
                    self.active_tunnels.remove(&udid);
                    if let Some(hb) = self.active_heartbeats.remove(&udid) {
                        hb.stop();
                    }
                    if let Some(b) = self.active_bridges.remove(&udid) {
                        b.stop();
                    }
                    if let Some(w) = self.active_watchdogs.remove(&udid) {
                        w.store(false, Ordering::SeqCst);
                    }
                    let udid_clone = udid.clone();
                    tokio::spawn(async move {
                        send_offline_sync(&udid_clone, None).await;
                    });
                }
            },
            Message::StartDevice(udid) => {
                let dev_opt = self.devices.iter().find(|d| d.udid == udid).cloned();
                if let Some(dev) = dev_opt {
                    if !dev.runner_installed() {
                        self.add_log(LogLevel::Warn, "Cannot start session: MeridianRunner is not installed on this device. Please sideload first.".to_string());
                        if let Some(d) = self.devices.iter_mut().find(|d| d.udid == udid) {
                            d.state = DeviceState::NeedsSideload;
                            d.status_message = "Runner not installed".to_string();
                        }
                        return Task::none();
                    }

                    if let Some(d) = self.devices.iter_mut().find(|d| d.udid == udid) {
                        d.session_phase = SessionPhase::Starting;
                        d.status_message = "Binding tunnels and launching runner...".to_string();
                    }
                    let ports = dev.ports;
                    let dev_id = dev.device_id;

                    self.add_log(LogLevel::Info, format!("Starting session for {} on WDA :{}, Stream :{}", udid, ports.wda, ports.stream));

                    return Task::perform(async move {
                        // 1. Start tunnels for WDA (8100) and Stream (9200)
                        let wda_tun = start_tunnel(ports.wda, 8100, dev_id, udid.clone()).await;
                        let stream_tun = start_tunnel(ports.stream, 9200, dev_id, udid.clone()).await;

                        let mut tunnels = Vec::new();
                        if let Ok(t) = wda_tun { tunnels.push(Arc::new(t)); }
                        if let Ok(t) = stream_tun { tunnels.push(Arc::new(t)); }

                        // 2. Launch MeridianRunner app in pure Rust via CoreDevice / DVT
                        let launch_res = launch_meridian_runner(udid.clone(), dev_id, ports.stream, None).await;

                        (udid, tunnels, launch_res)
                    }, |(udid, tunnels, launch_res)| {
                        match launch_res {
                            Ok(_) => Message::DeviceStarted(udid, tunnels),
                            Err(e) => Message::DeviceLaunchFailed(udid, e.to_string(), tunnels),
                        }
                    });
                }
            }
            Message::DeviceStarted(udid, tunnels) => {
                self.active_tunnels.insert(udid.clone(), tunnels);
                if let Some(dev) = self.devices.iter_mut().find(|d| d.udid == udid) {
                    dev.session_phase = SessionPhase::Running;
                    dev.state = DeviceState::Ready;
                    dev.status_message = format!("Live Streaming on :{}", dev.ports.stream);
                    let ports = dev.ports;
                    let device_id = dev.device_id;
                    let stream_port = dev.ports.stream;
                    let udid_clone = udid.clone();
                    if let Some(hb) = self.active_heartbeats.get(&udid) {
                        hb.set_session_active(true);
                    }
                    tokio::spawn(async move {
                        sync_session_state(&udid_clone, true, Some(ports), None).await;
                    });

                    // Watchdog: relaunch the runner if its stream dies so the
                    // session is perpetually available.
                    let active = Arc::new(AtomicBool::new(true));
                    self.active_watchdogs.insert(udid.clone(), active.clone());
                    let (wu, wd, wp) = (udid.clone(), device_id, stream_port);
                    tokio::spawn(async move {
                        stream_watchdog(wu, wd, wp, active).await;
                    });
                }
                self.add_log(LogLevel::Info, format!("✓ Meridian session LIVE for {}", udid));
            }
            Message::DeviceLaunchFailed(udid, err, _tunnels) => {
                if let Some(dev) = self.devices.iter_mut().find(|d| d.udid == udid) {
                    dev.session_phase = SessionPhase::Idle;
                    dev.state = DeviceState::Error;
                    dev.status_message = err.clone();
                }
                self.add_log(LogLevel::Error, format!("Failed to start session on {}: {}", udid, err));
            }
            Message::StopDevice(udid) => {
                self.add_log(LogLevel::Info, format!("Stopping session for {}", udid));
                if let Some(tunnels) = self.active_tunnels.remove(&udid) {
                    for t in tunnels {
                        t.stop();
                    }
                }
                if let Some(b) = self.active_bridges.remove(&udid) {
                    b.stop();
                }
                if let Some(hb) = self.active_heartbeats.get(&udid) {
                    hb.set_session_active(false);
                }
                if let Some(w) = self.active_watchdogs.remove(&udid) {
                    w.store(false, Ordering::SeqCst);
                }
                if let Some(dev) = self.devices.iter_mut().find(|d| d.udid == udid) {
                    dev.session_phase = SessionPhase::Idle;
                    dev.status_message = "Stopped".to_string();
                }
                let dev_id = self.devices.iter().find(|d| d.udid == udid).map(|d| d.device_id).unwrap_or(0);
                let udid_clone = udid.clone();
                return Task::perform(async move {
                    // 1. Immediately inform cloud database that session stopped & clear host_ports
                    sync_session_state(&udid_clone, false, None, None).await;
                    // 2. Close the iOS runner app via CoreDevice
                    let _ = kill_meridian_runner(udid_clone, dev_id, None).await;
                    // 3. Navigate to homescreen via WDA
                    let wda = WdaClient::new("http://127.0.0.1:8100");
                    let _ = wda.homescreen().await;
                }, |_| Message::Tick);
            }
            Message::OpenSideload(udid) => {
                self.sideload.is_open = true;
                self.sideload.udid = udid;
                self.sideload.progress = 0.0;
                self.sideload.is_busy = false;
                self.sideload.status_message.clear();
                self.sideload.two_factor_hint = None;
                self.sideload.two_factor_slot = None;
                self.sideload.password.clear();
                // Prefill with the vault/settings Apple ID (auto-managed).
                self.sideload.apple_id = self.settings.apple_id.clone();
            }
            Message::SideloadAppleIdChanged(id) => {
                self.sideload.apple_id = id;
            }
            Message::SideloadPasswordChanged(pwd) => {
                self.sideload.password = pwd;
            }
            Message::SideloadTwoFactorCodeChanged(code) => {
                self.sideload.two_factor_code = code;
            }
            Message::SideloadTwoFactor((hint, slot)) => {
                self.sideload.two_factor_hint = Some(hint);
                self.sideload.two_factor_slot = Some(slot);
                self.sideload.two_factor_code.clear();
            }
            Message::SubmitTwoFactor => {
                if let Some(slot) = self.sideload.two_factor_slot.take() {
                    *slot.lock().unwrap() = Some(self.sideload.two_factor_code.clone());
                }
                self.sideload.two_factor_code.clear();
                self.sideload.two_factor_hint = None;
            }
            Message::SubmitSideload => {
                // Resolve credentials: reuse the vault-stored password when the
                // field was left blank; never re-prompt when it isn't needed.
                let apple_id = self.sideload.apple_id.trim().to_string();
                if apple_id.is_empty() {
                    self.sideload.status_message = "Enter your Apple ID to continue.".to_string();
                    return Task::none();
                }
                let password = if self.sideload.password.is_empty() {
                    self.vault.load().password.unwrap_or_default()
                } else {
                    self.sideload.password.clone()
                };
                if password.is_empty() {
                    self.sideload.status_message =
                        "Enter your Apple ID password (or app-specific password).".to_string();
                    return Task::none();
                }

                self.sideload.is_busy = true;
                self.sideload.progress = 0.0;
                self.sideload.status_message = "Fetching latest Runner from VPS...".to_string();
                let opts = SideloadOptions {
                    apple_id: apple_id.clone(),
                    password,
                    anisette_url: self.settings.anisette_url.clone(),
                    udid: self.sideload.udid.clone(),
                    device_id: 1,
                };
                self.settings.apple_id = apple_id;

                return Task::perform(async move {
                    Sideloader::execute_sideload(opts, |p, s| {
                        info!("[sideload] {:.0}% {s}", p * 100.0);
                        let _ = sideload_progress_tx().send((p, s.to_string()));
                    })
                    .await
                    .map_err(|e| e.to_string())
                }, Message::SideloadFinished);
            }
            Message::SideloadProgress(progress, status) => {
                self.sideload.progress = progress;
                self.sideload.status_message = status;
            }
            Message::SideloadFinished(res) => {
                self.sideload.is_busy = false;
                match res {
                    Ok(_) => {
                        // Persist the freshly-authenticated credentials so the
                        // same login is reused automatically on both platforms.
                        if !self.sideload.apple_id.is_empty() {
                            let mut data = self.vault.load();
                            data.apple_id = Some(self.sideload.apple_id.clone());
                            if !self.sideload.password.is_empty() {
                                data.password = Some(self.sideload.password.clone());
                            }
                            data.anisette_url = Some(self.settings.anisette_url.clone());
                            if let Err(e) = self.vault.save(&data) {
                                warn!("Failed to persist credentials to vault: {e}");
                            } else {
                                info!("✓ Apple credentials saved to secure local vault");
                            }
                            self.settings.apple_id = self.sideload.apple_id.clone();
                        }

                        self.sideload.status_message = "Installation succeeded!".to_string();
                        self.sideload.is_open = false;
                        self.sideload.password.clear();
                        self.add_log(LogLevel::Info, format!("Sideload successful for {}", self.sideload.udid));
                        if let Some(dev) = self.devices.iter_mut().find(|d| d.udid == self.sideload.udid) {
                            dev.health.runner_installed = crate::device::models::TriState::Yes;
                            dev.state = health_state(&dev.health);
                            dev.status_message = "Sideload succeeded. Detecting device state...".to_string();
                        }
                    }
                    Err(e) => {
                        self.sideload.status_message = format!("Error: {}", e);
                        self.add_log(LogLevel::Error, format!("Sideload failed: {}", e));
                    }
                }
            }
            Message::CloseSideload => {
                self.sideload.is_open = false;
            }
            Message::ToggleMask(val) => {
                self.settings.mask_sensitive = val;
            }
            Message::AnisetteChanged(val) => {
                self.settings.anisette_url = val;
            }
            Message::AppleIdChanged(val) => {
                self.settings.apple_id = val;
            }
            Message::SavePreferences => {
                let mut data = self.vault.load();
                data.sensitive_data_masked = self.settings.mask_sensitive;
                data.anisette_url = Some(self.settings.anisette_url.clone());
                data.apple_id = Some(self.settings.apple_id.clone());
                let _ = self.vault.save(&data);
                self.add_log(LogLevel::Info, "Preferences saved to secure local vault".to_string());
            }
            Message::SelectLogLevel(lvl) => {
                self.log_level = lvl;
            }
            Message::LogSearchChanged(query) => {
                self.log_search = query;
            }
            Message::ClearLogs => {
                self.logs.clear();
            }
            Message::CopyLogs => {
                let text: String = self
                    .logs
                    .iter()
                    .map(|e| format!("[{}] {}", e.timestamp, e.message))
                    .collect::<Vec<_>>()
                    .join("\n");
                return iced::clipboard::write(text);
            }
            Message::AddLog(level, msg) => {
                self.add_log(level, msg);
            }
            Message::Tick => {
                self.uptime_secs += 1;
                let tunnel_sup = self.tunnel_supervisor.clone();
                return Task::perform(async move {
                    tunnel_sup.get_status().await
                }, Message::TunnelUpdated);
            }
            Message::TunnelUpdated(status) => {
                self.tunnel_status = status;
            }
            Message::TitleAction(action) => match action {
                TitleBarAction::Minimize => {
                    return window::latest().and_then(|id| window::minimize(id, true));
                }
                TitleBarAction::Close => {
                    return window::latest().and_then(window::close);
                }
                TitleBarAction::Maximize => {
                    return window::latest().and_then(window::toggle_maximize);
                }
            },
        }

        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let timer = iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick);
        let monitor_sub = Subscription::run(monitor_subscription);
        let two_factor_sub = Subscription::run(two_factor_subscription);
        let progress_sub = Subscription::run(sideload_progress_subscription);
        Subscription::batch(vec![timer, monitor_sub, two_factor_sub, progress_sub])
    }

    pub fn view(&self) -> Element<'_, Message> {
        let titlebar = view_titlebar(
            self.tunnel_status.is_running,
            self.tunnel_status.latency_ms,
            Message::TitleAction,
        );

        let tab_btn = |tab: Tab, label: &'static str| {
            let active = self.active_tab == tab;
            button(text(label).font(FONT_MEDIUM).size(12))
                .padding([6, 14])
                .on_press(Message::TabSelected(tab))
                .style(move |theme, status| style_tab_button(theme, status, active))
        };

        let nav_bar = container(
            row![
                tab_btn(Tab::Devices, "Devices"),
                tab_btn(Tab::Status, "Status & Tunnel"),
                tab_btn(Tab::Logs, "Console Logs"),
                tab_btn(Tab::Settings, "Settings"),
            ]
            .spacing(4)
        )
        .padding([4, 14]);

        let tab_content: Element<'_, Message> = match self.active_tab {
            Tab::Devices => view_devices(
                &self.devices,
                self.settings.mask_sensitive,
                Message::StartDevice,
                Message::StopDevice,
                Message::OpenSideload,
            ),
            Tab::Status => view_status(
                &self.tunnel_status,
                &self.devices,
                self.uptime_secs,
                self.settings.mask_sensitive,
            ),
            Tab::Logs => view_logs(
                &self.logs,
                self.log_level,
                &self.log_search,
                Message::SelectLogLevel,
                Message::LogSearchChanged,
                Message::ClearLogs,
                Message::CopyLogs,
            ),
            Tab::Settings => view_settings(
                &self.settings,
                Message::ToggleMask,
                Message::AnisetteChanged,
                Message::AppleIdChanged,
                Message::SavePreferences,
            ),
        };

        let body = container(tab_content)
            .padding([8, 16])
            .width(Length::Fill)
            .height(Length::Fill);

        let window_layout = container(
            column![
                titlebar,
                nav_bar,
                body,
            ]
        )
        .style(style_window_background)
        .width(Length::Fill)
        .height(Length::Fill);

        if self.sideload.is_open {
            let modal = view_sideload_modal(
                &self.sideload,
                Message::SideloadAppleIdChanged,
                Message::SideloadPasswordChanged,
                Message::SideloadTwoFactorCodeChanged,
                Message::SubmitTwoFactor,
                Message::SubmitSideload,
                Message::CloseSideload,
            );
            stack![window_layout, modal].into()
        } else {
            window_layout.into()
        }
    }

    fn add_log(&mut self, level: LogLevel, message: String) {
        if self.logs.len() > 500 {
            self.logs.remove(0);
        }
        self.logs.push(LogEntry {
            level,
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            message,
        });
    }

    fn sync_tunnel_slots(&self) {
        let slots: Vec<u16> = self.devices.iter().map(|d| d.ports.slot).collect();
        self.tunnel_supervisor.update_slots(slots);
    }
}

fn monitor_subscription() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::SinkExt;
    iced::stream::channel(100, |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let slot_mgr = SlotManager::new();
        let monitor = DeviceMonitor::new(slot_mgr, tx.clone());
        monitor.start();

        // Shared view of attached devices: udid -> last known report.
        let devices: Arc<std::sync::Mutex<HashMap<String, DeviceReport>>> = Arc::default();
        // Debounce state: candidate reports waiting for a second stable read.
        let pending: Arc<std::sync::Mutex<HashMap<String, DeviceReport>>> = Arc::default();

        // Real-time health poller: re-probes every 3s and emits an Updated
        // event only once a change is confirmed stable (twice in a row), so the
        // UI reflects install/uninstall, Developer Mode and lock state changes
        // automatically — with zero flicker.
        {
            let devices = devices.clone();
            let pending = pending.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    let snapshots: Vec<DeviceReport> = {
                        devices.lock().unwrap().values().cloned().collect()
                    };
                    for rep in snapshots {
                        let candidate = poll_once(&rep).await;
                        let key = candidate.udid.clone();
                        let mut dev_map = devices.lock().unwrap();
                        let mut pend_map = pending.lock().unwrap();
                        match pend_map.get(&key) {
                            Some(prev)
                                if *prev == candidate && dev_map.get(&key) != Some(&candidate) =>
                            {
                                // Confirmed stable change — emit it.
                                if let Some(cur) = dev_map.get_mut(&key) {
                                    *cur = candidate.clone();
                                }
                                pend_map.remove(&key);
                                debug!("[HEALTH] {} → {}", key, candidate.state.label());
                                let _ = tx.send(DeviceEvent::Updated(candidate));
                            }
                            Some(_) => {
                                // Still conflicting; wait for a stable read.
                            }
                            None => {
                                if dev_map.get(&key) != Some(&candidate) {
                                    pend_map.insert(key, candidate);
                                }
                            }
                        }
                    }
                }
            });
        }

        while let Some(evt) = rx.recv().await {
            match &evt {
                DeviceEvent::Attached(r) => {
                    devices.lock().unwrap().insert(r.udid.clone(), r.clone());
                }
                DeviceEvent::Updated(r) => {
                    devices.lock().unwrap().insert(r.udid.clone(), r.clone());
                }
                DeviceEvent::Detached(udid) => {
                    devices.lock().unwrap().remove(udid);
                    pending.lock().unwrap().remove(udid);
                }
            }
            let _ = output.send(Message::DeviceEvent(evt)).await;
        }
    })
}

/// One health probe pass over a device snapshot, honoring the session guard:
/// while a session is Starting/Running it is only downgraded on *hard* health
/// evidence (device locked, runner uninstalled, or Developer Mode explicitly
/// off); transient probe blips can never kill a live session's UI.
async fn poll_once(rep: &DeviceReport) -> DeviceReport {
    let mut candidate = rep.clone();
    let health = probe_health(&rep.udid, rep.device_id, os_major(&rep.os_version)).await;
    let derived = health_state(&health);

    let state = if matches!(
        rep.session_phase,
        SessionPhase::Starting | SessionPhase::Running
    ) {
        if health.locked.is_yes() {
            DeviceState::Locked
        } else if health.runner_installed.is_no() {
            DeviceState::NeedsSideload
        } else if health.developer_mode.is_no() {
            DeviceState::DeveloperModeOff
        } else {
            rep.state
        }
    } else {
        derived
    };

    candidate.health = health;
    candidate.state = state;
    if rep.session_phase == SessionPhase::Idle {
        candidate.status_message = status_message(&health);
    }
    candidate
}

/// Perpetual-availability watchdog: probes the on-device runner's HTTP server and,
/// if it stops responding (app jetsam'd / crashed), relaunches it so the stream and
/// WDA come back without user intervention.
async fn stream_watchdog(udid: String, device_id: u32, stream_port: u16, active: Arc<AtomicBool>) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
        .unwrap_or_default();
    let url = format!("http://127.0.0.1:{stream_port}/status");
    let mut failures = 0u32;

    while active.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_secs(5)).await;
        if !active.load(Ordering::SeqCst) {
            break;
        }

        let healthy = matches!(
            client.get(&url).send().await,
            Ok(resp) if resp.status().is_success()
        );

        if healthy {
            failures = 0;
        } else {
            failures += 1;
            warn!("[WATCH] runner unresponsive for {udid} ({failures}/3)");
            if failures >= 3 {
                info!("[WATCH] relaunching MeridianRunner for {udid}");
                let _ = launch_meridian_runner(udid.clone(), device_id, stream_port, None).await;
                failures = 0;
            }
        }
    }
}

/// Stream 2FA prompt requests from the sideload login flow into the UI.
fn two_factor_subscription() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::SinkExt;
    iced::stream::channel(
        1,
        |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            // Seed the channel eagerly so the receiver exists before the login
            // flow ever sends a prompt. Otherwise `next_two_factor_prompt` sees an
            // uninitialized channel at startup, returns None, and this loop exits
            // before the 2FA prompt is ever delivered.
            let _ = two_factor_tx();
            loop {
                if let Some(prompt) = next_two_factor_prompt().await {
                    let _ = output.send(Message::SideloadTwoFactor(prompt)).await;
                }
            }
        },
    )
}

/// Stream live sideload progress (fetch → sign → install) into the modal's
/// progress bar.
fn sideload_progress_subscription() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::SinkExt;
    iced::stream::channel(
        1,
        |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            let _ = sideload_progress_tx();
            loop {
                if let Some((p, s)) = next_sideload_progress().await {
                    let _ = output.send(Message::SideloadProgress(p, s)).await;
                }
            }
        },
    )
}
