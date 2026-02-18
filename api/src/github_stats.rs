use axum::extract::Query;
use axum::http::StatusCode;
use axum::Json;
use mikaana_shared::GitHubStats;
use serde::Deserialize;
use std::sync::LazyLock;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
struct CachedStats {
    stats: GitHubStats,
    fetched_at: std::time::Instant,
}

static CACHE: LazyLock<RwLock<Option<CachedStats>>> = LazyLock::new(|| RwLock::new(None));

const CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(3600);

#[derive(Deserialize)]
pub struct StatsQuery {
    pub repo: String,
}

#[derive(Deserialize)]
struct RepoInfo {
    stargazers_count: i64,
    forks_count: i64,
    open_issues_count: i64,
    pushed_at: String,
}

pub async fn get_github_stats(
    Query(query): Query<StatsQuery>,
) -> Result<Json<GitHubStats>, StatusCode> {
    // Check cache
    {
        let cache = CACHE.read().await;
        if let Some(ref cached) = *cache {
            if cached.fetched_at.elapsed() < CACHE_TTL {
                return Ok(Json(cached.stats.clone()));
            }
        }
    }

    // Fetch fresh data
    let stats = fetch_stats(&query.repo).await.map_err(|e| {
        eprintln!("GitHub API error: {e}");
        StatusCode::BAD_GATEWAY
    })?;

    // Update cache
    {
        let mut cache = CACHE.write().await;
        *cache = Some(CachedStats {
            stats: stats.clone(),
            fetched_at: std::time::Instant::now(),
        });
    }

    Ok(Json(stats))
}

async fn fetch_stats(repo: &str) -> Result<GitHubStats, String> {
    let client = reqwest::Client::builder()
        .user_agent("mikaana-api")
        .build()
        .map_err(|e| e.to_string())?;

    let base = format!("https://api.github.com/repos/{repo}");

    // Fetch repo info
    let repo_info: RepoInfo = client
        .get(&base)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    // Fetch languages (bytes per language)
    let languages: std::collections::HashMap<String, i64> = client
        .get(format!("{base}/languages"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let rust_bytes = languages.get("Rust").copied().unwrap_or(0);
    let lines_of_code = rust_bytes / 53; // ~53 bytes per line of Rust (measured against actual LOC)

    // Get commit count from Link header
    let commits_resp = client
        .get(format!("{base}/commits?per_page=1"))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let commits = if let Some(link) = commits_resp.headers().get("link") {
        let link_str = link.to_str().unwrap_or("");
        parse_last_page(link_str)
    } else {
        0
    };

    // Get crate count from contents API
    let crate_count = match client
        .get(format!("{base}/contents/crates"))
        .send()
        .await
    {
        Ok(resp) => {
            let entries: Vec<serde_json::Value> =
                resp.json().await.unwrap_or_default();
            // +2 for root binary crate and vscode extension
            let dir_count = entries
                .iter()
                .filter(|e| e.get("type").and_then(|t| t.as_str()) == Some("dir"))
                .count() as i64;
            dir_count + 2
        }
        Err(_) => 0,
    };

    Ok(GitHubStats {
        commits,
        lines_of_code,
        crate_count,
        stars: repo_info.stargazers_count,
        forks: repo_info.forks_count,
        open_issues: repo_info.open_issues_count,
        last_push: repo_info.pushed_at,
    })
}

fn parse_last_page(link_header: &str) -> i64 {
    for part in link_header.split(',') {
        if part.contains("rel=\"last\"") {
            if let Some(start) = part.rfind("page=") {
                let rest = &part[start + 5..];
                if let Some(end) = rest.find('>') {
                    return rest[..end].parse().unwrap_or(0);
                }
            }
        }
    }
    0
}
