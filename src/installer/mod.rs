use anyhow::{bail, Context, Result};
use log::{error, info, warn};
use std::env;
use std::path::{Path, PathBuf};
use tokio::fs;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

pub async fn install() -> Result<()> {
    info!("Starting autolocalhost installation...");

    // Check privileges
    check_privileges()?;

    // Check if running from target directory
    let current_exe = env::current_exe().context("Failed to get current executable path")?;
    let install_dir = get_install_dir();

    if current_exe.parent() == Some(&install_dir) {
        bail!("Cannot install from target directory. Please run from a different location.");
    }

    // Stop service if running
    if is_service_running().await? {
        info!("Stopping existing service...");
        stop_service().await?;
    }

    // Create directories
    create_directories().await?;

    // Copy executable
    copy_executable(&current_exe).await?;

    // Copy nginx template
    copy_nginx_template().await?;

    // Install service
    install_service().await?;

    // Enable autostart
    enable_autostart().await?;

    // Start the service
    start_service().await?;

    info!("Autolocalhost installation completed successfully!");
    info!("The service has been started and will start automatically on system boot");

    Ok(())
}

pub async fn uninstall() -> Result<()> {
    info!("Starting autolocalhost uninstallation...");

    // Clean up nginx container first
    cleanup_nginx_container().await;

    // Stop and remove service
    if is_service_running().await? {
        info!("Stopping service...");
        stop_service().await?;
    }

    uninstall_service().await?;

    // Remove executable (best effort)
    let install_path = get_install_dir().join(get_executable_name());
    if install_path.exists() {
        if let Err(e) = fs::remove_file(&install_path).await {
            warn!(
                "Failed to remove executable {}: {}",
                install_path.display(),
                e
            );
            warn!("You may need to remove it manually after reboot");
        } else {
            info!("Removed executable: {}", install_path.display());
        }
    }

    info!("Autolocalhost uninstallation completed");
    info!("Configuration and data directories were preserved");

    Ok(())
}

async fn create_directories() -> Result<()> {
    let config_dir = get_config_dir();
    let data_dir = get_data_dir();
    let certs_dir = get_certs_dir();
    let ca_dir = get_ca_dir();
    let log_dir = get_log_dir();
    let nginx_log_dir = get_nginx_log_dir();
    let install_dir = get_install_dir();

    fs::create_dir_all(&install_dir).await.with_context(|| {
        format!(
            "Failed to create install directory {}",
            install_dir.display()
        )
    })?;
    info!("Created install directory: {}", install_dir.display());

    fs::create_dir_all(&config_dir).await.with_context(|| {
        format!(
            "Failed to create config directory: {}",
            config_dir.display()
        )
    })?;
    info!("Created config directory: {}", config_dir.display());

    fs::create_dir_all(&data_dir)
        .await
        .with_context(|| format!("Failed to create data directory: {}", data_dir.display()))?;
    info!("Created data directory: {}", certs_dir.display());

    fs::create_dir_all(&certs_dir)
        .await
        .with_context(|| format!("Failed to create certs directory: {}", certs_dir.display()))?;
    info!("Created certs directory: {}", certs_dir.display());

    fs::create_dir_all(&ca_dir)
        .await
        .with_context(|| format!("Failed to create ca directory: {}", ca_dir.display()))?;
    info!("Created ca directory: {}", ca_dir.display());

    fs::create_dir_all(&log_dir)
        .await
        .with_context(|| format!("Failed to create log directory: {}", log_dir.display()))?;
    info!("Created log directory: {}", log_dir.display());

    fs::create_dir_all(&nginx_log_dir).await.with_context(|| {
        format!(
            "Failed to create nginx log directory: {}",
            nginx_log_dir.display()
        )
    })?;
    info!("Created nginx log directory: {}", nginx_log_dir.display());

    Ok(())
}

async fn copy_executable(source: &Path) -> Result<()> {
    let target = get_install_dir().join(get_executable_name());

    // Try to copy, handle file in use error
    match fs::copy(source, &target).await {
        Ok(_) => {
            info!("Copied executable to: {}", target.display());
            Ok(())
        }
        Err(e) => {
            error!("Failed to copy executable to {}: {}", target.display(), e);
            bail!("Failed to install executable. Service may be running or file is in use.");
        }
    }
}

async fn copy_nginx_template() -> Result<()> {
    let source = Path::new("nginx.template.conf");
    let target = get_config_dir().join("nginx.template.conf");

    if source.exists() {
        fs::copy(source, &target)
            .await
            .with_context(|| format!("Failed to copy nginx template to {}", target.display()))?;
        info!("Copied nginx template to: {}", target.display());
    } else {
        warn!("nginx.template.conf not found in current directory, skipping copy");
    }

    Ok(())
}

async fn cleanup_nginx_container() {
    info!("Cleaning up managed nginx container...");

    // Try to connect to Docker
    let docker = match try_connect_docker().await {
        Ok(client) => client,
        Err(e) => {
            warn!("Failed to connect to Docker, skipping container cleanup: {}", e);
            return;
        }
    };

    // Create container manager and stop/remove containers
    let nginx_manager = crate::nginx::container_manager::ContainerManager::new(docker);
    match nginx_manager.stop_and_remove().await {
        Ok(count) => {
            if count > 0 {
                info!("Removed {} nginx container(s)", count);
            } else {
                info!("No nginx containers to remove");
            }
        }
        Err(e) => {
            warn!("Failed to remove nginx containers: {}", e);
        }
    }
}

async fn try_connect_docker() -> Result<bollard::Docker> {
    use bollard::Docker;

    let docker = if cfg!(windows) {
        Docker::connect_with_http_defaults()
            .context("Failed to connect to Docker over HTTP")?
    } else {
        Docker::connect_with_socket_defaults()
            .context("Failed to connect to Docker socket")?
    };

    // Test the connection
    docker.version().await
        .context("Docker connection test failed")?;

    Ok(docker)
}

pub fn get_install_dir() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\Program Files\Autolocalhost")
    } else {
        PathBuf::from("/usr/sbin")
    }
}

pub fn get_config_dir() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".to_string()))
            .join("Autolocalhost")
    } else {
        PathBuf::from("/etc/autolocalhost")
    }
}

pub fn get_data_dir() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".to_string()))
            .join("Autolocalhost")
    } else {
        PathBuf::from("/var/lib/autolocalhost")
    }
}

pub fn get_certs_dir() -> PathBuf {
    return get_data_dir().join("certs");
}

pub fn get_ca_dir() -> PathBuf {
    return get_data_dir().join("ca");
}

pub fn get_log_dir() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".to_string()))
            .join("Autolocalhost")
            .join("log")
    } else {
        PathBuf::from("/var/log/autolocalhost")
    }
}

pub fn get_nginx_log_dir() -> PathBuf {
    return get_log_dir().join("nginx");
}

fn get_executable_name() -> &'static str {
    if cfg!(windows) {
        "autolocalhost.exe"
    } else {
        "autolocalhost"
    }
}

// Platform-specific implementations
#[cfg(unix)]
async fn is_service_running() -> Result<bool> {
    unix::is_service_running().await
}

#[cfg(unix)]
async fn stop_service() -> Result<()> {
    unix::stop_service().await
}

#[cfg(unix)]
async fn install_service() -> Result<()> {
    unix::install_service().await
}

#[cfg(unix)]
async fn uninstall_service() -> Result<()> {
    unix::uninstall_service().await
}

#[cfg(unix)]
async fn enable_autostart() -> Result<()> {
    unix::enable_autostart().await
}

#[cfg(unix)]
async fn start_service() -> Result<()> {
    unix::start_service().await
}

#[cfg(windows)]
async fn is_service_running() -> Result<bool> {
    windows::is_service_running().await
}

#[cfg(windows)]
async fn stop_service() -> Result<()> {
    windows::stop_service().await
}

#[cfg(windows)]
async fn install_service() -> Result<()> {
    windows::install_service().await
}

#[cfg(windows)]
async fn uninstall_service() -> Result<()> {
    windows::uninstall_service().await
}

#[cfg(windows)]
async fn enable_autostart() -> Result<()> {
    windows::enable_autostart().await
}

#[cfg(windows)]
async fn start_service() -> Result<()> {
    windows::start_service().await
}

// Platform-specific privilege checking
#[cfg(unix)]
fn check_privileges() -> Result<()> {
    unix::check_privileges()
}

#[cfg(windows)]
fn check_privileges() -> Result<()> {
    windows::check_privileges()
}
