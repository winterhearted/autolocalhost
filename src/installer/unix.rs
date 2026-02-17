use anyhow::{Result, Context, bail};
use log::{info, warn};
use tokio::fs;
use tokio::process::Command as AsyncCommand;
use nix::libc;

const SERVICE_NAME: &str = "autolocalhost";
const SERVICE_FILE_CONTENT: &str = r#"[Unit]
Description=Autolocalhost - Local development environment automation
After=network.target docker.service
Requires=network.target
Wants=docker.service

[Service]
Type=simple
User=root
ExecStart=/usr/sbin/autolocalhost start
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal
SyslogIdentifier=autolocalhost

# Security settings
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ReadWritePaths=/etc/hosts /var/lib/autolocalhost /var/log/autolocalhost /etc/autolocalhost
ProtectHome=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes

[Install]
WantedBy=multi-user.target
"#;

pub async fn is_service_running() -> Result<bool> {
    let output = AsyncCommand::new("systemctl")
    .args(["is-active", "--quiet", SERVICE_NAME])
    .output()
    .await
    .context("Failed to check service status")?;

    Ok(output.status.success())
}

pub async fn stop_service() -> Result<()> {
    let output = AsyncCommand::new("systemctl")
    .args(["stop", SERVICE_NAME])
    .output()
    .await
    .context("Failed to stop service")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("Failed to stop service: {}", stderr);
    } else {
        info!("Service stopped successfully");
    }

    Ok(())
}

pub async fn install_service() -> Result<()> {
    let service_path = format!("/etc/systemd/system/{}.service", SERVICE_NAME);

    // Write service file
    fs::write(&service_path, SERVICE_FILE_CONTENT).await
    .with_context(|| format!("Failed to write service file: {}", service_path))?;

    info!("Created systemd service file: {}", service_path);

    // Reload systemd
    let output = AsyncCommand::new("systemctl")
    .arg("daemon-reload")
    .output()
    .await
    .context("Failed to reload systemd")?;

    if !output.status.success() {
        bail!("Failed to reload systemd daemon");
    }

    info!("Reloaded systemd daemon");
    Ok(())
}

pub async fn uninstall_service() -> Result<()> {
    // Disable service
    let _ = AsyncCommand::new("systemctl")
    .args(["disable", SERVICE_NAME])
    .output()
    .await;

    // Remove service file
    let service_path = format!("/etc/systemd/system/{}.service", SERVICE_NAME);
    if let Err(e) = fs::remove_file(&service_path).await {
        warn!("Failed to remove service file {}: {}", service_path, e);
    } else {
        info!("Removed service file: {}", service_path);
    }

    // Reload systemd
    let _ = AsyncCommand::new("systemctl")
    .arg("daemon-reload")
    .output()
    .await;

    info!("Service uninstalled");
    Ok(())
}

pub async fn enable_autostart() -> Result<()> {
    let output = AsyncCommand::new("systemctl")
    .args(["enable", SERVICE_NAME])
    .output()
    .await
    .context("Failed to enable service")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to enable service autostart: {}", stderr);
    }

    info!("Service autostart enabled");
    Ok(())
}

pub async fn start_service() -> Result<()> {
    let output = AsyncCommand::new("systemctl")
    .args(["start", SERVICE_NAME])
    .output()
    .await
    .context("Failed to start service")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to start service: {}", stderr);
    }

    info!("Service started successfully");
    Ok(())
}

// Check if we're running as root
pub fn check_privileges() -> Result<()> {
    unsafe {
        if libc::geteuid() != 0 {
            bail!("Installation requires root privileges. Please run with sudo.");
        }
    }

    Ok(())
}
