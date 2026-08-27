mod deployments;
mod error;
mod events;
mod images;
mod logs;
mod resources;
mod state;
mod watch;
mod ws;

use std::net::SocketAddr;

use axum::routing::{get, post};
use axum::Router;
use clap::Parser;
use kube::Client;
use sqlx::postgres::PgPoolOptions;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use state::AppState;

/// A small dashboard that shows the pods running in a Kubernetes namespace,
/// with their basic resource requests (CPU, memory, accelerators).
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Namespace to watch.
    #[arg(long, env = "NAMESPACE")]
    namespace: String,

    /// Address to bind the HTTP server to.
    #[arg(long, env = "BIND_ADDR", default_value = "0.0.0.0:3000")]
    bind_addr: String,

    /// Directory containing the built frontend (trunk build output) to serve as static files.
    #[arg(long, env = "STATIC_DIR", default_value = "frontend/dist")]
    static_dir: String,

    /// Postgres connection string backing the container image catalog.
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let args = Args::parse();

    // Tries an in-cluster service account first, then falls back to the local kubeconfig.
    let client = Client::try_default().await?;

    let pg = PgPoolOptions::new().max_connections(5).connect(&args.database_url).await?;
    sqlx::migrate!().run(&pg).await?;

    let state = AppState::new(args.namespace.clone(), client.clone(), pg);
    tokio::spawn(watch::run(state.clone(), client));

    let index_html = format!("{}/index.html", args.static_dir);
    let static_service = ServeDir::new(&args.static_dir).fallback(ServeFile::new(&index_html));

    let app = Router::new()
        .route("/api/pods", get(ws::list_pods))
        .route("/api/images", get(images::list_images))
        .route("/api/deployments", post(deployments::create_deployment))
        .route("/api/pods/{name}/logs", get(logs::get_pod_logs))
        .route("/api/pods/{name}/events", get(events::get_pod_events))
        .route("/ws", get(ws::ws_handler))
        .fallback_service(static_service)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = args.bind_addr.parse()?;
    tracing::info!(%addr, namespace = %args.namespace, "starting server");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
