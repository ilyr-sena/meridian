//! Sideloading pipeline: signing with zsign and installation over usbmuxd.
//!
//! Provides native file picking via `rfd` and real-time execution with progress reporting.

use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{debug, error, info};

#[derive(Debug, Clone)]
pub struct SideloadOptions {
    pub ipa_path: PathBuf,
    pub apple_id: String,
    pub password: String,
    pub anisette_url: String,
    pub udid: String,
    pub device_id: u32,
}

pub struct Sideloader;

impl Sideloader {
    /// Open native OS file dialog to select an IPA file.
    pub async fn pick_ipa_file() -> Option<PathBuf> {
        let file = rfd::AsyncFileDialog::new()
            .set_title("Select MeridianRunner IPA")
            .add_filter("iOS App Package (*.ipa)", &["ipa"])
            .pick_file()
            .await;

        file.map(|f| f.path().to_path_buf())
    }

    /// Sign and install the IPA onto the target iPhone over USB.
    pub async fn execute_sideload(
        opts: SideloadOptions,
        progress_callback: impl Fn(f32, &str) + Send + 'static,
    ) -> anyhow::Result<()> {
        progress_callback(0.05, "Validating IPA package...");

        if !opts.ipa_path.exists() {
            anyhow::bail!("Selected IPA file does not exist: {:?}", opts.ipa_path);
        }

        info!("Starting sideload process for {} on {}", opts.ipa_path.display(), opts.udid);

        let script_path = find_sideload_engine();
        if script_path.is_none() {
            anyhow::bail!("Sideload engine helper not found in bin/");
        }
        let script_path = script_path.unwrap();

        let python_bin = find_python_executable();

        let mut cmd = Command::new(&python_bin);
        cmd.arg(&script_path);
        cmd.arg("--ipa").arg(&opts.ipa_path);
        cmd.arg("--udid").arg(&opts.udid);
        cmd.arg("--apple-id").arg(&opts.apple_id);
        cmd.arg("--password").arg(&opts.password);
        cmd.arg("--anisette").arg(&opts.anisette_url);

        #[cfg(windows)]
        {
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn()?;

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let mut reader = BufReader::new(stdout).lines();
        let mut last_error = String::new();

        while let Ok(Some(line)) = reader.next_line().await {
            debug!("[sideload] {}", line);
            if line.starts_with("[PROGRESS:") {
                if let Some(end) = line.find(']') {
                    let pct_str = &line[10..end];
                    if let Ok(pct) = pct_str.parse::<f32>() {
                        let msg = line[end + 1..].trim();
                        progress_callback(pct / 100.0, msg);
                    }
                }
            } else if line.starts_with("[ERROR]") {
                last_error = line[7..].trim().to_string();
                error!("Sideload engine error: {}", last_error);
            }
        }

        let status = child.wait().await?;
        if !status.success() {
            let mut err_reader = BufReader::new(stderr).lines();
            let mut err_output = Vec::new();
            while let Ok(Some(line)) = err_reader.next_line().await {
                err_output.push(line);
            }
            let err_combined = if !last_error.is_empty() {
                last_error
            } else if !err_output.is_empty() {
                err_output.join("\n")
            } else {
                format!("Process exited with status code: {}", status)
            };
            anyhow::bail!("{}", err_combined);
        }

        progress_callback(1.0, "Installation complete!");
        info!("✓ Sideload successful for device {}", opts.udid);

        Ok(())
    }
}

fn find_sideload_engine() -> Option<PathBuf> {
    let script_name = "sideload-engine.py";

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join(script_name);
            if candidate.exists() { return Some(candidate); }
            let candidate_bin = parent.join("bin").join(script_name);
            if candidate_bin.exists() { return Some(candidate_bin); }
            if let Some(grandparent) = parent.parent() {
                let candidate_up_bin = grandparent.join("bin").join(script_name);
                if candidate_up_bin.exists() { return Some(candidate_up_bin); }
            }
        }
    }

    for rel_dir in &["bin", "apps/hub-rust/bin", "apps/hub/bin"] {
        let candidate = PathBuf::from(rel_dir).join(script_name);
        if candidate.exists() { return Some(candidate); }
    }

    None
}

fn find_python_executable() -> String {
    // 1. Check user provision-venv (preferred on Linux/Windows developer machines)
    if let Some(home) = dirs::home_dir() {
        let venv_candidates = [
            home.join("provision-venv/bin/python3"),
            home.join("provision-venv/bin/python"),
            home.join("provision-venv/Scripts/python.exe"),
        ];
        for candidate in venv_candidates {
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }
    }

    // 2. Check local virtualenv in apps/hub/.venv or .venv
    let venv_candidates = if cfg!(windows) {
        vec![
            PathBuf::from("apps/hub/.venv/Scripts/python.exe"),
            PathBuf::from(".venv/Scripts/python.exe"),
        ]
    } else {
        vec![
            PathBuf::from("apps/hub/.venv/bin/python3"),
            PathBuf::from("apps/hub/.venv/bin/python"),
            PathBuf::from(".venv/bin/python3"),
            PathBuf::from(".venv/bin/python"),
        ]
    };
    for candidate in venv_candidates {
        if candidate.exists() {
            return candidate.to_string_lossy().to_string();
        }
    }

    // 3. Check system python3 / python
    if which::which("python3").is_ok() {
        return "python3".to_string();
    }
    "python".to_string()
}
