// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
#![recursion_limit = "256"]

use std::net::SocketAddr;
use std::{collections::HashMap, time::Duration};

use anyhow::{anyhow, Result};
use axum::{extract::Extension, http::StatusCode, routing::get, Router};
use clap::Parser;
use diesel::pg::PgConnection;
use diesel::r2d2::ConnectionManager;
use jsonrpsee::http_client::{HeaderMap, HeaderValue, HttpClient, HttpClientBuilder};
use metrics::IndexerMetrics;
use prometheus::{Registry, TextEncoder};
use regex::Regex;
use tracing::{info, warn};
use url::Url;

use errors::IndexerError;
use mysten_metrics::RegistryService;
use sui_json_rpc_api::CLIENT_SDK_TYPE_HEADER;

pub mod apis;
pub mod errors;
pub mod framework;
mod handlers;
pub mod indexer_reader;
pub mod indexer_v2;
pub mod metrics;
pub mod models_v2;
pub mod processors_v2;
pub mod schema_v2;
pub mod store;
pub mod test_utils;
pub mod types;
pub mod types_v2;
pub mod utils;

pub type PgConnectionPool = diesel::r2d2::Pool<ConnectionManager<PgConnection>>;
pub type PgPoolConnection = diesel::r2d2::PooledConnection<ConnectionManager<PgConnection>>;

const METRICS_ROUTE: &str = "/metrics";
/// Returns all endpoints for which we have implemented on the indexer,
/// some of them are not validated yet.
/// NOTE: we only use this for integration testing
const IMPLEMENTED_METHODS: [&str; 9] = [
    // read apis
    "get_checkpoint",
    "get_latest_checkpoint_sequence_number",
    "get_object",
    "get_owned_objects",
    "get_total_transaction_blocks",
    "get_transaction_block",
    "multi_get_transaction_blocks",
    // indexer apis
    "query_events",
    "query_transaction_blocks",
];

#[derive(Parser, Clone, Debug)]
#[clap(
    name = "Sui indexer",
    about = "An off-fullnode service serving data from Sui protocol",
    rename_all = "kebab-case"
)]
pub struct IndexerConfig {
    #[clap(long)]
    pub db_url: Option<String>,
    #[clap(long)]
    pub db_user_name: Option<String>,
    #[clap(long)]
    pub db_password: Option<String>,
    #[clap(long)]
    pub db_host: Option<String>,
    #[clap(long)]
    pub db_port: Option<u16>,
    #[clap(long)]
    pub db_name: Option<String>,
    #[clap(long, default_value = "http://0.0.0.0:9000", global = true)]
    pub rpc_client_url: String,
    #[clap(long, default_value = "0.0.0.0", global = true)]
    pub client_metric_host: String,
    #[clap(long, default_value = "9184", global = true)]
    pub client_metric_port: u16,
    #[clap(long, default_value = "0.0.0.0", global = true)]
    pub rpc_server_url: String,
    #[clap(long, default_value = "9000", global = true)]
    pub rpc_server_port: u16,
    #[clap(long, num_args(1..))]
    pub migrated_methods: Vec<String>,
    #[clap(long)]
    pub reset_db: bool,
    #[clap(long)]
    pub fullnode_sync_worker: bool,
    #[clap(long)]
    pub rpc_server_worker: bool,
    #[clap(long)]
    pub analytical_worker: bool,
    // NOTE: experimental only, do not use in production.
    #[clap(long)]
    pub skip_db_commit: bool,
    #[clap(long)]
    pub use_v2: bool,
}

impl IndexerConfig {
    /// returns connection url without the db name
    pub fn base_connection_url(&self) -> Result<String, anyhow::Error> {
        let url_str = self.get_db_url()?;
        let url = Url::parse(&url_str).expect("Failed to parse URL");
        Ok(format!(
            "{}://{}:{}@{}:{}/",
            url.scheme(),
            url.username(),
            url.password().unwrap_or_default(),
            url.host_str().unwrap_or_default(),
            url.port().unwrap_or_default()
        ))
    }

    pub fn all_implemented_methods() -> Vec<String> {
        IMPLEMENTED_METHODS.iter().map(|&s| s.to_string()).collect()
    }

    pub fn get_db_url(&self) -> Result<String, anyhow::Error> {
        match (&self.db_url, &self.db_user_name, &self.db_password, &self.db_host, &self.db_port, &self.db_name) {
            (Some(db_url), _, _, _, _, _) => Ok(db_url.clone()),
            (None, Some(db_user_name), Some(db_password), Some(db_host), Some(db_port), Some(db_name)) => {
                Ok(format!(
                    "postgres://{}:{}@{}:{}/{}",
                    db_user_name, db_password, db_host, db_port, db_name
                ))
            }
            _ => Err(anyhow!("Invalid db connection config, either db_url or (db_user_name, db_password, db_host, db_port, db_name) must be provided")),
        }
    }
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            db_url: Some("postgres://postgres:postgres@localhost:5432/sui_indexer".to_string()),
            db_user_name: None,
            db_password: None,
            db_host: None,
            db_port: None,
            db_name: None,
            rpc_client_url: "http://127.0.0.1:9000".to_string(),
            client_metric_host: "0.0.0.0".to_string(),
            client_metric_port: 9184,
            rpc_server_url: "0.0.0.0".to_string(),
            rpc_server_port: 9000,
            migrated_methods: vec![],
            reset_db: false,
            fullnode_sync_worker: true,
            rpc_server_worker: true,
            analytical_worker: false,
            skip_db_commit: false,
            use_v2: false,
        }
    }
}

fn get_http_client(rpc_client_url: &str) -> Result<HttpClient, IndexerError> {
    let mut headers = HeaderMap::new();
    headers.insert(CLIENT_SDK_TYPE_HEADER, HeaderValue::from_static("indexer"));

    HttpClientBuilder::default()
        .max_request_body_size(2 << 30)
        .max_concurrent_requests(usize::MAX)
        .set_headers(headers.clone())
        .build(rpc_client_url)
        .map_err(|e| {
            warn!("Failed to get new Http client with error: {:?}", e);
            IndexerError::HttpClientInitError(format!(
                "Failed to initialize fullnode RPC client with error: {:?}",
                e
            ))
        })
}

pub fn new_pg_connection_pool(
    db_url: &str,
    pool_size: Option<u32>,
) -> Result<PgConnectionPool, IndexerError> {
    let pool_config = PgConnectionPoolConfig::default();
    let manager = ConnectionManager::<PgConnection>::new(db_url);

    let pool_size = pool_size.unwrap_or(pool_config.pool_size);
    diesel::r2d2::Pool::builder()
        .max_size(pool_size)
        .connection_timeout(pool_config.connection_timeout)
        .connection_customizer(Box::new(pool_config.connection_config()))
        .build(manager)
        .map_err(|e| {
            IndexerError::PgConnectionPoolInitError(format!(
                "Failed to initialize connection pool with error: {:?}",
                e
            ))
        })
}

#[derive(Debug, Clone, Copy)]
pub struct PgConnectionPoolConfig {
    pool_size: u32,
    connection_timeout: Duration,
    statement_timeout: Duration,
}

impl PgConnectionPoolConfig {
    const DEFAULT_POOL_SIZE: u32 = 100;
    const DEFAULT_CONNECTION_TIMEOUT: u64 = 30;
    const DEFAULT_STATEMENT_TIMEOUT: u64 = 30;

    fn connection_config(&self) -> PgConnectionConfig {
        PgConnectionConfig {
            statement_timeout: self.statement_timeout,
            read_only: false,
        }
    }

    pub fn set_pool_size(&mut self, size: u32) {
        self.pool_size = size;
    }

    pub fn set_connection_timeout(&mut self, timeout: Duration) {
        self.connection_timeout = timeout;
    }

    pub fn set_statement_timeout(&mut self, timeout: Duration) {
        self.statement_timeout = timeout;
    }
}

impl Default for PgConnectionPoolConfig {
    fn default() -> Self {
        let db_pool_size = std::env::var("DB_POOL_SIZE")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(Self::DEFAULT_POOL_SIZE);
        let conn_timeout_secs = std::env::var("DB_CONNECTION_TIMEOUT")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(Self::DEFAULT_CONNECTION_TIMEOUT);
        let statement_timeout_secs = std::env::var("DB_STATEMENT_TIMEOUT")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(Self::DEFAULT_STATEMENT_TIMEOUT);

        Self {
            pool_size: db_pool_size,
            connection_timeout: Duration::from_secs(conn_timeout_secs),
            statement_timeout: Duration::from_secs(statement_timeout_secs),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PgConnectionConfig {
    statement_timeout: Duration,
    read_only: bool,
}

impl diesel::r2d2::CustomizeConnection<PgConnection, diesel::r2d2::Error> for PgConnectionConfig {
    fn on_acquire(&self, conn: &mut PgConnection) -> std::result::Result<(), diesel::r2d2::Error> {
        use diesel::{sql_query, RunQueryDsl};

        sql_query(format!(
            "SET statement_timeout = {}",
            self.statement_timeout.as_millis(),
        ))
        .execute(conn)
        .map_err(diesel::r2d2::Error::QueryError)?;

        if self.read_only {
            sql_query("SET default_transaction_read_only = 't'")
                .execute(conn)
                .map_err(diesel::r2d2::Error::QueryError)?;
        }

        Ok(())
    }
}

pub fn get_pg_pool_connection(pool: &PgConnectionPool) -> Result<PgPoolConnection, IndexerError> {
    pool.get().map_err(|e| {
        IndexerError::PgPoolConnectionError(format!(
            "Failed to get connection from PG connection pool with error: {:?}",
            e
        ))
    })
}

fn convert_url(url_str: &str) -> Option<String> {
    // NOTE: unwrap here is safe because the regex is a constant.
    let re = Regex::new(r"https?://([a-z0-9-]+\.[a-z0-9-]+\.[a-z]+)").unwrap();
    let captures = re.captures(url_str)?;

    captures.get(1).map(|m| m.as_str().to_string())
}

pub fn start_prometheus_server(
    addr: SocketAddr,
    fn_url: &str,
) -> Result<(RegistryService, Registry), anyhow::Error> {
    let converted_fn_url = convert_url(fn_url);
    if converted_fn_url.is_none() {
        warn!(
            "Failed to convert full node url {} to a shorter version",
            fn_url
        );
    }
    let fn_url_str = converted_fn_url.unwrap_or_else(|| "unknown_url".to_string());

    let labels = HashMap::from([("indexer_fullnode".to_string(), fn_url_str)]);
    info!("Starting prometheus server with labels: {:?}", labels);
    let registry = Registry::new_custom(Some("indexer".to_string()), Some(labels))?;
    let registry_service = RegistryService::new(registry.clone());

    let app = Router::new()
        .route(METRICS_ROUTE, get(metrics))
        .layer(Extension(registry_service.clone()));

    tokio::spawn(async move {
        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await
            .unwrap();
    });
    Ok((registry_service, registry))
}

async fn metrics(Extension(registry_service): Extension<RegistryService>) -> (StatusCode, String) {
    let metrics_families = registry_service.gather_all();
    match TextEncoder.encode_to_string(&metrics_families) {
        Ok(metrics) => (StatusCode::OK, metrics),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("unable to encode metrics: {error}"),
        ),
    }
}
