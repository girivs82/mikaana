use leptos::prelude::*;
use mikaana_shared::User;
use wasm_bindgen_futures::spawn_local;
use web_sys::window;

use crate::api;

/// Reactive auth state shared via context.
#[derive(Clone, Debug)]
pub struct AuthState {
    pub user: RwSignal<Option<User>>,
    pub token: RwSignal<Option<String>>,
}

impl AuthState {
    pub fn is_logged_in(&self) -> bool {
        self.token.get_untracked().is_some()
    }
}

/// Check the URL for a `?token=...` param (set after OAuth callback),
/// store it, and clean the URL.
fn consume_url_token() -> Option<String> {
    let win = window()?;
    let href = win.location().href().ok()?;
    let url = web_sys::Url::new(&href).ok()?;
    let params = url.search_params();
    let token = params.get("token");

    if let Some(ref t) = token {
        api::set_token(t);
        // Remove ?token= from the visible URL
        params.delete("token");
        let clean = if params.to_string().as_string().map_or(true, |s| s.is_empty()) {
            url.pathname()
        } else {
            format!("{}?{}", url.pathname(), params.to_string())
        };
        let _ = win.history().ok().map(|h| {
            let _ = h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&clean));
        });
    }

    token
}

/// Provider component — wraps children with auth context.
#[component]
pub fn AuthProvider(children: Children) -> impl IntoView {
    // Check for token from URL (OAuth redirect) or localStorage
    let initial_token = consume_url_token().or_else(|| {
        window()?
            .local_storage()
            .ok()??
            .get_item("mikaana_token")
            .ok()?
    });

    let token = RwSignal::new(initial_token);
    let user: RwSignal<Option<User>> = RwSignal::new(None);

    let auth = AuthState {
        user,
        token: token,
    };
    provide_context(auth.clone());

    // Fetch user profile when we have a token
    Effect::new(move |_| {
        if let Some(_t) = token.get() {
            spawn_local(async move {
                match api::get::<User>("/api/auth/me").await {
                    Ok(u) => user.set(Some(u)),
                    Err(_) => {
                        // Token invalid — clear it
                        api::clear_token();
                        token.set(None);
                        user.set(None);
                    }
                }
            });
        } else {
            user.set(None);
        }
    });

    children()
}

/// Login / logout button.
#[component]
pub fn LoginButton() -> impl IntoView {
    let auth = expect_context::<AuthState>();

    let on_logout = move |_| {
        api::clear_token();
        auth.token.set(None);
        auth.user.set(None);
    };

    move || {
        if let Some(user) = auth.user.get() {
            view! {
                <div class="mikaana-auth">
                    <img src={user.avatar_url.clone()} alt="" class="mikaana-avatar" width="24" height="24" />
                    <span class="mikaana-username">{user.username.clone()}</span>
                    <button class="mikaana-btn mikaana-btn-sm" on:click=on_logout>"Logout"</button>
                </div>
            }
            .into_any()
        } else {
            let url = api::github_login_url();
            view! {
                <a class="mikaana-btn" href={url}>"Login with GitHub"</a>
            }
            .into_any()
        }
    }
}
