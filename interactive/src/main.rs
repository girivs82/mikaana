mod api;
mod auth;
mod comments;
mod forum;
mod github_stats;
mod votes;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

fn main() {
    console_error_panic_hook::set_once();

    let document = web_sys::window()
        .expect("no window")
        .document()
        .expect("no document");

    // Mount comment section if the mount point exists
    if let Some(el) = document.get_element_by_id("mikaana-comments") {
        let slug = el.get_attribute("data-slug").unwrap_or_default();
        let html_el: web_sys::HtmlElement = el.unchecked_into();
        leptos::mount::mount_to(html_el, move || {
            view! {
                <auth::AuthProvider>
                    <comments::CommentSection slug=slug.clone() />
                </auth::AuthProvider>
            }
        })
        .forget();
    }

    // Mount post-level vote buttons if the mount point exists
    if let Some(el) = document.get_element_by_id("mikaana-votes") {
        let slug = el.get_attribute("data-slug").unwrap_or_default();
        let html_el: web_sys::HtmlElement = el.unchecked_into();
        leptos::mount::mount_to(html_el, move || {
            view! {
                <auth::AuthProvider>
                    <votes::PostVotes slug=slug.clone() />
                </auth::AuthProvider>
            }
        })
        .forget();
    }

    // Mount GitHub stats widget if the mount point exists
    if let Some(el) = document.get_element_by_id("mikaana-github-stats") {
        let repo = el.get_attribute("data-repo").unwrap_or_default();
        let html_el: web_sys::HtmlElement = el.unchecked_into();
        leptos::mount::mount_to(html_el, move || {
            view! { <github_stats::RepoStats repo=repo.clone() /> }
        })
        .forget();
    }

    // Mount forum SPA if the mount point exists
    if let Some(el) = document.get_element_by_id("mikaana-forum") {
        let html_el: web_sys::HtmlElement = el.unchecked_into();
        leptos::mount::mount_to(html_el, move || {
            view! {
                <auth::AuthProvider>
                    <forum::ForumApp />
                </auth::AuthProvider>
            }
        })
        .forget();
    }
}
