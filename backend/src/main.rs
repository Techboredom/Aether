mod auth;
mod deployments;
mod error;
mod events;
mod images;
mod logs;
mod proxy;
mod quota;
mod resources;
mod state;
mod templates;
mod users;
mod validate;
mod visibility;
mod watch;
mod ws;

use std::net::SocketAddr;

use axum::routing::{any, get, post, put};
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

    /// Password for a one-time bootstrap "admin" account, created only if the
    /// `users` table is empty. Ignored once any user exists.
    #[arg(long, env = "ADMIN_BOOTSTRAP_PASSWORD")]
    admin_bootstrap_password: Option<String>,

    /// Public origin this app is served from, e.g.
    /// "https://aether.example.com". Must be set together with
    /// `--proxy-base-domain`.
    #[arg(long, env = "APP_ORIGIN", requires = "proxy_base_domain")]
    app_origin: Option<String>,

    /// Base domain for per-deployment proxy origins, e.g.
    /// "proxy.aether.example.com" — a deployment named `foo` is then
    /// served at `foo.proxy.aether.example.com`, on its own origin
    /// rather than on a path under this app's. Needs a wildcard DNS record
    /// (and TLS cert) covering `*.<this domain>`.
    ///
    /// Leaving this unset keeps the legacy same-origin `/proxy/<name>/`
    /// behavior, which lets a proxied app's JavaScript call this app's own
    /// API as whoever is browsing it — acceptable for local development,
    /// not for a shared deployment.
    #[arg(long, env = "PROXY_BASE_DOMAIN", requires = "app_origin")]
    proxy_base_domain: Option<String>,
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
    bootstrap_admin(&pg, args.admin_bootstrap_password.as_deref()).await?;

    // `requires` on both flags means clap rejects setting one without the
    // other, so this zip either yields both or neither.
    let proxy_origin = args.app_origin.as_ref().zip(args.proxy_base_domain.as_ref()).map(|(app_origin, base_domain)| {
        state::ProxyOrigin {
            app_origin: app_origin.trim_end_matches('/').to_string(),
            base_domain: base_domain.trim_start_matches('.').to_string(),
        }
    });
    match &proxy_origin {
        Some(origin) => tracing::info!(
            base_domain = %origin.base_domain,
            app_origin = %origin.app_origin,
            "serving proxied deployments on per-deployment origins"
        ),
        None => tracing::warn!(
            "PROXY_BASE_DOMAIN is not set — proxied apps are served same-origin at /proxy/<name>/, \
             where their own JavaScript can call this app's API as whoever is browsing them; \
             intended for local development only"
        ),
    }

    let state = AppState::new(args.namespace.clone(), client.clone(), pg, proxy_origin);
    tokio::spawn(watch::run(state.clone(), client));

    let index_html = format!("{}/index.html", args.static_dir);
    let static_service = ServeDir::new(&args.static_dir).fallback(ServeFile::new(&index_html));

    let app = Router::new()
        .route("/api/login", post(auth::login))
        .route("/api/logout", post(auth::logout))
        .route("/api/me", get(auth::me))
        .route("/api/me/password", put(auth::change_password))
        .route("/api/sessions", get(auth::list_sessions))
        .route("/api/users", get(users::list_users).post(users::create_user))
        .route("/api/users/{id}", axum::routing::delete(users::delete_user))
        .route("/api/users/{id}/password", put(users::reset_password))
        .route("/api/users/{id}/node-label", put(users::set_node_label))
        .route("/api/pods", get(ws::list_pods))
        .route("/api/images", get(images::list_images).post(images::create_image))
        .route("/api/images/{id}", put(images::update_image).delete(images::delete_image))
        .route("/api/templates", get(templates::list_templates).post(templates::create_template))
        .route("/api/templates/{id}", put(templates::update_template).delete(templates::delete_template))
        .route("/api/deployments", post(deployments::create_deployment))
        .route(
            "/api/deployments/{name}",
            get(deployments::get_deployment).put(deployments::update_deployment).delete(deployments::delete_deployment),
        )
        .route("/api/launches", get(deployments::list_launches))
        .route("/api/quota/me", get(quota::my_quota))
        .route("/api/quota/settings", get(quota::get_settings).put(quota::update_settings))
        .route("/api/quota/users", get(quota::list_user_quotas))
        .route("/api/quota/users/{id}", put(quota::set_user_quota).delete(quota::clear_user_quota))
        .route("/api/pods/{name}/logs", get(logs::get_pod_logs))
        .route("/api/pods/{name}/events", get(events::get_pod_events))
        .route("/ws", get(ws::ws_handler))
        // The bare/trailing-slash routes exist because matchit's `{*rest}`
        // wildcard requires at least one character after the slash — without
        // them, every "Open" link (which points at the bare
        // "/proxy/<name>/") would silently miss this route entirely and hit
        // the SPA fallback below instead. See proxy::handler_root.
        .route("/proxy/{deployment_name}", any(proxy::handler_root))
        .route("/proxy/{deployment_name}/", any(proxy::handler_root))
        .route("/proxy/{deployment_name}/{*rest}", any(proxy::handler))
        // Entry point for a proxy origin that has no session of its own yet:
        // it lands here, on the app origin, where the caller's session cookie
        // actually exists. See proxy::start_proxy_auth.
        .route("/proxy-auth", get(proxy::start_proxy_auth))
        .fallback_service(static_service)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone())
        // Outermost, so a request to a deployment's own origin is served as
        // that deployment and never reaches the routes above at all.
        .layer(axum::middleware::from_fn_with_state(state, proxy::dispatch_by_host));

    let addr: SocketAddr = args.bind_addr.parse()?;
    tracing::info!(%addr, namespace = %args.namespace, "starting server");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    // `ConnectInfo` (used by auth::login to record a login's source IP in
    // session_log) requires the connect-info-aware make-service.
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;

    Ok(())
}

/// Creates the initial "admin" account if (and only if) no users exist yet.
/// Without this there'd be no way to log in at all on a fresh database.
async fn bootstrap_admin(pg: &sqlx::PgPool, bootstrap_password: Option<&str>) -> anyhow::Result<()> {
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(pg).await?;
    if user_count > 0 {
        return Ok(());
    }
    let Some(password) = bootstrap_password else {
        tracing::warn!(
            "no users exist yet and ADMIN_BOOTSTRAP_PASSWORD is not set — the app has no way to log in until a user is created"
        );
        return Ok(());
    };
    let password_hash = auth::hash_password(password).map_err(|_| anyhow::anyhow!("failed to hash bootstrap password"))?;
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES ('admin', $1, 'admin')")
        .bind(&password_hash)
        .execute(pg)
        .await?;
    tracing::info!("created initial admin account (username: admin)");
    Ok(())
}
