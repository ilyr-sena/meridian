//! Main Iced application lifecycle and state orchestration.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use iced::{
    widget::{button, column, container, row, stack, text},
    window, Element, Length, Subscription, Task,
};
use tokio::sync::mpsc;
use tracing::info;

use crate::core::slots::SlotManager;
use crate::core::vault::Vault;
use crate::device::launcher::{kill_meridian_runner, launch_meridian_runner};
use crate::device::models::{DeviceReport, DeviceState};
use crate::device::monitor::{DeviceEvent, DeviceMonitor};
use crate::device::tunnel::{start_tunnel, ActiveTunnel};
use crate::remote::heartbeat::HeartbeatWorker;
use crate::remote::key_fetcher::KeyFetcher;
use crate::remote::mesh::{MeshStatus, MeshSupervisor};
use crate::sideload::sideloader::{SideloadOptions, Sideloader};
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
    DeviceStarted(String, Vec<Arc<ActiveTunnel>>, Option<Arc<HeartbeatWorker>>),
    DeviceLaunchFailed(String, String, Vec<Arc<ActiveTunnel>>),
    StopDevice(String),
    OpenSideload(String),
    BrowseIpa,
    IpaFileSelected(Option<PathBuf>),
    SideloadAppleIdChanged(String),
    SideloadPasswordChanged(String),
    SubmitSideload,
    SideloadProgress(f32, String),
    SideloadFinished(Result<(), String>),
    CloseSideload,
    ToggleMask(bool),
    KeyUrlChanged(String),
    AuthKeyChanged(String),
    AnisetteChanged(String),
    AppleIdChanged(String),
    RefreshKeyNow,
    KeyRefreshed(Option<String>),
    SavePreferences,
    SelectLogLevel(LogLevel),
    LogSearchChanged(String),
    ClearLogs,
    AddLog(LogLevel, String),
    Tick,
    MeshUpdated(MeshStatus),
    TitleAction(TitleBarAction),
}

pub struct MeridianApp {
    active_tab: Tab,
    devices: Vec<DeviceReport>,
    active_tunnels: HashMap<String, Vec<Arc<ActiveTunnel>>>,
    active_heartbeats: HashMap<String, Arc<HeartbeatWorker>>,
    slot_mgr: SlotManager,
    vault: Vault,
    mesh_supervisor: Arc<MeshSupervisor>,
    mesh_status: MeshStatus,
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
        let mesh_supervisor = Arc::new(MeshSupervisor::new(vault.clone()));
        mesh_supervisor.start();

        let settings = SettingsState {
            mask_sensitive: vault_data.sensitive_data_masked,
            tailscale_key_url: vault_data.tailscale_key_url.unwrap_or_else(|| "https://meridianhub.cc/api/mesh/authkey".to_string()),
            tailscale_auth_key: vault_data.tailscale_auth_key.unwrap_or_default(),
            anisette_url: vault_data.anisette_url.unwrap_or_else(|| "http://100.51.75.20:6969".to_string()),
            apple_id: vault_data.apple_id.unwrap_or_default(),
            is_saving: false,
        };

        let sideload = SideloadDialogState {
            is_open: false,
            udid: String::new(),
            ipa_path: None,
            apple_id: settings.apple_id.clone(),
            password: String::new(),
            progress: 0.0,
            status_message: String::new(),
            is_busy: false,
        };

        let app = Self {
            active_tab: Tab::Devices,
            devices: Vec::new(),
            active_tunnels: HashMap::new(),
            active_heartbeats: HashMap::new(),
            slot_mgr,
            vault,
            mesh_supervisor,
            mesh_status: MeshStatus {
                is_running: false,
                mesh_ip: None,
                status_text: "Initializing mesh...".to_string(),
            },
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
                        self.devices[pos] = report;
                    } else {
                        self.devices.push(report);
                    }
                }
                DeviceEvent::Updated(report) => {
                    if let Some(pos) = self.devices.iter().position(|d| d.udid == report.udid) {
                        self.devices[pos] = report;
                    }
                }
                DeviceEvent::Detached(udid) => {
                    info!("Device detached in UI: {}", udid);
                    self.add_log(LogLevel::Warn, format!("Detached iPhone: {}", udid));
                    self.devices.retain(|d| d.udid != udid);
                    self.active_tunnels.remove(&udid);
                    self.active_heartbeats.remove(&udid);
                }
            },
            Message::StartDevice(udid) => {
                let dev_opt = self.devices.iter().find(|d| d.udid == udid).cloned();
                if let Some(dev) = dev_opt {
                    if !dev.runner_installed {
                        self.add_log(LogLevel::Warn, "Cannot start session: MeridianRunner is not installed on this device. Please sideload first.".to_string());
                        if let Some(d) = self.devices.iter_mut().find(|d| d.udid == udid) {
                            d.state = DeviceState::NeedsSideload;
                            d.status_message = "Runner not installed".to_string();
                        }
                        return Task::none();
                    }

                    if let Some(d) = self.devices.iter_mut().find(|d| d.udid == udid) {
                        d.state = DeviceState::Starting;
                        d.status_message = "Binding tunnels and launching runner...".to_string();
                    }
                    let ports = dev.ports;
                    let dev_id = dev.device_id;
                    let dev_copy = dev.clone();
                    let mesh_ip = self.mesh_status.mesh_ip.clone();

                    self.add_log(LogLevel::Info, format!("Starting session for {} on WDA :{}, Stream :{}", udid, ports.wda, ports.stream));

                    return Task::perform(async move {
                        // 1. Start tunnels for WDA and Stream
                        let wda_tun = start_tunnel(ports.wda, 8100, dev_id, udid.clone()).await;
                        let stream_tun = start_tunnel(ports.stream, 9200, dev_id, udid.clone()).await;

                        let mut tunnels = Vec::new();
                        if let Ok(t) = wda_tun { tunnels.push(Arc::new(t)); }
                        if let Ok(t) = stream_tun { tunnels.push(Arc::new(t)); }

                        // 2. Launch MeridianRunner app in pure Rust via CoreDevice / DVT
                        let launch_res = launch_meridian_runner(udid.clone(), dev_id, ports.stream, None).await;

                        // 3. Start Heartbeat worker if launched successfully
                        let hb = if launch_res.is_ok() {
                            Some(Arc::new(HeartbeatWorker::start(dev_copy, mesh_ip, None)))
                        } else {
                            None
                        };

                        (udid, tunnels, hb, launch_res)
                    }, |(udid, tunnels, hb, launch_res)| {
                        match launch_res {
                            Ok(_) => Message::DeviceStarted(udid, tunnels, hb),
                            Err(e) => Message::DeviceLaunchFailed(udid, e.to_string(), tunnels),
                        }
                    });
                }
            }
            Message::DeviceStarted(udid, tunnels, hb) => {
                self.active_tunnels.insert(udid.clone(), tunnels);
                if let Some(worker) = hb {
                    self.active_heartbeats.insert(udid.clone(), worker);
                }
                if let Some(dev) = self.devices.iter_mut().find(|d| d.udid == udid) {
                    dev.state = DeviceState::Running;
                    dev.status_message = format!("Live Streaming on :{}", dev.ports.stream);
                }
                self.add_log(LogLevel::Info, format!("✓ Meridian session LIVE for {}", udid));
            }
            Message::DeviceLaunchFailed(udid, err, _tunnels) => {
                if let Some(dev) = self.devices.iter_mut().find(|d| d.udid == udid) {
                    dev.state = DeviceState::Error;
                    dev.status_message = err.clone();
                }
                self.add_log(LogLevel::Error, format!("Failed to start session on {}: {}", udid, err));
            }
            Message::StopDevice(udid) => {
                self.add_log(LogLevel::Info, format!("Stopping session for {}", udid));
                self.active_tunnels.remove(&udid);
                self.active_heartbeats.remove(&udid);
                if let Some(dev) = self.devices.iter_mut().find(|d| d.udid == udid) {
                    dev.state = DeviceState::Ready;
                    dev.status_message = "Ready".to_string();
                }
                let dev_id = self.devices.iter().find(|d| d.udid == udid).map(|d| d.device_id).unwrap_or(0);
                let udid_clone = udid.clone();
                return Task::perform(async move {
                    let _ = kill_meridian_runner(udid_clone, dev_id, None).await;
                }, |_| Message::Tick);
            }
            Message::OpenSideload(udid) => {
                self.sideload.is_open = true;
                self.sideload.udid = udid;
                self.sideload.progress = 0.0;
                self.sideload.is_busy = false;
                self.sideload.status_message.clear();
            }
            Message::BrowseIpa => {
                return Task::perform(async {
                    Sideloader::pick_ipa_file().await
                }, Message::IpaFileSelected);
            }
            Message::IpaFileSelected(path) => {
                if let Some(p) = path {
                    self.sideload.ipa_path = Some(p);
                }
            }
            Message::SideloadAppleIdChanged(id) => {
                self.sideload.apple_id = id;
            }
            Message::SideloadPasswordChanged(pwd) => {
                self.sideload.password = pwd;
            }
            Message::SubmitSideload => {
                self.sideload.is_busy = true;
                self.sideload.status_message = "Starting sideload...".to_string();
                let opts = SideloadOptions {
                    ipa_path: self.sideload.ipa_path.clone().unwrap_or_else(|| PathBuf::from("runner/prebuilt/MeridianRunner-unsigned.ipa")),
                    apple_id: self.sideload.apple_id.clone(),
                    password: self.sideload.password.clone(),
                    anisette_url: self.settings.anisette_url.clone(),
                    udid: self.sideload.udid.clone(),
                    device_id: 1,
                };

                return Task::perform(async move {
                    Sideloader::execute_sideload(opts, |_, _| {}).await
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
                        self.sideload.status_message = "Installation succeeded!".to_string();
                        self.sideload.is_open = false;
                        self.add_log(LogLevel::Info, format!("Sideload successful for {}", self.sideload.udid));
                        if let Some(dev) = self.devices.iter_mut().find(|d| d.udid == self.sideload.udid) {
                            dev.runner_installed = true;
                            dev.state = DeviceState::Ready;
                            dev.status_message = format!("Ready (Slot {})", dev.ports.slot);
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
            Message::KeyUrlChanged(val) => {
                self.settings.tailscale_key_url = val;
            }
            Message::AuthKeyChanged(val) => {
                self.settings.tailscale_auth_key = val;
            }
            Message::AnisetteChanged(val) => {
                self.settings.anisette_url = val;
            }
            Message::AppleIdChanged(val) => {
                self.settings.apple_id = val;
            }
            Message::RefreshKeyNow => {
                let vault = self.vault.clone();
                return Task::perform(async move {
                    let fetcher = KeyFetcher::new(vault);
                    fetcher.fetch_active_key().await
                }, Message::KeyRefreshed);
            }
            Message::KeyRefreshed(opt) => {
                if let Some(key) = opt {
                    self.settings.tailscale_auth_key = key;
                    self.add_log(LogLevel::Info, "✓ Tailscale auth key refreshed from remote endpoint".to_string());
                } else {
                    self.add_log(LogLevel::Warn, "Could not fetch active auth key from endpoint".to_string());
                }
            }
            Message::SavePreferences => {
                let mut data = self.vault.load();
                data.sensitive_data_masked = self.settings.mask_sensitive;
                data.tailscale_key_url = Some(self.settings.tailscale_key_url.clone());
                data.tailscale_auth_key = if self.settings.tailscale_auth_key.is_empty() { None } else { Some(self.settings.tailscale_auth_key.clone()) };
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
            Message::AddLog(level, msg) => {
                self.add_log(level, msg);
            }
            Message::Tick => {
                self.uptime_secs += 1;
                let mesh_sup = self.mesh_supervisor.clone();
                return Task::perform(async move {
                    mesh_sup.get_status().await
                }, Message::MeshUpdated);
            }
            Message::MeshUpdated(status) => {
                self.mesh_status = status;
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
        Subscription::batch(vec![timer, monitor_sub])
    }

    pub fn view(&self) -> Element<'_, Message> {
        let titlebar = view_titlebar(
            self.mesh_status.mesh_ip.as_deref(),
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
                tab_btn(Tab::Status, "Status & Mesh"),
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
                &self.mesh_status,
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
            ),
            Tab::Settings => view_settings(
                &self.settings,
                Message::ToggleMask,
                Message::KeyUrlChanged,
                Message::AuthKeyChanged,
                Message::AnisetteChanged,
                Message::AppleIdChanged,
                Message::RefreshKeyNow,
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
                Message::BrowseIpa,
                Message::SideloadAppleIdChanged,
                Message::SideloadPasswordChanged,
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
}

fn monitor_subscription() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::SinkExt;
    iced::stream::channel(100, |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let slot_mgr = SlotManager::new();
        let monitor = DeviceMonitor::new(slot_mgr, tx);
        monitor.start();

        while let Some(evt) = rx.recv().await {
            let _ = output.send(Message::DeviceEvent(evt)).await;
        }
    })
}
