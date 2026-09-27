//! vp-relay — Vision-Pro Internet relay.
//!
//!   vp-relay --host relay.example.com --cert fullchain.pem --key privkey.pem [--token SECRET]
//!
//! Options (each also readable from the environment variable in brackets):
//!   --host <name>        public hostname or IP                 [VP_RELAY_HOST]      required
//!   --http <addr>        HTTP(S) listener                      [VP_RELAY_HTTP]      0.0.0.0:443 with a certificate, else 0.0.0.0:8080
//!   --turn <addr>        TURN UDP+TCP listener                 [VP_RELAY_TURN]      0.0.0.0:3478
//!   --turn-tls <addr>    TURN over TLS listener                [VP_RELAY_TURN_TLS]  0.0.0.0:5349 with a certificate
//!   --cert <file>        PEM certificate chain                 [VP_RELAY_CERT]
//!   --key <file>         PEM private key                       [VP_RELAY_KEY]
//!   --token <secret>     token agents must present             [VP_RELAY_TOKEN]
//!   --trust-proxy        trust X-Forwarded-For                 [VP_RELAY_TRUST_PROXY=1]

use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::net::SocketAddr;
use std::sync::Arc;
use vp_relay::RelayConfig;

fn option(args: &[String], flag: &str, env: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
        .or_else(|| std::env::var(env).ok())
        .filter(|v| !v.trim().is_empty())
}

fn address(value: Option<String>, default: &str, name: &str) -> anyhow::Result<SocketAddr> {
    value
        .as_deref()
        .unwrap_or(default)
        .parse()
        .map_err(|_| anyhow::anyhow!("{name} must be an address such as {default}"))
}

fn load_tls(cert: &str, key: &str) -> anyhow::Result<Arc<rustls::ServerConfig>> {
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(cert)
        .map_err(|e| anyhow::anyhow!("cannot read certificate {cert}: {e}"))?
        .collect::<Result<_, _>>()
        .map_err(|e| anyhow::anyhow!("invalid certificate {cert}: {e}"))?;
    anyhow::ensure!(!chain.is_empty(), "certificate file {cert} is empty");
    let private_key = PrivateKeyDer::from_pem_file(key)
        .map_err(|e| anyhow::anyhow!("cannot read private key {key}: {e}"))?;
    let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(chain, private_key)?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "{}",
            include_str!("main.rs")
                .lines()
                .take_while(|l| l.starts_with("//!"))
                .map(|l| l.trim_start_matches("//!").trim_start_matches(' '))
                .collect::<Vec<_>>()
                .join("\n")
        );
        return Ok(());
    }

    let public_host = option(&args, "--host", "VP_RELAY_HOST").ok_or_else(|| {
        anyhow::anyhow!("--host (or VP_RELAY_HOST) is required: the public name of this relay")
    })?;
    let tls = match (
        option(&args, "--cert", "VP_RELAY_CERT"),
        option(&args, "--key", "VP_RELAY_KEY"),
    ) {
        (Some(cert), Some(key)) => Some(load_tls(&cert, &key)?),
        (None, None) => None,
        _ => anyhow::bail!("--cert and --key must be provided together"),
    };
    let http_default = if tls.is_some() {
        "0.0.0.0:443"
    } else {
        "0.0.0.0:8080"
    };
    let config = RelayConfig {
        http_bind: address(
            option(&args, "--http", "VP_RELAY_HTTP"),
            http_default,
            "--http",
        )?,
        turn_bind: address(
            option(&args, "--turn", "VP_RELAY_TURN"),
            "0.0.0.0:3478",
            "--turn",
        )?,
        turn_tls_bind: match &tls {
            Some(_) => Some(address(
                option(&args, "--turn-tls", "VP_RELAY_TURN_TLS"),
                "0.0.0.0:5349",
                "--turn-tls",
            )?),
            None => None,
        },
        tls,
        public_host,
        access_token: option(&args, "--token", "VP_RELAY_TOKEN"),
        trust_proxy: args.iter().any(|a| a == "--trust-proxy")
            || std::env::var("VP_RELAY_TRUST_PROXY").is_ok_and(|v| v == "1"),
    };
    if config.access_token.as_ref().is_some_and(|t| t.len() < 16) {
        anyhow::bail!("--token must be at least 16 characters");
    }

    let running = vp_relay::start(config.clone()).await?;
    let scheme = if config.tls.is_some() { "wss" } else { "ws" };
    println!("Vision-Pro relay {}", env!("CARGO_PKG_VERSION"));
    println!(
        "  Signaling  {scheme}://{}:{}  (listening on {})",
        config.public_host,
        running.http_addr.port(),
        running.http_addr
    );
    for url in &running.relay.turn_urls {
        println!("  TURN       {url}");
    }
    println!(
        "  Agents     {}",
        if config.access_token.is_some() {
            "token required"
        } else {
            "any Vision-Pro Mac (set --token to restrict)"
        }
    );
    if config.tls.is_none() {
        println!(
            "  !! No certificate: serve this behind a TLS reverse proxy. iPads require wss://."
        );
    }

    tokio::signal::ctrl_c().await?;
    running.stop();
    Ok(())
}
