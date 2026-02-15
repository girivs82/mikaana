use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use mikaana_shared::{CreateVote, VoteResponse};
use serde::Deserialize;

use crate::{auth, AppState};

#[derive(Deserialize)]
pub struct VoteQuery {
    r#type: String,
    id: i64,
}

/// GET /api/votes?type=comment&id=123
pub async fn get_votes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<VoteQuery>,
) -> Result<Json<VoteResponse>, StatusCode> {
    let user_id = auth::extract_user_id(&headers, &state.jwt_secret).ok();

    let pool = state.db.clone();
    let target_type = params.r#type;
    let target_id = params.id;

    let resp = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let vote_count: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(value), 0) FROM votes
                 WHERE target_type = ?1 AND target_id = ?2",
                rusqlite::params![target_type, target_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let user_vote = user_id.and_then(|uid| {
            conn.query_row(
                "SELECT value FROM votes
                 WHERE user_id = ?1 AND target_type = ?2 AND target_id = ?3",
                rusqlite::params![uid, target_type, target_id],
                |row| row.get::<_, i32>(0),
            )
            .ok()
        });

        Ok::<_, StatusCode>(VoteResponse {
            vote_count,
            user_vote,
        })
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;

    Ok(Json(resp))
}

/// POST /api/votes — upsert (toggle on re-vote with same value)
pub async fn cast_vote(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateVote>,
) -> Result<Json<VoteResponse>, StatusCode> {
    let user_id = auth::extract_user_id(&headers, &state.jwt_secret)?;

    if payload.value != 1 && payload.value != -1 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let pool = state.db.clone();
    let target_type = payload.target_type.clone();
    let target_id = payload.target_id;
    let value = payload.value;

    let resp = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        // Check if user already voted
        let existing: Option<i32> = conn
            .query_row(
                "SELECT value FROM votes
                 WHERE user_id = ?1 AND target_type = ?2 AND target_id = ?3",
                rusqlite::params![user_id, target_type, target_id],
                |row| row.get(0),
            )
            .ok();

        let user_vote = match existing {
            Some(v) if v == value => {
                // Same vote → remove (toggle off)
                conn.execute(
                    "DELETE FROM votes WHERE user_id = ?1 AND target_type = ?2 AND target_id = ?3",
                    rusqlite::params![user_id, target_type, target_id],
                )
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                None
            }
            Some(_) => {
                // Different vote → update
                conn.execute(
                    "UPDATE votes SET value = ?4
                     WHERE user_id = ?1 AND target_type = ?2 AND target_id = ?3",
                    rusqlite::params![user_id, target_type, target_id, value],
                )
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                Some(value)
            }
            None => {
                // New vote → insert
                conn.execute(
                    "INSERT INTO votes (user_id, target_type, target_id, value)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![user_id, target_type, target_id, value],
                )
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                Some(value)
            }
        };

        let vote_count: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(value), 0) FROM votes
                 WHERE target_type = ?1 AND target_id = ?2",
                rusqlite::params![target_type, target_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        Ok::<_, StatusCode>(VoteResponse {
            vote_count,
            user_vote,
        })
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;

    Ok(Json(resp))
}
