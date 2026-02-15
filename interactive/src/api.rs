use gloo_net::http::Request;
use serde::de::DeserializeOwned;
use serde::Serialize;
use web_sys::window;

fn api_base() -> String {
    // Read from a meta tag set by Hugo, falling back to localhost for dev
    let document = window().unwrap().document().unwrap();
    if let Some(el) = document.query_selector("meta[name='mikaana-api']").ok().flatten() {
        if let Some(url) = el.get_attribute("content") {
            if !url.is_empty() {
                return url;
            }
        }
    }
    "http://localhost:8080".to_string()
}

fn get_token() -> Option<String> {
    window()?
        .local_storage()
        .ok()??
        .get_item("mikaana_token")
        .ok()?
}

pub fn set_token(token: &str) {
    if let Some(storage) = window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
    {
        let _ = storage.set_item("mikaana_token", token);
    }
}

pub fn clear_token() {
    if let Some(storage) = window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
    {
        let _ = storage.remove_item("mikaana_token");
    }
}

pub fn has_token() -> bool {
    get_token().is_some()
}

pub async fn get<T: DeserializeOwned>(path: &str) -> Result<T, String> {
    let url = format!("{}{}", api_base(), path);
    let mut req = Request::get(&url);

    if let Some(token) = get_token() {
        req = req.header("Authorization", &format!("Bearer {}", token));
    }

    let resp = req.send().await.map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(format!("API error: {}", resp.status()));
    }

    resp.json().await.map_err(|e| e.to_string())
}

pub async fn post<T: DeserializeOwned, B: Serialize>(path: &str, body: &B) -> Result<T, String> {
    let url = format!("{}{}", api_base(), path);
    let mut req = Request::post(&url).header("Content-Type", "application/json");

    if let Some(token) = get_token() {
        req = req.header("Authorization", &format!("Bearer {}", token));
    }

    let req = req.body(serde_json::to_string(body).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;

    let resp = req.send().await.map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(format!("API error: {}", resp.status()));
    }

    resp.json().await.map_err(|e| e.to_string())
}

pub async fn delete(path: &str) -> Result<(), String> {
    let url = format!("{}{}", api_base(), path);
    let mut req = Request::delete(&url);

    if let Some(token) = get_token() {
        req = req.header("Authorization", &format!("Bearer {}", token));
    }

    let resp = req.send().await.map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(format!("API error: {}", resp.status()));
    }

    Ok(())
}

/// Build the GitHub login URL, passing the current page as the redirect target.
pub fn github_login_url() -> String {
    let current_url = window()
        .and_then(|w| w.location().href().ok())
        .unwrap_or_default();
    format!(
        "{}/api/auth/github?redirect={}",
        api_base(),
        urlencoding(&current_url)
    )
}

fn urlencoding(s: &str) -> String {
    web_sys::js_sys::encode_uri_component(s).as_string().unwrap_or_default()
}
