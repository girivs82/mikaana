use serde::{Deserialize, Serialize};

// ── Auth ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub avatar_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: User,
}

// ── Comments ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: i64,
    pub post_slug: String,
    pub user: User,
    pub body: String,
    pub created_at: String,
    pub vote_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateComment {
    pub post_slug: String,
    pub body: String,
}

// ── Votes ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVote {
    pub target_type: String,
    pub target_id: i64,
    pub value: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteResponse {
    pub vote_count: i64,
    pub user_vote: Option<i32>,
}

// ── Forum ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForumCategory {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub id: i64,
    pub category_id: i64,
    pub user: User,
    pub title: String,
    pub body: String,
    pub created_at: String,
    pub reply_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateThread {
    pub category_slug: String,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub id: i64,
    pub thread_id: i64,
    pub user: User,
    pub body: String,
    pub created_at: String,
    pub vote_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateReply {
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paginated<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}
