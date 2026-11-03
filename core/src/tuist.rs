//! Sign in with Tuist and a small client for its REST API.
//!
//! Sign-in follows Habibi's native workflow: discover the server's OAuth
//! metadata, register a public client dynamically (RFC 7591), and authorize
//! with S256 PKCE in the system browser. The core never stores anything: it
//! returns the pending login and the session, and the edge persists them in
//! its platform's secure storage.

use crate::routes::{self, Params};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const SERVER: &str = "https://tuist.dev";

#[derive(Debug, Serialize)]
pub struct Error {
    pub kind: &'static str,
    pub message: String,
}

impl Error {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// What the edge keeps between starting and finishing a sign-in.
#[derive(Debug, Serialize, Deserialize)]
pub struct PendingSignIn {
    pub client_id: String,
    pub redirect_uri: String,
    pub state: String,
    pub verifier: String,
}

/// What the edge stores in its secure storage after signing in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub client_id: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix seconds.
    pub expires_at: u64,
}

#[derive(Deserialize)]
struct Metadata {
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: String,
}

#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .user_agent("native-edges/0.1")
        .build()
        .into()
}

fn network(error: ureq::Error) -> Error {
    Error::new("network", format!("Could not reach Tuist: {error}"))
}

fn metadata() -> Result<Metadata> {
    let mut response = agent()
        .get(&format!("{SERVER}/.well-known/oauth-authorization-server"))
        .call()
        .map_err(network)?;
    let metadata: Metadata = response.body_mut().read_json().map_err(network)?;
    for endpoint in [
        &metadata.authorization_endpoint,
        &metadata.token_endpoint,
        &metadata.registration_endpoint,
    ] {
        if !endpoint.starts_with(SERVER) {
            return Err(Error::new(
                "invalid_server",
                "OAuth endpoints must live on the Tuist server",
            ));
        }
    }
    Ok(metadata)
}

/// Starts a sign-in. Returns the URL to open in the system browser and the
/// pending state the edge passes back to `complete_sign_in`.
pub fn sign_in(redirect_uri: &str) -> Result<(String, PendingSignIn)> {
    let metadata = metadata()?;
    let mut response = agent()
        .post(&metadata.registration_endpoint)
        .send_json(json!({
            "client_name": "Native Edges",
            "redirect_uris": [redirect_uri],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        }))
        .map_err(network)?;
    if response.status().as_u16() != 201 {
        return Err(Error::new(
            "registration_failed",
            "Tuist did not register the app",
        ));
    }
    let registered: Value = response.body_mut().read_json().map_err(network)?;
    let client_id = registered["client_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| Error::new("registration_failed", "Tuist returned no client ID"))?
        .to_string();

    let verifier = random_token(32)?;
    let state = random_token(16)?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let url = format!(
        "{}?{}",
        metadata.authorization_endpoint,
        form(&[
            ("response_type", "code"),
            ("client_id", &client_id),
            ("redirect_uri", redirect_uri),
            ("state", &state),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ])
    );
    Ok((
        url,
        PendingSignIn {
            client_id,
            redirect_uri: redirect_uri.to_string(),
            state,
            verifier,
        },
    ))
}

/// Finishes a sign-in from the URL the browser redirected to.
pub fn complete_sign_in(pending: PendingSignIn, callback_url: &str) -> Result<Session> {
    let base = callback_url.split(['?', '#']).next().unwrap_or_default();
    if base != pending.redirect_uri {
        return Err(Error::new(
            "invalid_callback",
            "Unexpected sign-in callback",
        ));
    }
    let query = query(callback_url);
    if query.get("state") != Some(&pending.state) {
        return Err(Error::new(
            "invalid_callback",
            "The sign-in state did not match",
        ));
    }
    if let Some(error) = query.get("error") {
        return Err(Error::new(
            "denied",
            format!("Sign-in was not completed: {error}"),
        ));
    }
    let code = query
        .get("code")
        .ok_or_else(|| Error::new("invalid_callback", "The callback had no code"))?;
    token(
        &pending.client_id,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &pending.redirect_uri),
            ("client_id", &pending.client_id),
            ("code_verifier", &pending.verifier),
        ],
        None,
    )
}

fn token(client_id: &str, fields: &[(&str, &str)], previous: Option<&Session>) -> Result<Session> {
    let metadata = metadata()?;
    let mut response = agent()
        .post(&metadata.token_endpoint)
        .content_type("application/x-www-form-urlencoded")
        .send(form(fields))
        .map_err(network)?;
    if response.status().as_u16() != 200 {
        return Err(Error::new("unauthorized", "Tuist rejected the sign-in"));
    }
    let tokens: Tokens = response.body_mut().read_json().map_err(network)?;
    Ok(Session {
        client_id: client_id.to_string(),
        access_token: tokens.access_token,
        refresh_token: tokens
            .refresh_token
            .or_else(|| previous.and_then(|s| s.refresh_token.clone())),
        expires_at: now() + tokens.expires_in.unwrap_or(3600),
    })
}

fn refresh(session: &Session) -> Result<Session> {
    let refresh_token = session
        .refresh_token
        .as_deref()
        .ok_or_else(|| Error::new("unauthorized", "The session expired"))?;
    token(
        &session.client_id,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &session.client_id),
        ],
        Some(session),
    )
}

/// An authenticated GET. Returns the body and, when the access token had to be
/// refreshed, the new session for the edge to persist.
fn get(session: &Session, path: &str) -> Result<(Value, Option<Session>)> {
    let mut renewed = None;
    if session.expires_at <= now() + 60 {
        renewed = Some(refresh(session)?);
    }
    loop {
        let current = renewed.as_ref().unwrap_or(session);
        let mut response = agent()
            .get(&format!("{SERVER}{path}"))
            .header("Authorization", &format!("Bearer {}", current.access_token))
            .header("Accept", "application/json")
            .call()
            .map_err(network)?;
        match response.status().as_u16() {
            200 => {
                let body = response.body_mut().read_json().map_err(network)?;
                return Ok((body, renewed));
            }
            401 if renewed.is_none() => renewed = Some(refresh(session)?),
            401 => return Err(Error::new("unauthorized", "The session expired")),
            404 => return Err(Error::new("not_found", "That project doesn't exist")),
            403 => return Err(Error::new("forbidden", "You don't have access to that")),
            status => {
                return Err(Error::new(
                    "server",
                    format!("Tuist responded with {status}"),
                ));
            }
        }
    }
}

/// The projects screen: one row per project, each with the link that opens it.
pub fn projects(session: &Session) -> Result<(Value, Option<Session>)> {
    let (body, renewed) = get(session, "/api/projects")?;
    let mut rows: Vec<Value> = body["projects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|project| {
            let full_name = project["full_name"].as_str()?;
            let (account, name) = full_name.split_once('/')?;
            let params = Params::from([
                ("account".to_string(), account.to_string()),
                ("project".to_string(), name.to_string()),
            ]);
            Some(json!({
                "id": full_name,
                "title": name,
                "subtitle": account,
                "badge": project["visibility"].as_str().unwrap_or("private"),
                "url": routes::url("project", &params).ok()?,
            }))
        })
        .collect();
    rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    Ok((json!({ "title": "Projects", "rows": rows }), renewed))
}

/// The project screen.
pub fn project(session: &Session, account: &str, name: &str) -> Result<(Value, Option<Session>)> {
    let params = Params::from([
        ("account".to_string(), account.to_string()),
        ("project".to_string(), name.to_string()),
    ]);
    // Validates both handles before they reach a request path.
    let link = routes::url("project", &params)
        .map_err(|_| Error::new("invalid", "Invalid project handle"))?;
    let (body, renewed) = get(session, &format!("/api/projects/{account}/{name}"))?;
    let text = |key: &str| {
        body[key]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let mut fields = vec![
        json!({"label": "Account", "value": account}),
        json!({"label": "Visibility", "value": text("visibility").unwrap_or("private".into())}),
        json!({"label": "Default branch", "value": text("default_branch").unwrap_or("main".into())}),
    ];
    if let Some(build_system) = text("build_system") {
        fields.push(json!({"label": "Build system", "value": build_system}));
    }
    if let Some(repository) = text("repository_url") {
        fields.push(json!({"label": "Repository", "value": repository.trim_end_matches(".git")}));
    }
    Ok((
        json!({
            "title": name,
            "subtitle": format!("{account}/{name}"),
            "fields": fields,
            "dashboard_url": format!("{SERVER}/{account}/{name}"),
            "url": link,
        }),
        renewed,
    ))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn random_token(bytes: usize) -> Result<String> {
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).map_err(|_| Error::new("internal", "No randomness available"))?;
    Ok(URL_SAFE_NO_PAD.encode(buffer))
}

fn form(fields: &[(&str, &str)]) -> String {
    fields
        .iter()
        .map(|(key, value)| format!("{}={}", encode(key), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn query(url: &str) -> std::collections::HashMap<String, String> {
    let query = url
        .split_once('?')
        .map(|(_, q)| q.split('#').next().unwrap_or_default())
        .unwrap_or_default();
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (decode(k), decode(v)))
        .collect()
}

fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending() -> PendingSignIn {
        PendingSignIn {
            client_id: "client".into(),
            redirect_uri: "edges://oauth-callback".into(),
            state: "s1".into(),
            verifier: "v".into(),
        }
    }

    #[test]
    fn callbacks_must_match_the_redirect_and_state() {
        let wrong_state = complete_sign_in(pending(), "edges://oauth-callback?code=c&state=s2");
        assert_eq!(wrong_state.unwrap_err().kind, "invalid_callback");
        let wrong_target = complete_sign_in(pending(), "evil://oauth-callback?code=c&state=s1");
        assert_eq!(wrong_target.unwrap_err().kind, "invalid_callback");
        let denied = complete_sign_in(
            pending(),
            "edges://oauth-callback?error=access_denied&state=s1",
        );
        assert_eq!(denied.unwrap_err().kind, "denied");
    }

    #[test]
    fn query_values_are_percent_decoded() {
        let q = query("edges://oauth-callback?code=a%2Fb%3D&state=x+y#frag");
        assert_eq!(q["code"], "a/b=");
        assert_eq!(q["state"], "x y");
        assert_eq!(encode("edges://cb?x=1"), "edges%3A%2F%2Fcb%3Fx%3D1");
    }
}
