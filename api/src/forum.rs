use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use mikaana_shared::*;
use serde::{Deserialize, Serialize};

use crate::{auth, AppState};

// ── Query params ──

#[derive(Deserialize)]
pub struct ThreadListParams {
    category: String,
    page: Option<i64>,
}

// ── Response for thread detail ──

#[derive(Serialize)]
pub struct ThreadDetail {
    pub thread: Thread,
    pub replies: Vec<Reply>,
}

// ── Handlers ──

/// GET /api/forum/categories
pub async fn list_categories(
    State(state): State<AppState>,
) -> Result<Json<Vec<ForumCategory>>, StatusCode> {
    let pool = state.db.clone();

    let cats = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let mut stmt = conn
            .prepare("SELECT id, name, slug, description FROM categories ORDER BY id")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let rows = stmt
            .query_map([], |row| {
                Ok(ForumCategory {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    slug: row.get(2)?,
                    description: row.get(3)?,
                })
            })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();

        Ok::<_, StatusCode>(rows)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;

    Ok(Json(cats))
}

/// GET /api/forum/threads?category=general&page=1
pub async fn list_threads(
    State(state): State<AppState>,
    Query(params): Query<ThreadListParams>,
) -> Result<Json<Paginated<Thread>>, StatusCode> {
    let pool = state.db.clone();
    let cat_slug = params.category;
    let page = params.page.unwrap_or(1).max(1);
    let per_page: i64 = 20;
    let offset = (page - 1) * per_page;

    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        // Get category id
        let cat_id: i64 = conn
            .query_row(
                "SELECT id FROM categories WHERE slug = ?1",
                [&cat_slug],
                |row| row.get(0),
            )
            .map_err(|_| StatusCode::NOT_FOUND)?;

        // Total count
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE category_id = ?1",
                [cat_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // Threads
        let mut stmt = conn
            .prepare(
                "SELECT t.id, t.category_id, t.title, t.body, t.created_at,
                        u.id, u.username, u.avatar_url,
                        (SELECT COUNT(*) FROM replies WHERE thread_id = t.id)
                 FROM threads t
                 JOIN users u ON t.user_id = u.id
                 WHERE t.category_id = ?1
                 ORDER BY t.created_at DESC
                 LIMIT ?2 OFFSET ?3",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let threads = stmt
            .query_map(rusqlite::params![cat_id, per_page, offset], |row| {
                Ok(Thread {
                    id: row.get(0)?,
                    category_id: row.get(1)?,
                    title: row.get(2)?,
                    body: row.get(3)?,
                    created_at: row.get(4)?,
                    user: User {
                        id: row.get(5)?,
                        username: row.get(6)?,
                        avatar_url: row.get(7)?,
                    },
                    reply_count: row.get(8)?,
                })
            })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();

        Ok::<_, StatusCode>(Paginated {
            items: threads,
            total,
            page,
            per_page,
        })
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;

    Ok(Json(result))
}

/// POST /api/forum/threads
pub async fn create_thread(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateThread>,
) -> Result<Json<Thread>, StatusCode> {
    let user_id = auth::extract_user_id(&headers, &state.jwt_secret)?;
    let title = ammonia::clean(&payload.title);
    let body = ammonia::clean(&payload.body);

    if title.trim().is_empty() || body.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let pool = state.db.clone();
    let cat_slug = payload.category_slug;

    let thread = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let cat_id: i64 = conn
            .query_row(
                "SELECT id FROM categories WHERE slug = ?1",
                [&cat_slug],
                |row| row.get(0),
            )
            .map_err(|_| StatusCode::NOT_FOUND)?;

        conn.execute(
            "INSERT INTO threads (category_id, user_id, title, body) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![cat_id, user_id, title, body],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let id = conn.last_insert_rowid();

        conn.query_row(
            "SELECT t.id, t.category_id, t.title, t.body, t.created_at,
                    u.id, u.username, u.avatar_url
             FROM threads t JOIN users u ON t.user_id = u.id
             WHERE t.id = ?1",
            [id],
            |row| {
                Ok(Thread {
                    id: row.get(0)?,
                    category_id: row.get(1)?,
                    title: row.get(2)?,
                    body: row.get(3)?,
                    created_at: row.get(4)?,
                    user: User {
                        id: row.get(5)?,
                        username: row.get(6)?,
                        avatar_url: row.get(7)?,
                    },
                    reply_count: 0,
                })
            },
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;

    Ok(Json(thread))
}

/// GET /api/forum/threads/:id
pub async fn get_thread(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<ThreadDetail>, StatusCode> {
    let pool = state.db.clone();

    let detail = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let thread = conn
            .query_row(
                "SELECT t.id, t.category_id, t.title, t.body, t.created_at,
                        u.id, u.username, u.avatar_url,
                        (SELECT COUNT(*) FROM replies WHERE thread_id = t.id)
                 FROM threads t JOIN users u ON t.user_id = u.id
                 WHERE t.id = ?1",
                [id],
                |row| {
                    Ok(Thread {
                        id: row.get(0)?,
                        category_id: row.get(1)?,
                        title: row.get(2)?,
                        body: row.get(3)?,
                        created_at: row.get(4)?,
                        user: User {
                            id: row.get(5)?,
                            username: row.get(6)?,
                            avatar_url: row.get(7)?,
                        },
                        reply_count: row.get(8)?,
                    })
                },
            )
            .map_err(|_| StatusCode::NOT_FOUND)?;

        let mut stmt = conn
            .prepare(
                "SELECT r.id, r.thread_id, r.body, r.created_at,
                        u.id, u.username, u.avatar_url,
                        COALESCE((SELECT SUM(value) FROM votes
                                  WHERE target_type = 'reply' AND target_id = r.id), 0)
                 FROM replies r
                 JOIN users u ON r.user_id = u.id
                 WHERE r.thread_id = ?1
                 ORDER BY r.created_at ASC",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let replies = stmt
            .query_map([id], |row| {
                Ok(Reply {
                    id: row.get(0)?,
                    thread_id: row.get(1)?,
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

        Ok::<_, StatusCode>(ThreadDetail { thread, replies })
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;

    Ok(Json(detail))
}

/// POST /api/forum/threads/:id/replies
pub async fn create_reply(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(thread_id): Path<i64>,
    Json(payload): Json<CreateReply>,
) -> Result<Json<Reply>, StatusCode> {
    let user_id = auth::extract_user_id(&headers, &state.jwt_secret)?;
    let body = ammonia::clean(&payload.body);

    if body.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let pool = state.db.clone();

    let reply = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        // Verify thread exists
        let _: i64 = conn
            .query_row("SELECT id FROM threads WHERE id = ?1", [thread_id], |row| {
                row.get(0)
            })
            .map_err(|_| StatusCode::NOT_FOUND)?;

        conn.execute(
            "INSERT INTO replies (thread_id, user_id, body) VALUES (?1, ?2, ?3)",
            rusqlite::params![thread_id, user_id, body],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let id = conn.last_insert_rowid();

        conn.query_row(
            "SELECT r.id, r.thread_id, r.body, r.created_at,
                    u.id, u.username, u.avatar_url
             FROM replies r JOIN users u ON r.user_id = u.id
             WHERE r.id = ?1",
            [id],
            |row| {
                Ok(Reply {
                    id: row.get(0)?,
                    thread_id: row.get(1)?,
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

    Ok(Json(reply))
}
