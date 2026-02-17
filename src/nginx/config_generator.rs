use anyhow::{Result, anyhow};
use handlebars::Handlebars;
use log::{info, debug};
use serde::Serialize;
use tokio::fs;
use std::path::Path;
use crate::docker::container_info::ContainerInfo;

// Template data structure for Handlebars
#[derive(Serialize)]
struct TemplateData {
    containers: Vec<ContainerInfo>,
}

/// NGINX configuration generator
pub struct ConfigGenerator<'a> {
    containers: &'a [ContainerInfo],
    template_path: String,
}

impl<'a> ConfigGenerator<'a> {
    /// Create a new ConfigGenerator
    pub fn new(containers: &'a [ContainerInfo]) -> Self {
        let template_path = crate::installer::get_config_dir().join("nginx.template.conf");

        Self {
            containers,
            template_path:  String::from(template_path.to_str().unwrap()),
        }
    }

    /// Prepare template data
    fn prepare_template_data(&self) -> TemplateData {
        TemplateData {
            containers: self.containers.to_vec(),
        }
    }

    /// Generate NGINX configuration file
    pub async fn generate_config(&self, output_file: &str) -> Result<()> {
        debug!("Generating NGINX config from template: {}", self.template_path);

        // Check if template file exists
        if !Path::new(&self.template_path).exists() {
            return Err(anyhow!("NGINX template file not found: {}", self.template_path));
        }

        // Read template file
        let template_source = fs::read_to_string(&self.template_path).await?;

        // Setup Handlebars
        let mut handlebars = Handlebars::new();

        // Register template
        handlebars.register_template_string("nginx_template", template_source)?;

        // Prepare data
        let data = self.prepare_template_data();

        // Render template
        let config = handlebars.render("nginx_template", &data)?;

        // Write output file
        fs::write(output_file, config).await?;

        info!("NGINX configuration generated: {}", output_file);
        Ok(())
    }
}

/// Create the default NGINX template if it doesn't exist
pub async fn ensure_nginx_template_exists() -> Result<()> {
    //let template_path = "nginx.template.conf";
    let template_path = crate::installer::get_config_dir().join("nginx.template.conf");


    if template_path.exists() {
        return Ok(());
    }

    info!("Creating default NGINX template: {}", template_path.to_str().unwrap());

    let template_content = r#"# Основные настройки
user nginx;
worker_processes auto;
error_log /var/log/nginx/error.log warn;
pid /var/run/nginx.pid;

events {
    worker_connections 1024;
}

# HTTP настройки для обычного HTTP трафика
http {
    include /etc/nginx/mime.types;
    default_type application/octet-stream;

    log_format main '$remote_addr - $remote_user [$time_local] "$request" '
                    '$status $body_bytes_sent "$http_referer" '
                    '"$http_user_agent" "$http_x_forwarded_for"';

    access_log /var/log/nginx/access.log main;

    sendfile on;
    tcp_nopush on;
    tcp_nodelay on;

    keepalive_timeout 65;
    types_hash_max_size 2048;

    {{#each containers}}
    # Container ID: {{id}}
    {{#each ports}}
    server {
        listen {{external}};
        server_name {{../domain}};

        location / {
            proxy_pass http://{{../name}}:{{internal}};
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
            proxy_set_header X-Forwarded-Proto $scheme;
        }
    }
    {{/each}}
    {{#each ssl_ports}}
    server {
        listen {{external}} ssl;
        server_name {{../domain}};

        ssl_certificate /etc/ssl/certs/{{../domain}}.fullchain.crt;
        ssl_certificate_key /etc/ssl/certs/{{../domain}}.key;

        ssl_session_cache shared:le_nginx_SSL:10m;
        ssl_session_timeout 1440m;
        ssl_session_tickets off;

        ssl_protocols TLSv1.2 TLSv1.3;
        ssl_prefer_server_ciphers off;

        ssl_ciphers "ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-CHACHA20-POLY1305:DHE-RSA-AES128-GCM-SHA256:DHE-RSA-AES256-GCM-SHA384";

        ssl_dhparam /etc/ssl/certs/dhparams.crt;

        location / {
            proxy_pass http://{{../name}}:{{internal}};
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
            proxy_set_header X-Forwarded-Proto $scheme;

            proxy_set_header X-Forwarded-Port {{external}};
            proxy_set_header X-Forwarded-Ssl on;
            proxy_set_header X-Https on;
            proxy_set_header HTTPS "on";
        }
    }
    {{/each}}

    {{/each}}
}
"#;

    fs::write(template_path, template_content).await?;

    Ok(())
}
