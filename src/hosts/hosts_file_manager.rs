use anyhow::{Result, anyhow};
use log::{info, warn, debug};
use regex::Regex;
use std::env;
use std::path::{Path, PathBuf};
use tokio::fs;

/// Manages the hosts file entries for local domains
pub struct HostsFileManager {
    hosts_file_path: PathBuf,
    block_start: String,
    block_end: String,
}

impl HostsFileManager {
    /// Create a new HostsFileManager
    pub fn new(hosts_file_path: Option<PathBuf>) -> Self {
        let hosts_file_path = hosts_file_path.unwrap_or_else(|| Self::get_system_hosts_file_path());

        Self {
            hosts_file_path,
            block_start: String::from("# BEGIN MANAGED BLOCK - DO NOT EDIT MANUALLY # kz.byte0.autolocalhost"),
            block_end: String::from("# END MANAGED BLOCK - DO NOT EDIT MANUALLY # kz.byte0.autolocalhost"),
        }
    }

    /// Get the path to the system hosts file
    fn get_system_hosts_file_path() -> PathBuf {
        if cfg!(windows) {
            let system_root = env::var("SYSTEMROOT").unwrap_or_else(|_| String::from("C:\\Windows"));
            Path::new(&system_root)
            .join("System32")
            .join("drivers")
            .join("etc")
            .join("hosts")
        } else {
            PathBuf::from("/etc/hosts")
        }
    }

    /// Update the managed block in the hosts file
    pub async fn update_managed_block(&self, domains: &[String]) -> Result<()> {
        // Filter out "localhost" from domains
        let domains: Vec<String> = domains.iter()
        .filter(|domain| *domain != "localhost")
        .cloned()
        .collect();

        debug!("Updating hosts file at {}", self.hosts_file_path.display());

        // Read current content of hosts file
        let content = match fs::read_to_string(&self.hosts_file_path).await {
            Ok(content) => content,
            Err(e) => return Err(anyhow!("Failed to read hosts file: {}", e)),
        };

        // Update the content
        let updated_content = self.update_block_in_content(&content, &domains);

        // Write the updated content back to the file
        match fs::write(&self.hosts_file_path, updated_content).await {
            Ok(_) => {
                info!("Hosts file updated successfully at {}", self.hosts_file_path.display());
                Ok(())
            },
            Err(e) => {
                warn!("Failed to write hosts file: {}. This may require administrator/root privileges.", e);
                Err(anyhow!("Failed to write hosts file: {}. This may require administrator/root privileges.", e))
            }
        }
    }

    /// Update or create the managed block in the hosts file content
    fn update_block_in_content(&self, content: &str, domains: &[String]) -> String {
        // Pattern to find the block including possible empty lines before and after
        let block_pattern = format!(
            r"\n*{}[\s\S]*?{}\n*",
            regex::escape(&self.block_start),
                                    regex::escape(&self.block_end)
        );

        let re = Regex::new(&block_pattern).unwrap();

        // Create new block if domains are provided
        let new_block = if !domains.is_empty() {
            self.create_managed_block(domains)
        } else {
            String::new()
        };

        if re.is_match(content) {
            if !new_block.is_empty() {
                // Replace existing block with new one
                let result = re.replace(content, &new_block).to_string();
                self.normalize_content(&result)
            } else {
                // Remove the block if domains are empty
                let result = re.replace(content, "").to_string();
                self.normalize_content(&result)
            }
        } else if !new_block.is_empty() {
            // Add block to the end of the file
            let normalized_content = self.normalize_content(content);

            // Ensure there's one empty line before the block
            if normalized_content.is_empty() {
                new_block
            } else if normalized_content.ends_with('\n') {
                format!("{}\n{}", normalized_content, new_block)
            } else {
                format!("{}\n\n{}", normalized_content, new_block)
            }
        } else {
            // No changes needed
            self.normalize_content(content)
        }
    }

    /// Create the managed block with domain entries
    fn create_managed_block(&self, domains: &[String]) -> String {
        let mut block = format!("{}\n", self.block_start);

        for domain in domains {
            block.push_str(&format!("127.0.0.1 {}\n", domain));
        }

        block.push_str(&self.block_end);
        block
    }

    /// Normalize content by removing excessive empty lines and ensuring proper ending
    fn normalize_content(&self, content: &str) -> String {
        // Remove multiple consecutive empty lines and normalize
        let lines: Vec<&str> = content.lines().collect();
        let mut normalized_lines = Vec::new();
        let mut prev_was_empty = false;

        for line in lines {
            let is_empty = line.trim().is_empty();

            if is_empty && prev_was_empty {
                // Skip multiple consecutive empty lines
                continue;
            }

            normalized_lines.push(line);
            prev_was_empty = is_empty;
        }

        // Remove empty lines at the end
        while let Some(last) = normalized_lines.last() {
            if last.trim().is_empty() {
                normalized_lines.pop();
            } else {
                break;
            }
        }

        // Form result with one newline at the end
        if normalized_lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", normalized_lines.join("\n"))
        }
    }
}
