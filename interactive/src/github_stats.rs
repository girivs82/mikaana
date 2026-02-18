use leptos::prelude::*;
use mikaana_shared::GitHubStats;
use wasm_bindgen_futures::spawn_local;

use crate::api;

#[component]
pub fn RepoStats(repo: String) -> impl IntoView {
    let stats: RwSignal<Option<GitHubStats>> = RwSignal::new(None);

    spawn_local(async move {
        let url = format!(
            "/api/github-stats?repo={}",
            web_sys::js_sys::encode_uri_component(&repo)
        );
        if let Ok(s) = api::get::<GitHubStats>(&url).await {
            stats.set(Some(s));
        }
    });

    move || {
        stats.get().map(|s| {
            let lines = format_lines(s.lines_of_code);
            let commits = format_number(s.commits);
            view! {
                <span class="mikaana-repo-stats">
                    <a href={format!("https://github.com/{}", "girivs82/skalp")}
                       target="_blank" rel="noopener">"GitHub"</a>
                    " | ~" {lines} " lines of Rust"
                    " | " {s.crate_count.to_string()} " workspace crates"
                    " | " {commits} " commits"
                </span>
            }
        })
    }
}

fn format_lines(lines: i64) -> String {
    if lines >= 1000 {
        format!("{}K", lines / 1000)
    } else {
        lines.to_string()
    }
}

fn format_number(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 10_000 {
        format!("{:.0}K", n as f64 / 1_000.0)
    } else {
        // Add commas for thousands
        let s = n.to_string();
        if s.len() > 3 {
            format!("{},{}", &s[..s.len() - 3], &s[s.len() - 3..])
        } else {
            s
        }
    }
}
