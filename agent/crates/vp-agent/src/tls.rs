//! Local trust: a per-installation, name-constrained root authority (only valid for `.local`,
//! `localhost`, private IP ranges and Tailscale addresses) issues a short-lived TLS 1.3 server
//! certificate. The iPad trusts the authority once (configuration profile); nothing is exposed to
//! the Internet.

use crate::paths;
use crate::platform::dpapi;
use crate::util::{b64u, from_b64u, now_ms};
use rcgen::{
    BasicConstraints, CertificateParams, CidrSubnet, DnType, ExtendedKeyUsagePurpose,
    GeneralSubtree, IsCa, Issuer, KeyPair, KeyUsagePurpose, NameConstraints,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::sync::Arc;
use time::{Duration, OffsetDateTime};
use zeroize::Zeroizing;

const PURPOSE: &str = "local-authority";
/// Version 2 also permits Tailscale addresses (100.64.0.0/10). Older authorities are replaced,
/// so iPads trust the new one once.
const AUTHORITY_VERSION: u32 = 2;

#[derive(Serialize, Deserialize)]
struct StoredAuthority {
    version: u32,
    key: String,
    cert: String,
    created_at: u64,
}

pub struct LocalAuthority {
    key: KeyPair,
    pub cert_der: Vec<u8>,
    pub created_at: u64,
    version: u32,
}

impl LocalAuthority {
    pub fn load_or_create(computer_name: &str) -> anyhow::Result<LocalAuthority> {
        let path = paths::ensure_data_dir()?.join("authority.bin");
        if let Ok(wrapped) = std::fs::read(&path) {
            match Self::decode(&wrapped) {
                Ok(authority)
                    if !authority.expires_soon() && authority.version >= AUTHORITY_VERSION =>
                {
                    return Ok(authority)
                }
                Ok(_) => tracing::info!("local authority outdated or close to expiry; renewing"),
                Err(error) => {
                    tracing::error!(%error, "local authority unreadable; creating a new one")
                }
            }
        }
        let authority = Self::generate(computer_name)?;
        let stored = StoredAuthority {
            version: AUTHORITY_VERSION,
            key: b64u(&authority.key.serialize_der()),
            cert: b64u(&authority.cert_der),
            created_at: authority.created_at,
        };
        let plain = Zeroizing::new(serde_json::to_vec(&stored)?);
        paths::write_atomic(&path, &dpapi::protect(&plain, PURPOSE)?)?;
        Ok(authority)
    }

    fn decode(wrapped: &[u8]) -> anyhow::Result<LocalAuthority> {
        let plain = Zeroizing::new(dpapi::unprotect(wrapped, PURPOSE)?);
        let stored: StoredAuthority = serde_json::from_slice(&plain)?;
        let key_der =
            Zeroizing::new(from_b64u(&stored.key, None).ok_or_else(|| anyhow::anyhow!("bad key"))?);
        let key = KeyPair::try_from(key_der.as_slice())?;
        let cert_der = from_b64u(&stored.cert, None).ok_or_else(|| anyhow::anyhow!("bad cert"))?;
        Ok(LocalAuthority {
            key,
            cert_der,
            created_at: stored.created_at,
            version: stored.version,
        })
    }

    fn expires_soon(&self) -> bool {
        // Authorities are valid 5 years; renew during the last 90 days.
        now_ms().saturating_sub(self.created_at) > (5 * 365 - 90) * 86_400_000
    }

    fn generate(computer_name: &str) -> anyhow::Result<LocalAuthority> {
        let key = KeyPair::generate()?;
        let mut params = CertificateParams::new(Vec::<String>::new())?;
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.distinguished_name.push(
            DnType::CommonName,
            format!("Vision-Pro Local Authority · {computer_name}"),
        );
        params
            .distinguished_name
            .push(DnType::OrganizationName, "Vision-Pro");
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        let now = OffsetDateTime::now_utc();
        params.not_before = now - Duration::days(1);
        params.not_after = now + Duration::days(5 * 365);
        params.name_constraints = Some(NameConstraints {
            permitted_subtrees: vec![
                GeneralSubtree::DnsName("local".into()),
                GeneralSubtree::DnsName("localhost".into()),
                GeneralSubtree::IpAddress(CidrSubnet::V4([10, 0, 0, 0], [255, 0, 0, 0])),
                GeneralSubtree::IpAddress(CidrSubnet::V4([172, 16, 0, 0], [255, 240, 0, 0])),
                GeneralSubtree::IpAddress(CidrSubnet::V4([192, 168, 0, 0], [255, 255, 0, 0])),
                GeneralSubtree::IpAddress(CidrSubnet::V4([169, 254, 0, 0], [255, 255, 0, 0])),
                GeneralSubtree::IpAddress(CidrSubnet::V4([127, 0, 0, 0], [255, 0, 0, 0])),
                GeneralSubtree::IpAddress(CidrSubnet::V4([100, 64, 0, 0], [255, 192, 0, 0])),
            ],
            excluded_subtrees: vec![],
        });
        let cert = params.self_signed(&key)?;
        Ok(LocalAuthority {
            key,
            cert_der: cert.der().to_vec(),
            created_at: now_ms(),
            version: AUTHORITY_VERSION,
        })
    }

    pub fn fingerprint_sha256(&self) -> String {
        crate::crypto::sha256(&[&self.cert_der])
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(":")
    }

    /// Issues a 30-day server certificate for the given hostnames and LAN/Tailscale addresses.
    pub fn issue_server_certificate(
        &self,
        hostnames: &[String],
        ips: &[Ipv4Addr],
    ) -> anyhow::Result<ServerCertificate> {
        let mut names: Vec<String> = hostnames.to_vec();
        names.push("localhost".into());
        names.push("127.0.0.1".into());
        names.extend(ips.iter().map(|ip| ip.to_string()));
        names.dedup();
        let mut params = CertificateParams::new(names)?;
        params.distinguished_name.push(
            DnType::CommonName,
            hostnames
                .first()
                .cloned()
                .unwrap_or_else(|| "localhost".into()),
        );
        params
            .distinguished_name
            .push(DnType::OrganizationName, "Vision-Pro");
        let now = OffsetDateTime::now_utc();
        params.not_before = now - Duration::hours(1);
        params.not_after = now + Duration::days(30);
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.use_authority_key_identifier_extension = true;
        let leaf_key = KeyPair::generate()?;
        let issuer =
            Issuer::from_ca_cert_der(&CertificateDer::from(self.cert_der.as_slice()), &self.key)?;
        let cert = params.signed_by(&leaf_key, &issuer)?;
        Ok(ServerCertificate {
            cert_der: cert.der().to_vec(),
            key_der: Zeroizing::new(leaf_key.serialize_der()),
            issued_at: now_ms(),
        })
    }

    /// iOS/iPadOS configuration profile that installs the authority.
    pub fn mobileconfig(&self, computer_name: &str) -> String {
        let payload_uuid = uuid::Uuid::new_v4().to_string().to_uppercase();
        let profile_uuid = uuid::Uuid::new_v4().to_string().to_uppercase();
        let data =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &self.cert_der);
        let name = xml_escape(computer_name);
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>PayloadContent</key>
  <array>
    <dict>
      <key>PayloadCertificateFileName</key><string>vision-pro-local-authority.cer</string>
      <key>PayloadContent</key><data>{data}</data>
      <key>PayloadDescription</key><string>Lets this iPad verify the secure connection to {name}. Valid only for local network and Tailscale addresses.</string>
      <key>PayloadDisplayName</key><string>Vision-Pro Local Authority</string>
      <key>PayloadIdentifier</key><string>app.vision-pro.local-authority.{payload_uuid}</string>
      <key>PayloadType</key><string>com.apple.security.root</string>
      <key>PayloadUUID</key><string>{payload_uuid}</string>
      <key>PayloadVersion</key><integer>1</integer>
    </dict>
  </array>
  <key>PayloadDescription</key><string>Secure local connection to {name} with Vision-Pro.</string>
  <key>PayloadDisplayName</key><string>Vision-Pro · {name}</string>
  <key>PayloadIdentifier</key><string>app.vision-pro.profile.{profile_uuid}</string>
  <key>PayloadRemovalDisallowed</key><false/>
  <key>PayloadType</key><string>Configuration</string>
  <key>PayloadUUID</key><string>{profile_uuid}</string>
  <key>PayloadVersion</key><integer>1</integer>
</dict>
</plist>
"#
        )
    }
}

pub struct ServerCertificate {
    pub cert_der: Vec<u8>,
    pub key_der: Zeroizing<Vec<u8>>,
    pub issued_at: u64,
}

impl ServerCertificate {
    pub fn rustls_config(
        &self,
        authority: &LocalAuthority,
    ) -> anyhow::Result<Arc<rustls::ServerConfig>> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let chain = vec![
            CertificateDer::from(self.cert_der.clone()),
            CertificateDer::from(authority.cert_der.clone()),
        ];
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.to_vec()));
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_no_client_auth()
            .with_single_cert(chain, key)?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
