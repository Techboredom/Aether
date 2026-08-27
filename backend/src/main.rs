mod resources;
mod state;
mod watch;
mod ws;

use std::net::SocketAddr;

use axum::routing::get;
use axum::Router;
use clap::Parser;
use kube::Client;
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let args = Args::parse();

    // Tries an in-cluster service account first, then falls back to the local kubeconfig.
    let client = Client::try_default().await?;

    let state = AppState::new(args.namespace.clone());
    tokio::spawn(watch::run(state.clone(), client));

    let index_html = format!("{}/index.html", args.static_dir);
    let static_service = ServeDir::new(&args.static_dir).fallback(ServeFile::new(&index_html));

    let app = Router::new()
        .route("/api/pods", get(ws::list_pods))
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
