use anyhow::{anyhow, Result};
use log::{debug, info};
use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, SanType};
use std::path::PathBuf;
use time::{Duration, OffsetDateTime};
use tokio::fs;

/// Generator for SSL certificates for local domains
pub struct CertificateGenerator {
    domain: String,
    certs_dir: PathBuf,
    ca_dir: PathBuf,
}

impl CertificateGenerator {
    /// Create a new CertificateGenerator for a domain
    pub fn new(domain: &str) -> Self {
        Self {
            domain: domain.to_string(),
            certs_dir: crate::installer::get_certs_dir(),
            ca_dir: crate::installer::get_ca_dir(),
            // certs_dir: PathBuf::from("./certs")
        }
    }

    /// Create a CA certificate
    async fn create_ca_certificate(&self) -> Result<Certificate> {
        info!("Creating CA certificate");

        let mut params = CertificateParams::default();

        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::OrganizationName, "Local Dev Organization");
        distinguished_name.push(DnType::CommonName, "Local Development CA");
        distinguished_name.push(DnType::OrganizationalUnitName, "Development");
        distinguished_name.push(DnType::CountryName, "KZ");

        params.distinguished_name = distinguished_name;
        params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params.key_usages = vec![
            rcgen::KeyUsagePurpose::KeyCertSign,
            rcgen::KeyUsagePurpose::CrlSign,
            rcgen::KeyUsagePurpose::DigitalSignature,
            rcgen::KeyUsagePurpose::KeyEncipherment,
        ];

        // Срок действия: 10 лет
        let now = OffsetDateTime::now_utc();
        params.not_before = now;
        params.not_after = now + Duration::days(3650);

        let cert = Certificate::from_params(params)?;

        Ok(cert)
    }

    /// Create a CA certificate with an existing key
    async fn create_ca_with_key(&self, key_pair: KeyPair) -> Result<Certificate> {
        info!("Creating CA certificate with existing key");

        let mut params = CertificateParams::default();

        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::OrganizationName, "Local Dev Organization");
        distinguished_name.push(DnType::CommonName, "Local Development CA");
        distinguished_name.push(DnType::OrganizationalUnitName, "Development");
        distinguished_name.push(DnType::CountryName, "KZ");

        params.distinguished_name = distinguished_name;
        params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params.key_usages = vec![
            rcgen::KeyUsagePurpose::KeyCertSign,
            rcgen::KeyUsagePurpose::CrlSign,
            rcgen::KeyUsagePurpose::DigitalSignature,
            rcgen::KeyUsagePurpose::KeyEncipherment,
        ];

        // Срок действия: 10 лет
        let now = OffsetDateTime::now_utc();
        params.not_before = now;
        params.not_after = now + Duration::days(3650);

        params.key_pair = Some(key_pair);

        let cert = Certificate::from_params(params)?;

        Ok(cert)
    }

    /// Create a domain certificate
    async fn create_domain_certificate(&self) -> Result<Certificate> {
        info!("Creating domain certificate for {}", self.domain);

        let mut params = CertificateParams::default();

        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::OrganizationName, "Local Dev Organization");
        distinguished_name.push(DnType::CommonName, &self.domain);
        distinguished_name.push(DnType::OrganizationalUnitName, "Development");
        distinguished_name.push(DnType::CountryName, "KZ");

        params.distinguished_name = distinguished_name;

        // Срок действия: 10 лет
        let now = OffsetDateTime::now_utc();
        params.not_before = now;
        params.not_after = now + Duration::days(3650);

        // Добавляем альтернативные имена
        params
            .subject_alt_names
            .push(SanType::DnsName(self.domain.clone()));
        params
            .subject_alt_names
            .push(SanType::DnsName(format!("www.{}", self.domain)));
        params
            .subject_alt_names
            .push(SanType::DnsName("localhost".to_string()));

        // Попробуем добавить IP-адрес 127.0.0.1
        match "127.0.0.1".parse() {
            Ok(ip) => {
                params.subject_alt_names.push(SanType::IpAddress(ip));
            }
            Err(e) => {
                debug!("Failed to parse IP 127.0.0.1: {}", e);
            }
        }

        let cert = Certificate::from_params(params)?;

        Ok(cert)
    }

    /// Check if CA certificate files exist
    async fn has_ca_files(&self) -> bool {
        let ca_cert_path = self.ca_dir.join("localCA.crt");
        let ca_key_path = self.ca_dir.join("localCA.key");

        fs::metadata(&ca_cert_path).await.is_ok() && fs::metadata(&ca_key_path).await.is_ok()
    }

    /// Check if domain certificate files exist
    async fn has_domain_certs(&self) -> bool {
        let domain_cert_path = self.certs_dir.join(format!("{}.crt", self.domain));
        let domain_key_path = self.certs_dir.join(format!("{}.key", self.domain));
        let fullchain_path = self
            .certs_dir
            .join(format!("{}.fullchain.crt", self.domain));

        fs::metadata(&domain_cert_path).await.is_ok()
            && fs::metadata(&domain_key_path).await.is_ok()
            && fs::metadata(&fullchain_path).await.is_ok()
    }

    /// Load CA certificate from files
    async fn load_ca(&self) -> Result<Option<(Certificate, KeyPair)>> {
        let ca_cert_path = self.ca_dir.join("localCA.crt");
        let ca_key_path = self.ca_dir.join("localCA.key");

        if !fs::metadata(&ca_cert_path).await.is_ok() || !fs::metadata(&ca_key_path).await.is_ok() {
            return Ok(None);
        }

        info!("Loading CA certificate from files");

        // Загружаем CA ключ
        let ca_key_pem = fs::read_to_string(&ca_key_path)
            .await
            .map_err(|e| anyhow!("Failed to read CA key file: {}", e))?;

        // Создаем KeyPair из PEM-файла
        let ca_key_pair = KeyPair::from_pem(&ca_key_pem)
            .map_err(|e| anyhow!("Failed to parse CA key PEM: {}", e))?;

        // Создаем новый CA сертификат с тем же ключом
        let ca_cert = self.create_ca_with_key(ca_key_pair).await?;

        // Создаем новый KeyPair из того же PEM (так как KeyPair не реализует Clone)
        let ca_key_pair_new = KeyPair::from_pem(&ca_key_pem)
            .map_err(|e| anyhow!("Failed to parse CA key PEM again: {}", e))?;

        Ok(Some((ca_cert, ca_key_pair_new)))
    }

    /// Generate certificates for a domain if they don't exist
    pub async fn generate_certificates(&self) -> Result<()> {
        // Create certs directory if it doesn't exist
        fs::create_dir_all(&self.certs_dir).await?;
        fs::create_dir_all(&self.ca_dir).await?;

        // Check if domain certificates already exist
        if self.has_domain_certs().await {
            debug!("Domain certificates for {} already exist", self.domain);
            return Ok(());
        }

        info!("Generating certificates for {}", self.domain);

        // Get or create CA certificate
        let (ca_cert, _ca_key) = if self.has_ca_files().await {
            match self.load_ca().await? {
                Some(ca) => ca,
                None => {
                    let ca_cert = self.create_ca_certificate().await?;

                    // Сохраняем CA сертификат
                    let ca_cert_pem = ca_cert.serialize_pem()?;
                    let ca_key_pem = ca_cert.serialize_private_key_pem();

                    fs::write(self.ca_dir.join("localCA.crt"), &ca_cert_pem).await?;
                    fs::write(self.ca_dir.join("localCA.key"), &ca_key_pem).await?;

                    // Получаем KeyPair из CA сертификата для подписи
                    let ca_key_pair = KeyPair::from_pem(&ca_key_pem)
                        .map_err(|e| anyhow!("Failed to parse generated CA key PEM: {}", e))?;

                    (ca_cert, ca_key_pair)
                }
            }
        } else {
            // Создаем CA сертификат
            let ca_cert = self.create_ca_certificate().await?;

            // Сохраняем CA сертификат
            let ca_cert_pem = ca_cert.serialize_pem()?;
            let ca_key_pem = ca_cert.serialize_private_key_pem();

            fs::write(self.ca_dir.join("localCA.crt"), &ca_cert_pem).await?;
            fs::write(self.ca_dir.join("localCA.key"), &ca_key_pem).await?;

            // Получаем KeyPair из CA сертификата для подписи
            let ca_key_pair = KeyPair::from_pem(&ca_key_pem)
                .map_err(|e| anyhow!("Failed to parse generated CA key PEM: {}", e))?;

            (ca_cert, ca_key_pair)
        };

        // Создаем сертификат домена
        let domain_cert = self.create_domain_certificate().await?;

        // Подписываем сертификат домена с помощью CA
        let cert_pem = domain_cert
            .serialize_pem_with_signer(&ca_cert)
            .map_err(|e| anyhow!("Failed to sign domain certificate: {}", e))?;
        let key_pem = domain_cert.serialize_private_key_pem();

        // Создаем цепочку сертификатов
        let ca_cert_pem = ca_cert.serialize_pem()?;
        let chain_pem = format!("{}\n{}", cert_pem, ca_cert_pem);

        // Сохраняем файлы сертификатов
        fs::write(
            self.certs_dir.join(format!("{}.crt", self.domain)),
            &cert_pem,
        )
        .await?;
        fs::write(
            self.certs_dir.join(format!("{}.key", self.domain)),
            &key_pem,
        )
        .await?;
        fs::write(
            self.certs_dir
                .join(format!("{}.fullchain.crt", self.domain)),
            &chain_pem,
        )
        .await?;

        info!("Successfully generated certificates for {}", self.domain);
        Ok(())
    }
}
