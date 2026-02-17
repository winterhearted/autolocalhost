use anyhow::{Result, anyhow};
use bollard::Docker;
use log::{debug, warn};
use serde::{Serialize, Deserialize};
use crate::utils::port_mapping::PortMapping;

/// Container information structure, roughly equivalent to the Node.js ContainerInfo class
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub is_running: bool,
    pub domain: String,
    pub ports: Vec<PortMapping>,
    pub ssl_ports: Vec<PortMapping>,
}

impl ContainerInfo {
    /// Create a ContainerInfo from a Docker container ID
    pub async fn from_container(docker: &Docker, container_id: &str) -> Result<Self> {
        debug!("Getting details for container {}", container_id);

        let details = docker.inspect_container(container_id, None).await?;

        // Extract container ID
        let id = details.id.ok_or_else(|| anyhow!("Container ID not available"))?;

        // Extract container name (remove leading slash if present)
        let name = match details.name {
            Some(n) => n.trim_start_matches('/').to_string(),
            None => id.clone(),
        };

        // Check if container is running
        let is_running = match details.state {
            Some(state) => state.running.unwrap_or(false),
            None => false,
        };

        // Extract labels from config
        let labels = match details.config {
            Some(config) => match config.labels {
                Some(labels) => labels,
                None => return Err(anyhow!("Container has no labels")),
            },
            None => return Err(anyhow!("Container has no config")),
        };

        // Extract domain from labels
        let domain = match labels.get("kz.byte0.autolocalhost.domain") {
            Some(domain) => domain.clone(),
            None => {
                warn!("Container {} has no domain label", name);
                String::new()
            }
        };

        // Parse port mappings
        let ports_str = labels.get("kz.byte0.autolocalhost.ports")
            .map(|s| s.as_str())
            .unwrap_or("");

        let ports = match PortMapping::parse_port_mappings(ports_str) {
            Ok(ports) => ports,
            Err(e) => {
                warn!("Failed to parse port mappings for {}: {}", name, e);
                Vec::new()
            }
        };

        // Check if SSL is enabled
        let ssl_enabled = labels.get("kz.byte0.autolocalhost.sslEnabled")
            .map(|v| v == "true")
            .unwrap_or(false);

        // Parse SSL port mappings if enabled
        let ssl_ports = if ssl_enabled {
            let ssl_ports_str = labels.get("kz.byte0.autolocalhost.sslPorts")
                .map(|s| s.as_str())
                .unwrap_or("");

            match PortMapping::parse_port_mappings(ssl_ports_str) {
                Ok(ports) => ports,
                Err(e) => {
                    warn!("Failed to parse SSL port mappings for {}: {}", name, e);
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        Ok(ContainerInfo {
            id,
            name,
            is_running,
            domain,
            ports,
            ssl_ports,
        })
    }
}
