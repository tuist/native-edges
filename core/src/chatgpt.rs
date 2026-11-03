//! Sign in with ChatGPT and talk to it.
//!
//! This is Habibi's workflow: OpenAI's device-code sign-in with the public
//! Codex client that Pi uses (not a secret), and the Codex responses endpoint.
//! The core never touches the Keychain or the Keystore: it hands the session to
//! the edge's `SecureStorage`.

use crate::{Error, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const AUTH: &str = "https://auth.openai.com";
const RESPONSES: &str = "https://chatgpt.com/backend-api/codex/responses";
pub const VERIFICATION_URL: &str = "https://auth.openai.com/codex/device";
pub const MODEL: &str = "gpt-6.1-sol";

/// What the edge shows while the person enters the code in a browser.
#[derive(Debug, Clone, uniffi::Record)]
pub struct DeviceCode {
    pub device_auth_id: String,
    pub user_code: String,
    pub verification_url: String,
    /// Seconds the edge waits between polls.
    pub interval: u64,
}

/// What the core keeps in the edge's secure storage after signing in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub account_id: String,
    /// Unix seconds.
    pub expires_at: u64,
}

/// Starts a sign-in: the person enters `user_code` at `VERIFICATION_URL`.
pub fn sign_in() -> Result<DeviceCode> {
    let (status, body) = post_json(
        &format!("{AUTH}/api/accounts/deviceauth/usercode"),
        json!({ "client_id": CLIENT_ID }),
    )?;
    if status != 200 {
        return Err(Error::new(
            "unavailable",
            format!("OpenAI declined to start sign-in (HTTP {status})"),
        ));
    }
    let text = |key: &str| {
        body[key]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let interval = body["interval"]
        .as_u64()
        .or_else(|| body["interval"].as_str()?.parse().ok())
        .unwrap_or(5);
    Ok(DeviceCode {
        device_auth_id: text("device_auth_id").ok_or_else(invalid)?,
        verification_url: VERIFICATION_URL.into(),
        user_code: text("user_code")
            .or_else(|| text("usercode"))
            .ok_or_else(invalid)?,
        interval: interval.clamp(5, 60),
    })
}

/// Checks whether the person entered the code. `None` means not yet.
pub fn poll_sign_in(pending: &DeviceCode) -> Result<Option<Session>> {
    let (status, body) = post_json(
        &format!("{AUTH}/api/accounts/deviceauth/token"),
        json!({ "device_auth_id": pending.device_auth_id, "user_code": pending.user_code }),
    )?;
    let error = body["error"]["code"].as_str().or(body["error"].as_str());
    if matches!(status, 403 | 404)
        || matches!(
            error,
            Some("deviceauth_authorization_pending" | "slow_down")
        )
    {
        return Ok(None);
    }
    if status != 200 {
        return Err(Error::new(
            "denied",
            "ChatGPT sign-in failed. Enable device-code login in ChatGPT's security settings and try again.",
        ));
    }
    let code = body["authorization_code"].as_str().ok_or_else(invalid)?;
    let verifier = body["code_verifier"].as_str().ok_or_else(invalid)?;
    token(&[
        ("grant_type", "authorization_code"),
        ("client_id", CLIENT_ID),
        ("code", code),
        ("code_verifier", verifier),
        (
            "redirect_uri",
            "https://auth.openai.com/deviceauth/callback",
        ),
    ])
    .map(Some)
}

/// Sends a conversation to ChatGPT and returns the reply's output items. When
/// the access token had to be refreshed, also returns the new session for the
/// edge to persist.
pub fn respond(session: &Session, body: Value) -> Result<(Vec<Value>, Option<Session>)> {
    let renewed = if session.expires_at <= now() + 60 {
        Some(token(&[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", &session.refresh_token),
        ])?)
    } else {
        None
    };
    let current = renewed.as_ref().unwrap_or(session);
    let mut request = body;
    request["model"] = json!(MODEL);
    request["store"] = json!(false);
    request["stream"] = json!(true);
    request["include"] = json!(["reasoning.encrypted_content"]);
    let mut response = agent(120)
        .post(RESPONSES)
        .header("Authorization", &format!("Bearer {}", current.access_token))
        .header("ChatGPT-Account-Id", &current.account_id)
        .header("originator", "native_edges")
        .header("OpenAI-Beta", "responses=experimental")
        .header("Accept", "text/event-stream")
        .send_json(request)
        .map_err(network)?;
    match response.status().as_u16() {
        200 => {}
        401 => return Err(Error::new("unauthorized", "Sign in with ChatGPT again")),
        429 => {
            return Err(Error::new(
                "limited",
                "ChatGPT usage limit reached. Try again later.",
            ));
        }
        status => {
            return Err(Error::new(
                "server",
                format!("ChatGPT responded with {status}"),
            ));
        }
    }
    let stream = response.body_mut().read_to_string().map_err(network)?;
    Ok((completed_output(&stream)?, renewed))
}

/// Reads the server-sent events and keeps only a completed reply.
fn completed_output(stream: &str) -> Result<Vec<Value>> {
    for frame in stream.replace("\r\n", "\n").split("\n\n") {
        let data: String = frame
            .lines()
            .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
            .collect();
        let Ok(event) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        match event["type"].as_str() {
            Some("response.completed" | "response.done")
                if event["response"]["status"] == "completed" =>
            {
                return Ok(event["response"]["output"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default());
            }
            Some("error" | "response.failed" | "response.incomplete") => break,
            _ => {}
        }
    }
    Err(Error::new(
        "server",
        "ChatGPT didn't finish this reply. Try again.",
    ))
}

fn token(fields: &[(&str, &str)]) -> Result<Session> {
    let form = fields
        .iter()
        .map(|(key, value)| format!("{key}={}", encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    let mut response = agent(30)
        .post(&format!("{AUTH}/oauth/token"))
        .content_type("application/x-www-form-urlencoded")
        .send(form)
        .map_err(network)?;
    if response.status().as_u16() != 200 {
        return Err(Error::new("unauthorized", "Sign in with ChatGPT again"));
    }
    let tokens: Value = response.body_mut().read_json().map_err(network)?;
    let access_token = tokens["access_token"].as_str().ok_or_else(invalid)?;
    Ok(Session {
        account_id: account_id(access_token).ok_or_else(invalid)?,
        access_token: access_token.to_string(),
        refresh_token: tokens["refresh_token"]
            .as_str()
            .ok_or_else(invalid)?
            .to_string(),
        expires_at: now() + tokens["expires_in"].as_u64().unwrap_or(3600),
    })
}

/// The ChatGPT account that routes requests, read from the access token's claims.
fn account_id(access_token: &str) -> Option<String> {
    let payload = URL_SAFE_NO_PAD
        .decode(access_token.split('.').nth(1)?.trim_end_matches('='))
        .ok()?;
    let claims: Value = serde_json::from_slice(&payload).ok()?;
    claims["https://api.openai.com/auth"]["chatgpt_account_id"]
        .as_str()
        .map(str::to_string)
}

fn post_json(url: &str, body: Value) -> Result<(u16, Value)> {
    let mut response = agent(30).post(url).send_json(body).map_err(network)?;
    let status = response.status().as_u16();
    Ok((
        status,
        response.body_mut().read_json().unwrap_or(Value::Null),
    ))
}

fn agent(timeout: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout)))
        .http_status_as_error(false)
        .build()
        .into()
}

fn network(error: ureq::Error) -> Error {
    Error::new("network", format!("Could not reach ChatGPT: {error}"))
}

fn invalid() -> Error {
    Error::new("invalid_response", "OpenAI returned an unexpected response")
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_completed_reply_counts() {
        let done = r#"{"type":"response.completed","response":{"status":"completed","output":[{"type":"message"}]}}"#;
        let stream = format!(
            "data: {{\"type\":\"response.output_text.delta\",\"delta\":\"Hi\"}}\r\n\r\ndata: {done}\r\n\r\ndata: [DONE]\n\n"
        );
        assert_eq!(
            completed_output(&stream).unwrap(),
            vec![json!({"type":"message"})]
        );
        assert!(
            completed_output(
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hi\"}\n\n"
            )
            .is_err()
        );
        assert!(completed_output("data: {\"type\":\"response.failed\"}\n\n").is_err());
    }

    #[test]
    fn the_account_comes_from_the_access_token() {
        let claims = URL_SAFE_NO_PAD
            .encode(r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"acct-1"}}"#);
        assert_eq!(
            account_id(&format!("header.{claims}.signature")).as_deref(),
            Some("acct-1")
        );
        assert_eq!(account_id("not-a-jwt"), None);
    }
}
