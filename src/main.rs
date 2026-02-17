mod docker;
mod hosts;
mod installer;
mod nginx;
mod ssl;
mod utils;

use anyhow::Result;
use clap::{Parser, Subcommand};
use log::{error, info, warn};
use std::sync::Arc;
use tokio::fs;
use tokio::signal;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "autolocalhost")]
#[command(about = "Local development environment automation tool", long_about = None)]
#[command(version = VERSION)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the autolocalhost service
    Start,
    /// Install autolocalhost as a system service
    Install,
    /// Uninstall the autolocalhost system service
    Uninstall,
    /// Show version information
    Version,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start => run_service().await,
        Commands::Install => installer::install().await,
        Commands::Uninstall => installer::uninstall().await,
        Commands::Version => {
            println!("autolocalhost {}", VERSION);
            Ok(())
        }
    }
}

async fn run_service() -> Result<()> {
    // Initialize logger with default configuration
    env_logger::init_from_env(env_logger::Env::default().filter_or("RUST_LOG", "info"));

    info!("Starting autolocalhost service...");

    // Ensure required directories exist
    let config_dir = installer::get_config_dir();
    let data_dir = installer::get_data_dir();
    let certs_dir = installer::get_certs_dir();
    let ca_dir = installer::get_ca_dir();
    let log_dir = installer::get_log_dir();
    let nginx_log_dir = installer::get_nginx_log_dir();

    if let Err(e) = fs::create_dir_all(&config_dir).await {
        warn!(
            "Failed to create config directory {}: {}",
            config_dir.display(),
            e
        );
    }

    if let Err(e) = fs::create_dir_all(&data_dir).await {
        warn!(
            "Failed to create data directory {}: {}",
            data_dir.display(),
            e
        );
    }

    if let Err(e) = fs::create_dir_all(&certs_dir).await {
        warn!(
            "Failed to create certs directory {}: {}",
            certs_dir.display(),
            e
        );
    }

    if let Err(e) = fs::create_dir_all(&ca_dir).await {
        warn!("Failed to create ca directory {}: {}", ca_dir.display(), e);
    }

    if let Err(e) = fs::create_dir_all(&log_dir).await {
        warn!(
            "Failed to create logs directory {}: {}",
            log_dir.display(),
            e
        );
    }

    if let Err(e) = fs::create_dir_all(&nginx_log_dir).await {
        warn!(
            "Failed to create nginx logs directory {}: {}",
            nginx_log_dir.display(),
            e
        );
    }

    // Ensure nginx template exists
    if let Err(e) = nginx::config_generator::ensure_nginx_template_exists().await {
        warn!("Failed to create nginx template: {}", e);
        warn!("Continuing with default nginx template...");
    }

    // Generate DH parameters for SSL if needed
    if let Err(e) = ssl::generate_dhparam_if_needed().await {
        warn!("Failed to generate DH parameters: {}", e);
    }

    // Connect to Docker API
    let docker = match docker::connect_docker().await {
        Ok(client) => {
            info!("Connected to Docker API");
            Arc::new(client)
        }
        Err(err) => {
            error!("Failed to connect to Docker API: {}", err);
            return Err(err);
        }
    };

    // Create a channel for graceful shutdown
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

    // Handle Ctrl+C signal
    tokio::spawn(async move {
        if let Err(e) = signal::ctrl_c().await {
            error!("Failed to listen for ctrl+c signal: {}", e);
        }
        info!("Received shutdown signal, cleaning up...");
        let _ = shutdown_tx.send(());
    });

    // Start monitoring Docker containers
    if let Err(e) = docker::monitor_containers(docker, shutdown_rx).await {
        error!("Error monitoring containers: {}", e);
        return Err(e);
    }

    info!("Autolocalhost service stopped");
    Ok(())
}
