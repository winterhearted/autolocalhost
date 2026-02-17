use anyhow::Result;
use log::{debug, info};
use tokio::fs;
use tokio::process::Command;

/// Generate DH parameters for SSL
pub async fn generate_dhparam_if_needed() -> Result<()> {
    let certs_dir = crate::installer::get_certs_dir();
    let dhparam_path = certs_dir.join("dhparams.crt");

    // Check if file already exists
    if dhparam_path.exists() {
        debug!("DH parameters file already exists");
        return Ok(());
    }

    info!("Generating DH parameters (this may take a while)...");

    // Ensure certs directory exists
    fs::create_dir_all(&certs_dir).await?;

    // Try to use openssl command to generate DH params
    let dhparam_str = dhparam_path.to_string_lossy();
    let output = Command::new("openssl")
        .args(["dhparam", "-out", &dhparam_str, "2048"])
        .output()
        .await;

    match output {
        Ok(output) => {
            if output.status.success() {
                info!(
                    "DH parameters generated successfully at: {}",
                    dhparam_path.display()
                );
                Ok(())
            } else {
                let error = String::from_utf8_lossy(&output.stderr);
                info!("Failed to generate DH parameters: {}", error);

                // Provide a basic DH params file as fallback
                info!("Using pre-generated DH parameters as fallback");
                let default_dhparams = include_bytes!("../../assets/dhparams.crt");
                fs::write(&dhparam_path, default_dhparams).await?;
                info!(
                    "Fallback DH parameters written to: {}",
                    dhparam_path.display()
                );
                Ok(())
            }
        }
        Err(e) => {
            info!("OpenSSL command failed: {}", e);

            // Provide a basic DH params file as fallback
            info!("Using pre-generated DH parameters as fallback");
            let default_dhparams = include_bytes!("../../assets/dhparams.crt");
            fs::write(&dhparam_path, default_dhparams).await?;
            info!(
                "Fallback DH parameters written to: {}",
                dhparam_path.display()
            );
            Ok(())
        }
    }
}
