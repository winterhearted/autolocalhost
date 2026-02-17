pub mod container_info;

use anyhow::{Result, anyhow};
use bollard::Docker;
use bollard::container::ListContainersOptions;
use bollard::system::EventsOptions;
use crate::hosts::HostsFileManager;
use crate::nginx::config_generator::ConfigGenerator;
use crate::nginx::container_manager::ContainerManager;
use crate::ssl::certificate_generator::CertificateGenerator;
use container_info::ContainerInfo;
use futures_util::StreamExt;
use log::{info, error, warn};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::oneshot::Receiver;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration, Instant};
use std::env;

const TARGET_LABEL: &str = "kz.byte0.autolocalhost.enabled";
const DEBOUNCE_DURATION_SECS: u64 = 5;

/// Connect to Docker API based on the current platform
/// Will retry connection every 15 seconds until successful
pub async fn connect_docker() -> Result<Docker> {
    const RETRY_INTERVAL_SECS: u64 = 15;
    let mut attempt_count = 1;

    loop {
        let connection_result = if cfg!(windows) {
            info!("Windows detected, attempting to connect to Docker TCP (attempt {})", attempt_count);
            Docker::connect_with_http_defaults()
                .map_err(|e| anyhow!("Failed to connect to Docker over HTTP: {}", e))
        } else {
            let socket_path = env::var("DOCKER_SOCKET").unwrap_or_else(|_| "/var/run/docker.sock".to_string());
            info!("Unix-based system detected, attempting to connect to Docker socket: {} (attempt {})",
                  socket_path, attempt_count);
            Docker::connect_with_socket_defaults()
                .map_err(|e| anyhow!("Failed to connect to Docker socket: {}", e))
        };

        match connection_result {
            Ok(docker_client) => {
                // Test the connection by making a simple API call
                match docker_client.version().await {
                    Ok(_) => {
                        info!("Successfully connected to Docker API after {} attempt(s)", attempt_count);
                        return Ok(docker_client);
                    },
                    Err(e) => {
                        warn!("Connected to Docker but API is not ready: {}. Retrying in {} seconds...",
                              e, RETRY_INTERVAL_SECS);
                    }
                }
            },
            Err(e) => {
                warn!("Failed to connect to Docker (attempt {}): {}. Retrying in {} seconds...",
                      attempt_count, e, RETRY_INTERVAL_SECS);
            }
        }

        attempt_count += 1;

        // Use tokio sleep to avoid blocking the async runtime
        tokio::time::sleep(std::time::Duration::from_secs(RETRY_INTERVAL_SECS)).await;
    }
}

/// State for debouncing configuration updates
struct DebounceState {
    last_update_request: Option<Instant>,
    pending_update: bool,
}

/// Monitor Docker containers for events
pub async fn monitor_containers(docker: Arc<Docker>, shutdown_rx: Receiver<()>) -> Result<()> {
    let mut active_containers = HashMap::new();
    let debounce_state = Arc::new(Mutex::new(DebounceState {
        last_update_request: None,
        pending_update: false,
    }));

    // First, get all existing containers with our label
    let mut filters = HashMap::new();
    filters.insert("label".to_string(), vec![format!("{}=true", TARGET_LABEL).to_string()]);

    let options = ListContainersOptions {
        all: true,
        filters: filters.clone(),
        ..Default::default()
    };

    info!("Scanning for existing containers with label {}=true", TARGET_LABEL);
    let containers = docker.list_containers(Some(options)).await?;

    for container in containers {
        let id = match container.id {
            Some(id) => id,
            None => continue,
        };

        info!("Found container: {}", id);
        match ContainerInfo::from_container(&docker, &id).await {
            Ok(container_info) => {
                active_containers.insert(id, container_info);
            },
            Err(e) => {
                warn!("Failed to get container info for {}: {}", id, e);
            }
        }
    }

    // Update configuration based on initial containers
    update_configuration(&docker, &active_containers).await?;

    // Set up event monitoring
    let mut event_filters = HashMap::new();
    event_filters.insert("type".to_string(), vec!["container".to_string()]);
    event_filters.insert("event".to_string(), vec!["start".to_string(), "stop".to_string(), "die".to_string(), "destroy".to_string()]);
    event_filters.insert("label".to_string(), vec![format!("{}=true", TARGET_LABEL).to_string()]);

    let opts = EventsOptions {
        filters: event_filters,
        ..Default::default()
    };

    info!("Starting Docker events monitoring");
    let mut events = docker.events(Some(opts));
    let mut shutdown_future = shutdown_rx;

    // Spawn debounce task
    let docker_clone = docker.clone();
    let active_containers_arc = Arc::new(Mutex::new(active_containers.clone()));
    let active_containers_for_task = active_containers_arc.clone();
    let debounce_state_clone = debounce_state.clone();

    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(1)).await;

            let mut state = debounce_state_clone.lock().await;
            if state.pending_update {
                if let Some(last_request) = state.last_update_request {
                    if last_request.elapsed() >= Duration::from_secs(DEBOUNCE_DURATION_SECS) {
                        info!("Debounce period elapsed, triggering configuration update");
                        state.pending_update = false;
                        state.last_update_request = None;
                        drop(state);

                        let containers = active_containers_for_task.lock().await;
                        if let Err(e) = update_configuration(&docker_clone, &containers).await {
                            error!("Failed to update configuration: {}", e);
                        }
                    }
                }
            }
        }
    });

    loop {
        tokio::select! {
            Some(event_result) = events.next() => {
                match event_result {
                    Ok(event) => {
                        if let Some(actor) = event.actor {
                            if let Some(id) = actor.id {
                                if let Some(action) = event.action {
                                    info!("Container event: {} - {}", id, action);

                                    let mut state_changed = false;

                                    match action.as_str() {
                                        "start" => {
                                            // Check if container is already in active list
                                            if !active_containers.contains_key(&id) {
                                                match ContainerInfo::from_container(&docker, &id).await {
                                                    Ok(container_info) => {
                                                        active_containers.insert(id.clone(), container_info);
                                                        state_changed = true;
                                                        info!("Container {} added to active list", id);
                                                    },
                                                    Err(e) => warn!("Failed to get container info: {}", e)
                                                }
                                            } else {
                                                info!("Container {} already in active list, ignoring start event", id);
                                            }
                                        },
                                        "stop" | "die" | "destroy" => {
                                            // Check if container is actually in active list before removing
                                            if active_containers.contains_key(&id) {
                                                active_containers.remove(&id);
                                                state_changed = true;
                                                info!("Container {} removed from active list", id);
                                            } else {
                                                info!("Container {} already removed from active list, ignoring {} event", id, action);
                                            }
                                        },
                                        _ => {}
                                    }

                                    // Request configuration update only if state actually changed
                                    if state_changed {
                                        // Update the shared containers state
                                        let mut shared_containers = active_containers_arc.lock().await;
                                        *shared_containers = active_containers.clone();
                                        drop(shared_containers);

                                        // Request debounced update
                                        let mut state = debounce_state.lock().await;
                                        state.last_update_request = Some(Instant::now());
                                        state.pending_update = true;
                                        info!("Configuration update scheduled (debounced)");
                                    }
                                }
                            }
                        }
                    },
                    Err(e) => {
                        error!("Error in Docker events stream: {}", e);
                    }
                }
            },
            _ = &mut shutdown_future => {
                info!("Shutting down container monitoring");
                break;
            }
        }
    }

    Ok(())
}

/// Update configuration based on active containers
async fn update_configuration(docker: &Docker, containers: &HashMap<String, ContainerInfo>) -> Result<()> {
    info!("Updating configuration with {} containers", containers.len());

    // Filter out containers that aren't running
    let running_containers: Vec<ContainerInfo> = containers.values()
        .filter(|c| c.is_running)
        .cloned()
        .collect();

    // Extract domains for hosts file
    let mut domains = Vec::new();
    let mut external_ports = HashSet::new();

    for container in &running_containers {
        // Check for duplicate domains
        if domains.contains(&container.domain) {
            return Err(anyhow!("Duplicate domain name in container {}", container.name));
        }

        // Add domain to list
        if !container.domain.is_empty() {
            domains.push(container.domain.clone());
        }

        // Collect all external ports from container
        for port in &container.ports {
            external_ports.insert(port.external);
        }

        for ssl_port in &container.ssl_ports {
            external_ports.insert(ssl_port.external);

            // Generate SSL certificate if needed
            if !container.domain.is_empty() {
                let cert_gen = CertificateGenerator::new(&container.domain);
                if let Err(e) = cert_gen.generate_certificates().await {
                    warn!("Failed to generate SSL certificate for {}: {}", container.domain, e);
                }
            }
        }
    }

    // Update hosts file
    let hosts_manager = HostsFileManager::new(None);
    if let Err(e) = hosts_manager.update_managed_block(&domains).await {
        warn!("Failed to update hosts file: {}", e);
    }

    // Generate NGINX config
    let config_generator = ConfigGenerator::new(&running_containers);
    let nginx_config_path = crate::installer::get_data_dir().join("nginx.conf");
    if let Err(e) = config_generator.generate_config(nginx_config_path.to_str().unwrap()).await {
        warn!("Failed to generate NGINX config: {}", e);
    }

    // Convert HashSet to Vec for NGINX container manager
    let ports: Vec<u16> = external_ports.into_iter().collect();

    // Start NGINX container
    let nginx_manager = ContainerManager::new(docker.clone());
    if let Err(e) = nginx_manager.create_and_start(&ports).await {
        warn!("Failed to manage NGINX container: {}", e);
    }

    info!("Configuration updated successfully");
    Ok(())
}
