use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
    Json,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use mikaana_shared::User;
use serde::{Deserialize, Serialize};

use crate::AppState;

// ── JWT Claims ──

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,    // user id
    pub exp: usize,  // expiry (unix timestamp)
}

impl Claims {
    pub fn new(user_id: i64) -> Self {
        let exp = chrono_like_exp(); // 30 days from now
        Self { sub: user_id, exp }
    }
}

fn chrono_like_exp() -> usize {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize;
    now + 30 * 24 * 60 * 60 // 30 days
}

// ── Extract authenticated user from Authorization header ──

pub fn extract_user_id(headers: &HeaderMap, jwt_secret: &str) -> Result<i64, StatusCode> {
    let token = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| StatusCode::UNAUTHORIZED)?;

    Ok(data.claims.sub)
}

// ── GitHub OAuth types ──

#[derive(Deserialize)]
pub struct LoginParams {
    redirect: Option<String>,
}

#[derive(Deserialize)]
pub struct CallbackParams {
    code: String,
    state: Option<String>,
}

#[derive(Deserialize)]
struct GitHubTokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct GitHubUser {
    id: i64,
    login: String,
    avatar_url: String,
}

// ── Handlers ──

/// GET /api/auth/github — redirect to GitHub OAuth
pub async fn github_login(
    State(state): State<AppState>,
    Query(params): Query<LoginParams>,
) -> impl IntoResponse {
    let redirect_after = params
        .redirect
        .unwrap_or_else(|| state.cors_origin.clone());

    let url = format!(
        "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}/api/auth/callback&state={}",
        state.github_client_id,
        state.api_url,
        urlencoding::encode(&redirect_after),
    );

    Redirect::temporary(&url)
}

/// GET /api/auth/callback — exchange code, upsert user, redirect with JWT
pub async fn github_callback(
    State(state): State<AppState>,
    Query(params): Query<CallbackParams>,
) -> Result<impl IntoResponse, StatusCode> {
    // Exchange code for access token
    let client = reqwest::Client::new();
    let token_resp = client
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "client_id": state.github_client_id,
            "client_secret": state.github_client_secret,
            "code": params.code,
        }))
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?
        .json::<GitHubTokenResponse>()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    // Fetch GitHub user profile
    let gh_user = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {}", token_resp.access_token))
        .header("User-Agent", "mikaana-api")
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?
        .json::<GitHubUser>()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    // Upsert user in DB
    let pool = state.db.clone();
    let gh_id = gh_user.id;
    let username = gh_user.login.clone();
    let avatar = gh_user.avatar_url.clone();

    let user_id = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        conn.execute(
            "INSERT INTO users (github_id, username, avatar_url)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(github_id) DO UPDATE SET username = ?2, avatar_url = ?3",
            rusqlite::params![gh_id, username, avatar],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let id: i64 = conn
            .query_row(
                "SELECT id FROM users WHERE github_id = ?1",
                [gh_id],
                |row| row.get(0),
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        Ok::<_, StatusCode>(id)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;

    // Create JWT
    let claims = Claims::new(user_id);
    let jwt = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Redirect back to the frontend with the token
    let redirect_to = params
        .state
        .unwrap_or_else(|| state.cors_origin.clone());

    let separator = if redirect_to.contains('?') { "&" } else { "?" };
    let url = format!("{}{separator}token={jwt}", redirect_to);

    Ok(Redirect::temporary(&url))
}

/// GET /api/auth/me — return current user
pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<User>, StatusCode> {
    let user_id = extract_user_id(&headers, &state.jwt_secret)?;

    let pool = state.db.clone();
    let user = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        conn.query_row(
            "SELECT id, username, avatar_url FROM users WHERE id = ?1",
            [user_id],
            |row| {
                Ok(User {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    avatar_url: row.get(2)?,
                })
            },
        )
        .map_err(|_| StatusCode::NOT_FOUND)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;

    Ok(Json(user))
}
