//! Firewall rule provisioning for Meridian ports.
//!
//! Windows: provisions rules for WDA (8100-8131), Stream (9200-9231),
//! Remote Bridge (9001-9032), and Tunneld (49151) silently when elevated.


pub fn ensure_firewall_rules() {
    #[cfg(windows)]
    {
        use std::process::Command;
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x08000000;

        info!("Verifying Windows Firewall rules for Meridian ports...");

        let rules = [
            ("Meridian-WDA", "8100-8131", "TCP"),
            ("Meridian-Stream", "9200-9231", "TCP"),
            ("Meridian-Bridge", "9001-9032", "TCP"),
            ("Meridian-Tunneld", "49151", "TCP"),
        ];

        for (name, port_range, protocol) in rules {
            let mut cmd = Command::new("netsh");
            cmd.args([
                "advfirewall", "firewall", "add", "rule",
                &format!("name={name}"),
                "dir=in",
                "action=allow",
                &format!("protocol={protocol}"),
                &format!("localport={port_range}"),
                "profile=any",
            ]);
            cmd.creation_flags(CREATE_NO_WINDOW);

            match cmd.output() {
                Ok(out) if out.status.success() => {
                    info!("✓ Windows Firewall rule configured: {name} ({port_range})");
                }
                Ok(out) => {
                    let err = String::from_utf8_lossy(&out.stderr);
                    warn!("Firewall configuration returned non-zero for {name}: {err}");
                }
                Err(e) => {
                    warn!("Failed to execute netsh for {name}: {e}");
                }
            }
        }
    }

    #[cfg(not(windows))]
    {
        // On Linux / macOS, loopback ports require no external firewall rule manipulation
    }
}
