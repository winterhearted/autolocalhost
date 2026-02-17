use anyhow::{anyhow, Result};
use bollard::container::{
    Config, CreateContainerOptions, ListContainersOptions, RemoveContainerOptions,
    StartContainerOptions,
};
use bollard::image::{CreateImageOptions, ListImagesOptions};
use bollard::models::{
    HostConfig, Mount, MountTypeEnum, PortBinding, RestartPolicy, RestartPolicyNameEnum,
};
use bollard::network::{CreateNetworkOptions, ListNetworksOptions};
use bollard::Docker;
use futures_util::StreamExt;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

/// Manages the NGINX proxy container
pub struct ContainerManager {
    docker: Docker,
    label: String,
    container_name: String,
    image: String,
    base_dir: PathBuf,
    volume_mounts: Vec<String>,
    restart_policy: RestartPolicyNameEnum,
    network_name: String,
}

impl ContainerManager {
    /// Create a new ContainerManager
    pub fn new(docker: Docker) -> Self {
        let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let data_dir = crate::installer::get_data_dir();
        let certs_dir = crate::installer::get_certs_dir();
        let nginx_log_dir = crate::installer::get_nginx_log_dir();

        let nginx_config_mount = format!(
            "{}:/etc/nginx/nginx.conf:ro",
            data_dir.join("nginx.conf").to_str().unwrap()
        );

        let certs_mount = format!("{}:/etc/ssl/certs:ro", certs_dir.to_str().unwrap());

        let log_mount = format!("{}:/var/log/nginx", nginx_log_dir.to_str().unwrap());

        Self {
            docker,
            label: String::from("kz.byte0.autolocalhost.managed-nginx-container"),
            container_name: String::from("autolocalhost-nginx-container"),
            image: String::from("nginx:latest"),
            base_dir: current_dir,
            volume_mounts: vec![nginx_config_mount, certs_mount, log_mount],
            restart_policy: RestartPolicyNameEnum::UNLESS_STOPPED,
            network_name: String::from("autolocalhost-external-network"),
        }
    }

    /// Create and start the NGINX container with specified ports
    pub async fn create_and_start(&self, ports: &[u16]) -> Result<()> {
        // Ensure the image exists (pull if necessary)
        self.ensure_image_exists().await?;

        // Stop and remove existing containers
        self.stop_and_remove().await?;

        debug!("Creating NGINX container with {} ports", ports.len());

        // Format ports for Docker API
        let mut port_bindings = HashMap::new();
        let mut exposed_ports = HashMap::new();

        for port in ports {
            let port_key = format!("{}/tcp", port);
            exposed_ports.insert(port_key.clone(), HashMap::new());

            let host_binding = vec![PortBinding {
                host_ip: Some(String::from("")),
                host_port: Some(port.to_string()),
            }];

            port_bindings.insert(port_key, Some(host_binding));
        }

        // Ensure the network exists
        self.ensure_network_exists().await?;

        // Format mount points for Docker API
        let mounts = self.prepare_mounts()?;

        // Create labels for container
        let mut labels = HashMap::new();
        labels.insert(self.label.clone(), String::from("true"));

        // Create host config
        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            restart_policy: Some(RestartPolicy {
                name: Some(self.restart_policy.clone()),
                maximum_retry_count: None,
            }),
            mounts: Some(mounts),
            network_mode: Some(self.network_name.clone()),
            ..Default::default()
        };

        // Create container config
        let container_config = Config {
            image: Some(self.image.clone()),
            exposed_ports: Some(exposed_ports),
            host_config: Some(host_config),
            labels: Some(labels),
            ..Default::default()
        };

        // Create container options
        let options = CreateContainerOptions {
            name: self.container_name.clone(),
            platform: None,
        };

        // Create the container
        let response = self
            .docker
            .create_container(Some(options), container_config)
            .await?;

        let warning = response.warnings;
        if !warning.is_empty() {
            for warn_msg in warning {
                warn!("Warning creating container: {}", warn_msg);
            }
        }

        // Start the container
        self.docker
            .start_container(&response.id, None::<StartContainerOptions<String>>)
            .await?;

        info!(
            "NGINX container {} started with ID: {}",
            self.container_name, response.id
        );
        Ok(())
    }

    /// Stop and remove existing managed NGINX containers
    pub async fn stop_and_remove(&self) -> Result<usize> {
        debug!("Stopping and removing existing NGINX containers");

        // Create filter for our labeled containers
        let mut filters = HashMap::new();
        filters.insert(
            "label".to_string(),
            vec![format!("{}=true", self.label).to_string()],
        );

        let options = ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        };

        // Get all containers with our label
        let containers = self.docker.list_containers(Some(options)).await?;

        let mut count = 0;

        for container in containers {
            if let Some(id) = container.id {
                let container_name = container
                    .names
                    .unwrap_or_default()
                    .first()
                    .cloned()
                    .unwrap_or_else(|| String::from("unknown"));

                // Stop container if running
                if container.state == Some(String::from("running")) {
                    info!("Stopping container: {}", container_name);
                    if let Err(e) = self.docker.stop_container(&id, None).await {
                        warn!("Error stopping container {}: {}", id, e);
                    }
                }

                // Remove container
                info!("Removing container: {}", container_name);
                let remove_options = RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                };

                if let Err(e) = self
                    .docker
                    .remove_container(&id, Some(remove_options))
                    .await
                {
                    warn!("Error removing container {}: {}", id, e);
                } else {
                    count += 1;
                }
            }
        }

        info!("Removed {} containers", count);
        Ok(count)
    }

    /// Ensure the network exists
    async fn ensure_network_exists(&self) -> Result<()> {
        // List networks
        let networks = self
            .docker
            .list_networks(None::<ListNetworksOptions<String>>)
            .await?;

        // Check if our network already exists
        for network in networks {
            if network.name == Some(self.network_name.clone()) {
                debug!("Network {} already exists", self.network_name);
                return Ok(());
            }
        }

        // Create the network
        info!("Creating network: {}", self.network_name);

        let mut network_labels = HashMap::new();
        network_labels.insert(self.label.clone(), String::from("true"));

        let options = CreateNetworkOptions {
            name: self.network_name.clone(),
            driver: String::from("bridge"),
            labels: network_labels,
            ..Default::default()
        };

        self.docker.create_network(options).await?;
        info!("Network {} created", self.network_name);

        Ok(())
    }

    /// Ensure the Docker image exists, pull if necessary
    async fn ensure_image_exists(&self) -> Result<()> {
        // Parse image name and tag
        let (image_name, tag) = if self.image.contains(':') {
            let parts: Vec<&str> = self.image.splitn(2, ':').collect();
            (parts[0], parts[1])
        } else {
            (self.image.as_str(), "latest")
        };

        // Check if image already exists locally
        let mut filters = HashMap::new();
        filters.insert("reference".to_string(), vec![self.image.clone()]);

        let options = ListImagesOptions {
            filters,
            ..Default::default()
        };

        let images = self.docker.list_images(Some(options)).await?;

        if !images.is_empty() {
            debug!("Image {} already exists locally", self.image);
            return Ok(());
        }

        // Pull the image
        info!("Pulling image: {}", self.image);

        let pull_options = CreateImageOptions {
            from_image: image_name,
            tag,
            ..Default::default()
        };

        let mut stream = self.docker.create_image(Some(pull_options), None, None);

        while let Some(result) = stream.next().await {
            match result {
                Ok(info) => {
                    if let Some(status) = info.status {
                        debug!("Pull status: {}", status);
                    }
                }
                Err(e) => {
                    return Err(anyhow!("Failed to pull image {}: {}", self.image, e));
                }
            }
        }

        info!("Successfully pulled image: {}", self.image);
        Ok(())
    }

    /// Prepare mount points for Docker API
    fn prepare_mounts(&self) -> Result<Vec<Mount>> {
        let mut mounts = Vec::new();

        for mount_str in &self.volume_mounts {
            let parts: Vec<&str> = mount_str.split(':').collect();

            if parts.len() < 2 {
                return Err(anyhow!("Invalid mount format: {}", mount_str));
            }

            let source = parts[0];
            let target = parts[1];
            let readonly = parts.get(2) == Some(&"ro");

            // Convert relative paths to absolute
            let source_path = if Path::new(source).is_absolute() {
                PathBuf::from(source)
            } else {
                self.base_dir.join(source)
            };

            mounts.push(Mount {
                target: Some(target.to_string()),
                source: Some(source_path.to_string_lossy().to_string()),
                typ: Some(MountTypeEnum::BIND),
                read_only: Some(readonly),
                ..Default::default()
            });
        }

        Ok(mounts)
    }
}
