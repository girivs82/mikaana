use leptos::prelude::*;
use mikaana_shared::{CreateVote, VoteResponse};
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::auth::AuthState;

/// Upvote / downvote button with count.
#[component]
pub fn VoteButton(target_type: String, target_id: i64, initial_count: i64) -> impl IntoView {
    let count = RwSignal::new(initial_count);
    let user_vote: RwSignal<Option<i32>> = RwSignal::new(None);
    let auth = expect_context::<AuthState>();

    // Fetch current user's vote on mount
    {
        let tt = target_type.clone();
        spawn_local(async move {
            if let Ok(vr) =
                api::get::<VoteResponse>(&format!("/api/votes?type={}&id={}", tt, target_id)).await
            {
                count.set(vr.vote_count);
                user_vote.set(vr.user_vote);
            }
        });
    }

    let cast = {
        let tt = target_type.clone();
        move |value: i32| {
            if auth.token.get_untracked().is_none() {
                return; // must be logged in
            }
            // Optimistic update
            let prev_vote = user_vote.get_untracked();
            let prev_count = count.get_untracked();
            let delta = match prev_vote {
                Some(v) if v == value => -value, // toggling off
                Some(v) => value - v,            // switching
                None => value,                   // new vote
            };
            count.set(prev_count + delta as i64);
            let new_user_vote = if prev_vote == Some(value) {
                None
            } else {
                Some(value)
            };
            user_vote.set(new_user_vote);

            let payload = CreateVote {
                target_type: tt.clone(),
                target_id,
                value,
            };
            spawn_local(async move {
                match api::post::<VoteResponse, _>("/api/votes", &payload).await {
                    Ok(vr) => {
                        count.set(vr.vote_count);
                        user_vote.set(vr.user_vote);
                    }
                    Err(_) => {
                        // Rollback
                        count.set(prev_count);
                        user_vote.set(prev_vote);
                    }
                }
            });
        }
    };

    let cast_up = {
        let cast = cast.clone();
        move |_| cast(1)
    };
    let cast_down = move |_| cast(-1);

    view! {
        <div class="mikaana-votes">
            <button
                class="mikaana-vote-btn"
                class:active=move || user_vote.get() == Some(1)
                on:click=cast_up
                disabled=move || auth.token.get().is_none()
            >
                // Unicode up triangle
                "\u{25B2}"
            </button>
            <span class="mikaana-vote-count">{move || count.get()}</span>
            <button
                class="mikaana-vote-btn"
                class:active=move || user_vote.get() == Some(-1)
                on:click=cast_down
                disabled=move || auth.token.get().is_none()
            >
                "\u{25BC}"
            </button>
        </div>
    }
}

/// Standalone post-level votes (for embedding in extend_footer).
#[component]
pub fn PostVotes(slug: String) -> impl IntoView {
    // Use slug hash as a stable target_id for post-level votes
    let target_id = slug.bytes().fold(0i64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as i64)).abs();

    view! {
        <div class="mikaana-post-votes">
            <span>"Like this post? "</span>
            <VoteButton target_type="post".to_string() target_id=target_id initial_count=0 />
        </div>
    }
}
