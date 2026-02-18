mod auth;
mod comments;
mod db;
mod forum;
mod github_stats;
mod votes;

use axum::{
    routing::{delete, get, post},
    Router,
};
use tower_http::cors::{AllowHeaders, AllowMethods, CorsLayer};

pub type DbPool = r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>;

#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    pub jwt_secret: String,
    pub github_client_id: String,
    pub github_client_secret: String,
    pub api_url: String,
    pub cors_origin: String,
}

#[tokio::main]
async fn main() {
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "mikaana.db".to_string());
    let manager = r2d2_sqlite::SqliteConnectionManager::file(&database_url);
    let pool = r2d2::Pool::new(manager).expect("Failed to create DB pool");

    db::run_migrations(&pool).expect("Failed to run migrations");

    let cors_origin =
        std::env::var("CORS_ORIGIN").unwrap_or_else(|_| "http://localhost:1313".to_string());
    let api_url =
        std::env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());

    let state = AppState {
        db: pool,
        jwt_secret: std::env::var("JWT_SECRET")
            .unwrap_or_else(|_| "dev-secret-change-me".to_string()),
        github_client_id: std::env::var("GITHUB_CLIENT_ID").unwrap_or_default(),
        github_client_secret: std::env::var("GITHUB_CLIENT_SECRET").unwrap_or_default(),
        api_url,
        cors_origin: cors_origin.clone(),
    };

    let cors = CorsLayer::new()
        .allow_origin(
            cors_origin
                .parse::<axum::http::HeaderValue>()
                .expect("Invalid CORS_ORIGIN"),
        )
        .allow_methods(AllowMethods::any())
        .allow_headers(AllowHeaders::any());

    let app = Router::new()
        .route("/api/health", get(|| async { "ok" }))
        // Auth
        .route("/api/auth/github", get(auth::github_login))
        .route("/api/auth/callback", get(auth::github_callback))
        .route("/api/auth/me", get(auth::me))
        // Comments
        .route(
            "/api/comments",
            get(comments::list_comments).post(comments::create_comment),
        )
        .route("/api/comments/{id}", delete(comments::delete_comment))
        // Votes
        .route(
            "/api/votes",
            get(votes::get_votes).post(votes::cast_vote),
        )
        // GitHub Stats
        .route("/api/github-stats", get(github_stats::get_github_stats))
        // Forum
        .route("/api/forum/categories", get(forum::list_categories))
        .route(
            "/api/forum/threads",
            get(forum::list_threads).post(forum::create_thread),
        )
        .route("/api/forum/threads/{id}", get(forum::get_thread))
        .route(
            "/api/forum/threads/{id}/replies",
            post(forum::create_reply),
        )
        .layer(cors)
        .with_state(state);

    let addr = "0.0.0.0:8080";
    println!("API server listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
