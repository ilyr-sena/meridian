//! Tailscale Userspace Mesh Supervisor (`meridian-mesh`).
//!
//! Manages the perpetual Go sidecar process for zero-leak VPN connectivity.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::core::vault::Vault;
use crate::remote::key_fetcher::KeyFetcher;

#[derive(Debug, Clone)]
pub struct MeshStatus {
    pub is_running: bool,
    pub mesh_ip: Option<String>,
    pub status_text: String,
}

pub struct MeshSupervisor {
    vault: Vault,
    status: Arc<Mutex<MeshStatus>>,
    child: Arc<Mutex<Option<Child>>>,
    should_run: Arc<AtomicBool>,
}

impl MeshSupervisor {
    pub fn new(vault: Vault) -> Self {
        Self {
            vault,
            status: Arc::new(Mutex::new(MeshStatus {
                is_running: false,
                mesh_ip: None,
                status_text: "Initializing mesh...".to_string(),
            })),
            child: Arc::new(Mutex::new(None)),
            should_run: Arc::new(AtomicBool::new(true)),
        }
    }

    pub async fn get_status(&self) -> MeshStatus {
        let lock = self.status.lock().await;
        lock.clone()
    }

    pub fn start(&self) {
        let vault = self.vault.clone();
        let status = self.status.clone();
        let child_arc = self.child.clone();
        let should_run = self.should_run.clone();

        tokio::spawn(async move {
            let key_fetcher = KeyFetcher::new(vault);

            while should_run.load(Ordering::SeqCst) {
                // 1. Locate meridian-mesh binary
                let bin_path = find_mesh_binary();
                if bin_path.is_none() {
                    let mut st = status.lock().await;
                    st.status_text = "meridian-mesh binary not found in bin/".to_string();
                    warn!("{}", st.status_text);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    continue;
                }
                let bin_path = bin_path.unwrap();

                // 2. Fetch active auth key
                let auth_key = key_fetcher.fetch_active_key().await;

                // 3. Prepare state directory
                let state_dir = dirs::data_local_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("meridian")
                    .join("mesh");
                let _ = std::fs::create_dir_all(&state_dir);

                info!("Starting meridian-mesh sidecar from {:?} ...", bin_path);

                let mut cmd = Command::new(&bin_path);
                cmd.arg("-dir").arg(&state_dir);

                if let Some(ref key) = auth_key {
                    cmd.arg("-authkey").arg(key);
                }

                #[cfg(windows)]
                {
                    // Avoid opening console windows on Windows
                    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
                }

                cmd.stdout(Stdio::piped());
                cmd.stderr(Stdio::piped());

                match cmd.spawn() {
                    Ok(mut spawned) => {
                        {
                            let mut st = status.lock().await;
                            st.is_running = true;
                            st.status_text = "Connecting to Tailscale...".to_string();
                        }

                        let stdout = spawned.stdout.take().unwrap();
                        let stderr = spawned.stderr.take().unwrap();

                        let parse_line = |line: &str, status_ref: Arc<Mutex<MeshStatus>>| {
                            if line.contains("100.") {
                                for word in line.split_whitespace() {
                                    if word.starts_with("100.") && word.split('.').count() == 4 {
                                        let ip = word.trim_matches(|c: char| !c.is_ascii_digit() && c != '.');
                                        let ip_owned = ip.to_string();
                                        let status_task = status_ref.clone();
                                        tokio::spawn(async move {
                                            let mut st = status_task.lock().await;
                                            st.mesh_ip = Some(ip_owned.clone());
                                            st.status_text = format!("Online ({})", ip_owned);
                                            info!("✓ Tailscale Mesh IP assigned: {}", ip_owned);
                                        });
                                    }
                                }
                            }
                        };

                        let status_out = status.clone();
                        tokio::spawn(async move {
                            let mut reader = BufReader::new(stdout).lines();
                            while let Ok(Some(line)) = reader.next_line().await {
                                debug!("[mesh stdout] {}", line);
                                parse_line(&line, status_out.clone());
                            }
                        });

                        let status_err = status.clone();
                        tokio::spawn(async move {
                            let mut reader = BufReader::new(stderr).lines();
                            while let Ok(Some(line)) = reader.next_line().await {
                                debug!("[mesh stderr] {}", line);
                                parse_line(&line, status_err.clone());
                            }
                        });

                        *child_arc.lock().await = Some(spawned);

                        // Wait for process completion
                        while let Some(ref mut c) = *child_arc.lock().await {
                            match c.try_wait() {
                                Ok(Some(exit_status)) => {
                                    warn!("meridian-mesh exited with status: {}", exit_status);
                                    break;
                                }
                                Ok(None) => {
                                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                                }
                                Err(e) => {
                                    error!("Error awaiting meridian-mesh: {:?}", e);
                                    break;
                                }
                            }
                        }

                        {
                            let mut st = status.lock().await;
                            st.is_running = false;
                            st.mesh_ip = None;
                            st.status_text = "Mesh offline. Reconnecting in 3s...".to_string();
                        }
                    }
                    Err(e) => {
                        error!("Failed to spawn meridian-mesh: {:?}", e);
                        let mut st = status.lock().await;
                        st.status_text = format!("Launch failed: {}", e);
                    }
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            }
        });
    }

    pub async fn stop(&self) {
        self.should_run.store(false, Ordering::SeqCst);
        if let Some(mut child) = self.child.lock().await.take() {
            info!("Stopping meridian-mesh sidecar...");
            let _ = child.kill().await;
        }
    }
}

fn find_mesh_binary() -> Option<PathBuf> {
    let binary_name = if cfg!(windows) { "meridian-mesh.exe" } else { "meridian-mesh" };

    // 1. Next to current executable
    if let Ok(exe) = std::env::current_exe() {
        let candidate = exe.parent().unwrap().join(binary_name);
        if candidate.exists() {
            return Some(candidate);
        }
        let candidate_bin = exe.parent().unwrap().join("bin").join(binary_name);
        if candidate_bin.exists() {
            return Some(candidate_bin);
        }
    }

    // 2. Relative to working directory
    let candidate = PathBuf::from("bin").join(binary_name);
    if candidate.exists() {
        return Some(candidate);
    }

    // 3. apps/hub-rust/bin
    let candidate_dev = PathBuf::from("apps/hub-rust/bin").join(binary_name);
    if candidate_dev.exists() {
        return Some(candidate_dev);
    }

    None
}
