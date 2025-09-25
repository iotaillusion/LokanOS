use std::net::SocketAddr;
use std::sync::Arc;

use api_gateway::audit::AuditClient;
use api_gateway::config::{ApiGatewayConfig, TlsConfig};
use api_gateway::device_registry::DeviceRegistryClient;
use api_gateway::rate_limit::RateLimiter;
use api_gateway::{build_router, load_policy, AppState, SERVICE_NAME};
use axum_server::tls_rustls::RustlsConfig;
use common_config::load;
use common_mdns::announce;
use common_msgbus::{MessageBus, NatsBus, NatsConfig};
use common_obs::ObsInit;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig as RustlsServerConfig};
use tokio::fs;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn build_sha() -> &'static str {
    option_env!("BUILD_SHA").unwrap_or("unknown")
}

fn build_time() -> &'static str {
    option_env!("BUILD_TIME").unwrap_or("unknown")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ObsInit::init(SERVICE_NAME).map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;

    let config = load::<ApiGatewayConfig>()?;
    let addr = config.socket_addr()?;
    tracing::info!(
        event = "service_start",
        service = SERVICE_NAME,
        version = VERSION,
        build_sha = build_sha(),
        build_time = build_time(),
        listen_addr = %addr,
        "starting service"
    );

    let bus_config = NatsConfig {
        url: config.bus.url.clone(),
        request_timeout: config.bus.request_timeout(),
    };
    let bus: Arc<dyn MessageBus> = Arc::new(NatsBus::connect(bus_config).await?);

    let _mdns = if config.announce_mdns {
        Some(announce(&config.mdns_service, config.port).await?)
    } else {
        tracing::info!(service = SERVICE_NAME, "mDNS announcement disabled");
        None
    };

    let policy = Arc::new(load_policy(&config)?);
    let audit = AuditClient::new(config.audit.endpoint.clone(), config.audit.enabled);
    let rate_limiter = RateLimiter::new(&config.rate_limit);
    let device_client = DeviceRegistryClient::new(config.device_registry_url.clone())
        .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;

    let state = Arc::new(AppState {
        policy,
        audit,
        rate_limiter,
        device_client,
        bus,
    });

    let router = build_router(state.clone());
    let rustls_config = build_rustls_config(&config.tls).await?;

    axum_server::bind_rustls(addr, rustls_config)
        .serve(router.into_make_service_with_connect_info::<SocketAddr>())
        .await?;

    tracing::info!(event = "service_stop", service = SERVICE_NAME);

    Ok(())
}

async fn build_rustls_config(
    config: &TlsConfig,
) -> Result<RustlsConfig, Box<dyn std::error::Error>> {
    let certs = load_certs(&config.cert_path).await?;
    let key = load_private_key(&config.key_path).await?;
    let client_store = load_client_ca(&config.client_ca_path).await?;

    let client_verifier = WebPkiClientVerifier::builder(Arc::new(client_store))
        .build()
        .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;
    let server_config = RustlsServerConfig::builder()
        .with_client_cert_verifier(client_verifier)
        .with_single_cert(certs, key)?;

    Ok(RustlsConfig::from_config(Arc::new(server_config)))
}

async fn load_certs(
    path: &std::path::Path,
) -> Result<Vec<CertificateDer<'static>>, Box<dyn std::error::Error>> {
    let bytes = fs::read(path).await?;
    let mut reader = std::io::Cursor::new(bytes);
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    Ok(certs)
}

async fn load_private_key(
    path: &std::path::Path,
) -> Result<PrivateKeyDer<'static>, Box<dyn std::error::Error>> {
    let bytes = fs::read(path).await?;
    let mut reader = std::io::Cursor::new(bytes);
    let keys = rustls_pemfile::pkcs8_private_keys(&mut reader)
        .collect::<Result<Vec<PrivatePkcs8KeyDer<'static>>, _>>()?;
    if let Some(key) = keys.into_iter().next() {
        Ok(PrivateKeyDer::from(key))
    } else {
        Err("no private key found".into())
    }
}

async fn load_client_ca(
    path: &std::path::Path,
) -> Result<RootCertStore, Box<dyn std::error::Error>> {
    let bytes = fs::read(path).await?;
    let mut reader = std::io::Cursor::new(bytes);
    let mut store = RootCertStore::empty();
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    for cert in certs {
        store.add(cert)?;
    }
    Ok(store)
}
