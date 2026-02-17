use anyhow::{Result, anyhow};
use serde::{Serialize, Deserialize};
use log::debug;

/// Port mapping structure to handle internal/external port mappings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMapping {
    pub external: u16,
    pub internal: u16,
}

impl PortMapping {
    /// Create a new port mapping
    pub fn new(external: u16, internal: u16) -> Self {
        Self { external, internal }
    }

    /// Parse a single port mapping string (e.g., "8080" or "8080:80")
    pub fn parse_port_mapping(mapping_str: &str) -> Result<Self> {
        let trimmed = mapping_str.trim();

        if trimmed.is_empty() {
            return Err(anyhow!("Empty port mapping"));
        }

        if trimmed.contains(':') {
            let parts: Vec<&str> = trimmed.split(':').collect();

            if parts.len() != 2 {
                return Err(anyhow!("Invalid port mapping format: {}", trimmed));
            }

            let external = Self::validate_port(parts[0])?;
            let internal = Self::validate_port(parts[1])?;

            Ok(PortMapping::new(external, internal))
        } else {
            // If only one port is specified, use it for both external and internal
            let port = Self::validate_port(trimmed)?;
            Ok(PortMapping::new(port, port))
        }
    }

    /// Validate a port number
    pub fn validate_port(port_str: &str) -> Result<u16> {
        let port = port_str.trim().parse::<u16>()
            .map_err(|_| anyhow!("Invalid port number: {}", port_str))?;

        if port < 1 {
            return Err(anyhow!("Port number out of range (1-65535): {}", port));
        }

        Ok(port)
    }

    /// Parse a comma-separated list of port mappings
    pub fn parse_port_mappings(mappings_str: &str) -> Result<Vec<Self>> {
        if mappings_str.is_empty() {
            debug!("Empty port mappings string, returning empty vector");
            return Ok(Vec::new());
        }

        let mut mappings = Vec::new();

        for mapping_str in mappings_str.split(',') {
            match Self::parse_port_mapping(mapping_str) {
                Ok(mapping) => mappings.push(mapping),
                Err(e) => return Err(anyhow!("Failed to parse port mapping '{}': {}", mapping_str, e)),
            }
        }

        Ok(mappings)
    }
}

