use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;
use mikaana_shared::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::auth::{AuthState, LoginButton};
use crate::votes::VoteButton;

/// Top-level forum SPA — mounted on /discuss/*.
#[component]
pub fn ForumApp() -> impl IntoView {
    view! {
        <Router>
            <div class="mikaana-forum">
                <h2><a href="/discuss/">"Discuss"</a></h2>
                <LoginButton />
                <Routes fallback=|| view! { <p>"Page not found."</p> }>
                    <Route path=path!("/discuss/") view=CategoryList />
                    <Route path=path!("/discuss/:cat_slug") view=ThreadList />
                    <Route path=path!("/discuss/thread/:id") view=ThreadView />
                </Routes>
            </div>
        </Router>
    }
}

// ── Categories ──

#[component]
fn CategoryList() -> impl IntoView {
    let cats: RwSignal<Vec<ForumCategory>> = RwSignal::new(Vec::new());
    let loading = RwSignal::new(true);

    spawn_local(async move {
        if let Ok(c) = api::get::<Vec<ForumCategory>>("/api/forum/categories").await {
            cats.set(c);
        }
        loading.set(false);
    });

    view! {
        <section class="mikaana-categories">
            <h3>"Categories"</h3>
            <Show when=move || loading.get()>
                <p class="mikaana-loading">"Loading..."</p>
            </Show>
            <div class="mikaana-category-grid">
                <For
                    each=move || cats.get()
                    key=|c| c.id
                    let:cat
                >
                    <a class="mikaana-category-card" href={format!("/discuss/{}", cat.slug)}>
                        <h4>{cat.name.clone()}</h4>
                        <p>{cat.description.clone()}</p>
                    </a>
                </For>
            </div>
        </section>
    }
}

// ── Threads in a category ──

#[component]
fn ThreadList() -> impl IntoView {
    let params = leptos_router::hooks::use_params_map();
    let threads: RwSignal<Vec<Thread>> = RwSignal::new(Vec::new());
    let loading = RwSignal::new(true);
    let page = RwSignal::new(1i64);
    let total = RwSignal::new(0i64);
    let show_form = RwSignal::new(false);

    let cat_slug = Memo::new(move |_| {
        params.get().get("cat_slug").unwrap_or_default()
    });

    // Fetch threads when page or slug changes
    Effect::new(move |_| {
        let slug = cat_slug.get();
        let p = page.get();
        loading.set(true);
        spawn_local(async move {
            let url = format!("/api/forum/threads?category={}&page={}", slug, p);
            if let Ok(result) = api::get::<Paginated<Thread>>(&url).await {
                threads.set(result.items);
                total.set(result.total);
            }
            loading.set(false);
        });
    });

    view! {
        <section class="mikaana-threads">
            <h3>{move || format!("Threads in {}", cat_slug.get())}</h3>
            <button class="mikaana-btn" on:click=move |_| show_form.update(|v| *v = !*v)>
                {move || if show_form.get() { "Cancel" } else { "New Thread" }}
            </button>
            <Show when=move || show_form.get()>
                <NewThreadForm cat_slug=cat_slug.get_untracked() threads=threads show_form=show_form />
            </Show>
            <Show when=move || loading.get()>
                <p class="mikaana-loading">"Loading..."</p>
            </Show>
            <div class="mikaana-thread-list">
                <For
                    each=move || threads.get()
                    key=|t| t.id
                    let:thread
                >
                    <a class="mikaana-thread-card" href={format!("/discuss/thread/{}", thread.id)}>
                        <div class="mikaana-thread-title">{thread.title.clone()}</div>
                        <div class="mikaana-thread-meta">
                            <span>{thread.user.username.clone()}</span>
                            <time>{thread.created_at.clone()}</time>
                            <span>{format!("{} replies", thread.reply_count)}</span>
                        </div>
                    </a>
                </For>
            </div>
            // Pagination
            <div class="mikaana-pagination">
                <button
                    class="mikaana-btn mikaana-btn-sm"
                    disabled=move || page.get() <= 1
                    on:click=move |_| page.update(|p| *p -= 1)
                >
                    "Prev"
                </button>
                <span>{move || format!("Page {}", page.get())}</span>
                <button
                    class="mikaana-btn mikaana-btn-sm"
                    disabled=move || page.get() * 20 >= total.get()
                    on:click=move |_| page.update(|p| *p += 1)
                >
                    "Next"
                </button>
            </div>
        </section>
    }
}

/// New thread form.
#[component]
fn NewThreadForm(
    cat_slug: String,
    threads: RwSignal<Vec<Thread>>,
    show_form: RwSignal<bool>,
) -> impl IntoView {
    let auth = expect_context::<AuthState>();
    let title = RwSignal::new(String::new());
    let body = RwSignal::new(String::new());
    let submitting = RwSignal::new(false);

    let on_submit = {
        let cat_slug = cat_slug.clone();
        move |ev: leptos::ev::SubmitEvent| {
            ev.prevent_default();
            if auth.token.get_untracked().is_none() {
                return;
            }
            submitting.set(true);
            let payload = CreateThread {
                category_slug: cat_slug.clone(),
                title: title.get_untracked(),
                body: body.get_untracked(),
            };
            spawn_local(async move {
                match api::post::<Thread, _>("/api/forum/threads", &payload).await {
                    Ok(t) => {
                        threads.update(|list| list.insert(0, t));
                        title.set(String::new());
                        body.set(String::new());
                        show_form.set(false);
                    }
                    Err(_) => { /* TODO: error */ }
                }
                submitting.set(false);
            });
        }
    };

    view! {
        <form class="mikaana-thread-form" on:submit=on_submit>
            <input
                class="mikaana-input"
                type="text"
                placeholder="Thread title"
                prop:value=move || title.get()
                on:input=move |ev| title.set(event_target_value(&ev))
            />
            <textarea
                class="mikaana-textarea"
                placeholder="Write your post..."
                prop:value=move || body.get()
                on:input=move |ev| body.set(event_target_value(&ev))
            />
            <button class="mikaana-btn" type="submit" disabled=move || submitting.get()>
                {move || if submitting.get() { "Posting..." } else { "Create Thread" }}
            </button>
        </form>
    }
}

// ── Thread detail + replies ──

#[component]
fn ThreadView() -> impl IntoView {
    let params = leptos_router::hooks::use_params_map();
    let thread: RwSignal<Option<Thread>> = RwSignal::new(None);
    let replies: RwSignal<Vec<Reply>> = RwSignal::new(Vec::new());
    let loading = RwSignal::new(true);

    let thread_id = Memo::new(move |_| {
        params.get().get("id").unwrap_or_default()
    });

    Effect::new(move |_| {
        let id = thread_id.get();
        loading.set(true);
        spawn_local(async move {
            #[derive(serde::Deserialize)]
            struct ThreadDetail {
                thread: Thread,
                replies: Vec<Reply>,
            }
            if let Ok(detail) = api::get::<ThreadDetail>(&format!("/api/forum/threads/{}", id)).await
            {
                thread.set(Some(detail.thread));
                replies.set(detail.replies);
            }
            loading.set(false);
        });
    });

    view! {
        <section class="mikaana-thread-view">
            <Show when=move || loading.get()>
                <p class="mikaana-loading">"Loading..."</p>
            </Show>
            {move || {
                thread.get().map(|t| view! {
                    <article class="mikaana-thread-detail">
                        <h3>{t.title.clone()}</h3>
                        <div class="mikaana-thread-meta">
                            <img src={t.user.avatar_url.clone()} alt="" class="mikaana-avatar" width="24" height="24" />
                            <strong>{t.user.username.clone()}</strong>
                            <time>{t.created_at.clone()}</time>
                        </div>
                        <div class="mikaana-thread-body">{t.body.clone()}</div>
                    </article>
                })
            }}
            <h4>{move || format!("Replies ({})", replies.get().len())}</h4>
            <div class="mikaana-reply-list">
                <For
                    each=move || replies.get()
                    key=|r| r.id
                    let:reply
                >
                    <div class="mikaana-reply">
                        <div class="mikaana-reply-header">
                            <img src={reply.user.avatar_url.clone()} alt="" class="mikaana-avatar" width="24" height="24" />
                            <strong>{reply.user.username.clone()}</strong>
                            <time>{reply.created_at.clone()}</time>
                        </div>
                        <p>{reply.body.clone()}</p>
                        <VoteButton target_type="reply".to_string() target_id=reply.id initial_count=reply.vote_count />
                    </div>
                </For>
            </div>
            <ReplyForm thread_id=thread_id.get_untracked() replies=replies />
        </section>
    }
}

/// Reply form.
#[component]
fn ReplyForm(thread_id: String, replies: RwSignal<Vec<Reply>>) -> impl IntoView {
    let auth = expect_context::<AuthState>();
    let body = RwSignal::new(String::new());
    let submitting = RwSignal::new(false);

    let on_submit = {
        let tid = thread_id.clone();
        move |ev: leptos::ev::SubmitEvent| {
            ev.prevent_default();
            if auth.token.get_untracked().is_none() {
                return;
            }
            submitting.set(true);
            let payload = CreateReply {
                body: body.get_untracked(),
            };
            let tid = tid.clone();
            spawn_local(async move {
                match api::post::<Reply, _>(
                    &format!("/api/forum/threads/{}/replies", tid),
                    &payload,
                )
                .await
                {
                    Ok(r) => {
                        replies.update(|list| list.push(r));
                        body.set(String::new());
                    }
                    Err(_) => { /* TODO: error */ }
                }
                submitting.set(false);
            });
        }
    };

    move || {
        if auth.user.get().is_some() {
            view! {
                <form class="mikaana-reply-form" on:submit=on_submit.clone()>
                    <textarea
                        class="mikaana-textarea"
                        placeholder="Write a reply..."
                        prop:value=move || body.get()
                        on:input=move |ev| body.set(event_target_value(&ev))
                    />
                    <button class="mikaana-btn" type="submit" disabled=move || submitting.get()>
                        {move || if submitting.get() { "Replying..." } else { "Reply" }}
                    </button>
                </form>
            }
            .into_any()
        } else {
            view! { <p class="mikaana-hint">"Log in to reply."</p> }.into_any()
        }
    }
}
