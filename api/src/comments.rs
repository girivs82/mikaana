use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use mikaana_shared::{Comment, CreateComment, User};
use serde::Deserialize;

use crate::{auth, AppState};

#[derive(Deserialize)]
pub struct ListParams {
    slug: String,
}

/// GET /api/comments?slug=...
pub async fn list_comments(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<Comment>>, StatusCode> {
    let pool = state.db.clone();
    let slug = params.slug;

    let comments = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let mut stmt = conn
            .prepare(
                "SELECT c.id, c.post_slug, c.body, c.created_at,
                        u.id, u.username, u.avatar_url,
                        COALESCE((SELECT SUM(value) FROM votes
                                  WHERE target_type = 'comment' AND target_id = c.id), 0)
                 FROM comments c
                 JOIN users u ON c.user_id = u.id
                 WHERE c.post_slug = ?1
                 ORDER BY c.created_at ASC",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let rows = stmt
            .query_map([&slug], |row| {
                Ok(Comment {
                    id: row.get(0)?,
                    post_slug: row.get(1)?,
                    body: row.get(2)?,
                    created_at: row.get(3)?,
                    user: User {
                        id: row.get(4)?,
                        username: row.get(5)?,
                        avatar_url: row.get(6)?,
                    },
                    vote_count: row.get(7)?,
                })
            })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();

        Ok::<_, StatusCode>(rows)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;

    Ok(Json(comments))
}

/// POST /api/comments
pub async fn create_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateComment>,
) -> Result<Json<Comment>, StatusCode> {
    let user_id = auth::extract_user_id(&headers, &state.jwt_secret)?;
    let body = ammonia::clean(&payload.body);

    if body.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let pool = state.db.clone();
    let slug = payload.post_slug.clone();

    let comment = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        conn.execute(
            "INSERT INTO comments (post_slug, user_id, body) VALUES (?1, ?2, ?3)",
            rusqlite::params![slug, user_id, body],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let id = conn.last_insert_rowid();

        conn.query_row(
            "SELECT c.id, c.post_slug, c.body, c.created_at,
                    u.id, u.username, u.avatar_url
             FROM comments c JOIN users u ON c.user_id = u.id
             WHERE c.id = ?1",
            [id],
            |row| {
                Ok(Comment {
                    id: row.get(0)?,
                    post_slug: row.get(1)?,
                    body: row.get(2)?,
                    created_at: row.get(3)?,
                    user: User {
                        id: row.get(4)?,
                        username: row.get(5)?,
                        avatar_url: row.get(6)?,
                    },
                    vote_count: 0,
                })
            },
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;

    Ok(Json(comment))
}

/// DELETE /api/comments/:id
pub async fn delete_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let user_id = auth::extract_user_id(&headers, &state.jwt_secret)?;

    let pool = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let affected = conn
            .execute(
                "DELETE FROM comments WHERE id = ?1 AND user_id = ?2",
                rusqlite::params![id, user_id],
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if affected == 0 {
            Err(StatusCode::NOT_FOUND)
        } else {
            Ok(StatusCode::NO_CONTENT)
        }
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}
