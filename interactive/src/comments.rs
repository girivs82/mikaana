use leptos::prelude::*;
use mikaana_shared::{Comment, CreateComment};
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::auth::{AuthState, LoginButton};
use crate::votes::VoteButton;

/// Top-level comment section for a blog post.
#[component]
pub fn CommentSection(slug: String) -> impl IntoView {
    let comments: RwSignal<Vec<Comment>> = RwSignal::new(Vec::new());
    let loading = RwSignal::new(true);
    let error: RwSignal<Option<String>> = RwSignal::new(None);

    // Fetch comments on mount
    {
        let slug = slug.clone();
        spawn_local(async move {
            match api::get::<Vec<Comment>>(&format!("/api/comments?slug={}", slug)).await {
                Ok(c) => comments.set(c),
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    }

    view! {
        <section class="mikaana-comments">
            <h3>"Comments"</h3>
            <LoginButton />
            <CommentForm slug=slug.clone() comments=comments />
            <Show when=move || loading.get()>
                <p class="mikaana-loading">"Loading comments..."</p>
            </Show>
            <Show when=move || error.get().is_some()>
                <p class="mikaana-error">{move || error.get().unwrap_or_default()}</p>
            </Show>
            <div class="mikaana-comment-list">
                <For
                    each=move || comments.get()
                    key=|c| c.id
                    let:comment
                >
                    <CommentItem comment=comment comments=comments />
                </For>
            </div>
        </section>
    }
}

/// Form for posting a new comment.
#[component]
fn CommentForm(slug: String, comments: RwSignal<Vec<Comment>>) -> impl IntoView {
    let auth = expect_context::<AuthState>();
    let body = RwSignal::new(String::new());
    let submitting = RwSignal::new(false);

    let on_submit = {
        let slug = slug.clone();
        move |ev: leptos::ev::SubmitEvent| {
            ev.prevent_default();
            let text = body.get_untracked();
            if text.trim().is_empty() {
                return;
            }
            submitting.set(true);
            let slug = slug.clone();
            spawn_local(async move {
                let payload = CreateComment {
                    post_slug: slug,
                    body: text,
                };
                match api::post::<Comment, _>("/api/comments", &payload).await {
                    Ok(c) => {
                        comments.update(|list| list.push(c));
                        body.set(String::new());
                    }
                    Err(_e) => { /* TODO: show error */ }
                }
                submitting.set(false);
            });
        }
    };

    move || {
        if auth.user.get().is_some() {
            view! {
                <form class="mikaana-comment-form" on:submit=on_submit.clone()>
                    <textarea
                        class="mikaana-textarea"
                        placeholder="Write a comment..."
                        prop:value=move || body.get()
                        on:input=move |ev| {
                            body.set(event_target_value(&ev));
                        }
                    />
                    <button
                        class="mikaana-btn"
                        type="submit"
                        disabled=move || submitting.get()
                    >
                        {move || if submitting.get() { "Posting..." } else { "Post Comment" }}
                    </button>
                </form>
            }
            .into_any()
        } else {
            view! { <p class="mikaana-hint">"Log in to comment."</p> }.into_any()
        }
    }
}

/// Single comment display.
#[component]
fn CommentItem(comment: Comment, comments: RwSignal<Vec<Comment>>) -> impl IntoView {
    let auth = expect_context::<AuthState>();
    let comment_id = comment.id;
    let is_own = move || {
        auth.user
            .get()
            .map(|u| u.id == comment.user.id)
            .unwrap_or(false)
    };

    let on_delete = move |_| {
        spawn_local(async move {
            if api::delete(&format!("/api/comments/{}", comment_id))
                .await
                .is_ok()
            {
                comments.update(|list| list.retain(|c| c.id != comment_id));
            }
        });
    };

    view! {
        <div class="mikaana-comment">
            <div class="mikaana-comment-header">
                <img src={comment.user.avatar_url.clone()} alt="" class="mikaana-avatar" width="24" height="24" />
                <strong>{comment.user.username.clone()}</strong>
                <time>{comment.created_at.clone()}</time>
                <Show when=is_own>
                    <button class="mikaana-btn mikaana-btn-sm mikaana-btn-danger" on:click=on_delete>"Delete"</button>
                </Show>
            </div>
            <p class="mikaana-comment-body">{comment.body.clone()}</p>
            <VoteButton target_type="comment".to_string() target_id=comment.id initial_count=comment.vote_count />
        </div>
    }
}
