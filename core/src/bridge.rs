//! The one bridge between the edges and the core. UniFFI turns these
//! annotations into Swift and Kotlin bindings, so the apps call typed
//! functions and get records back. The CLI calls the same functions in Rust.

use crate::chatgpt::{self, DeviceCode, Session};
use crate::conversations::{self, ConversationList, ConversationScreen};
use crate::routes::{self, Params, Screen};
use crate::store;
use std::sync::{Arc, OnceLock};

/// A platform service the edge provides: the Keychain on iOS, the Keystore on
/// Android, a private file in the terminal. The core decides what to keep;
/// the edge decides how to keep it safe.
#[uniffi::export(with_foreign)]
pub trait SecureStorage: Send + Sync {
    fn read(&self, key: String) -> Option<String>;
    fn write(&self, key: String, value: String);
    fn delete(&self, key: String);
}

/// Every failure crosses the bridge as a typed error. Swift and Kotlin see the
/// variant and its message; Rust callers can also match on `kind`.
#[derive(Debug, uniffi::Error)]
#[uniffi(flat_error)]
pub enum CoreError {
    /// The session is missing or expired: show the sign-in screen.
    Unauthorized { message: String },
    Failed { kind: String, message: String },
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized { message } | Self::Failed { message, .. } => f.write_str(message),
        }
    }
}

impl std::error::Error for CoreError {}

impl From<crate::Error> for CoreError {
    fn from(error: crate::Error) -> Self {
        if error.kind == "unauthorized" {
            Self::Unauthorized {
                message: error.message,
            }
        } else {
            Self::Failed {
                kind: error.kind.into(),
                message: error.message,
            }
        }
    }
}

type Result<T> = std::result::Result<T, CoreError>;

/// A screen in the navigation catalog.
#[derive(uniffi::Record)]
pub struct Route {
    pub id: String,
    pub title: String,
    pub description: String,
    /// For example `edges://conversations/{id}`.
    pub pattern: String,
}

const SESSION: &str = "chatgpt_session";
static STORAGE: OnceLock<Arc<dyn SecureStorage>> = OnceLock::new();

/// Called once at launch: opens the database in a directory the edge may
/// write to, runs pending migrations, and keeps the edge's secure storage.
/// Returns the migrations it applied.
#[uniffi::export]
pub fn launch(data_dir: String, storage: Arc<dyn SecureStorage>) -> Result<Vec<String>> {
    let _ = STORAGE.set(storage);
    Ok(store::launch(&data_dir)?)
}

/// The navigation catalog.
#[uniffi::export]
pub fn routes() -> Vec<Route> {
    routes::ROUTES
        .iter()
        .map(|route| Route {
            id: route.id.into(),
            title: route.title.into(),
            description: route.description.into(),
            pattern: format!("{}://{}", routes::SCHEME, route.path),
        })
        .collect()
}

/// Resolves a deep link into the stack of screens to show, root first.
#[uniffi::export]
pub fn open(url: String) -> Result<Vec<Screen>> {
    routes::open(&url).map_err(|_| failed("unknown_route", format!("No screen matches {url}")))
}

/// Builds the deep link for a route.
#[uniffi::export]
pub fn url(route: String, params: Params) -> Result<String> {
    routes::url(&route, &params).map_err(|error| failed("invalid_route", format!("{error:?}")))
}

#[uniffi::export]
pub fn is_signed_in() -> bool {
    storage().is_ok_and(|storage| storage.read(SESSION.into()).is_some())
}

/// Starts signing in with ChatGPT: show `user_code` and poll until it's entered.
#[uniffi::export]
pub fn sign_in() -> Result<DeviceCode> {
    Ok(chatgpt::sign_in()?)
}

/// Returns true once the person entered the code, after saving the session.
#[uniffi::export]
pub fn poll_sign_in(code: DeviceCode) -> Result<bool> {
    match chatgpt::poll_sign_in(&code)? {
        Some(session) => save(&session).map(|()| true),
        None => Ok(false),
    }
}

#[uniffi::export]
pub fn sign_out() {
    if let Ok(storage) = storage() {
        storage.delete(SESSION.into());
    }
}

/// The conversations screen.
#[uniffi::export]
pub fn conversations() -> Result<ConversationList> {
    Ok(conversations::list(store::current()?)?)
}

/// One conversation's screen.
#[uniffi::export]
pub fn conversation(id: String) -> Result<ConversationScreen> {
    Ok(conversations::show(store::current()?, &id)?)
}

/// Sends a message and waits for ChatGPT to answer, calling tools along the
/// way. Without an `id`, starts a new conversation. Blocks, so edges call it
/// off the main thread.
#[uniffi::export]
pub fn send(id: Option<String>, text: String) -> Result<ConversationScreen> {
    let session = session()?;
    match conversations::send(&session, store::current()?, id.as_deref(), &text) {
        Ok((screen, renewed)) => {
            if let Some(renewed) = renewed {
                save(&renewed)?;
            }
            Ok(screen)
        }
        Err(error) if error.kind == "unauthorized" => {
            sign_out();
            Err(error.into())
        }
        Err(error) => Err(error.into()),
    }
}

fn session() -> Result<Session> {
    let saved = storage()?.read(SESSION.into()).ok_or_else(|| CoreError::Unauthorized {
        message: "Sign in with ChatGPT first".into(),
    })?;
    serde_json::from_str(&saved).map_err(|_| CoreError::Unauthorized {
        message: "The saved session is unreadable. Sign in again".into(),
    })
}

fn save(session: &Session) -> Result<()> {
    let json = serde_json::to_string(session).map_err(|e| failed("internal", e.to_string()))?;
    storage()?.write(SESSION.into(), json);
    Ok(())
}

fn storage() -> Result<&'static Arc<dyn SecureStorage>> {
    STORAGE
        .get()
        .ok_or_else(|| failed("not_launched", "Call `launch` first".into()))
}

fn failed(kind: &str, message: String) -> CoreError {
    CoreError::Failed {
        kind: kind.into(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Memory(Mutex<HashMap<String, String>>);

    impl SecureStorage for Memory {
        fn read(&self, key: String) -> Option<String> {
            self.0.lock().unwrap().get(&key).cloned()
        }
        fn write(&self, key: String, value: String) {
            self.0.lock().unwrap().insert(key, value);
        }
        fn delete(&self, key: String) {
            self.0.lock().unwrap().remove(&key);
        }
    }

    #[test]
    fn the_session_lives_in_the_edges_storage() {
        let dir = std::env::temp_dir().join(format!("edges-bridge-{}", std::process::id()));
        launch(dir.to_string_lossy().into(), Arc::new(Memory::default())).unwrap();
        assert!(!is_signed_in());
        assert!(matches!(
            send(None, "Hi".into()),
            Err(CoreError::Unauthorized { .. })
        ));
        save(&Session {
            access_token: "a".into(),
            refresh_token: "r".into(),
            account_id: "acc".into(),
            expires_at: 0,
        })
        .unwrap();
        assert!(is_signed_in());
        sign_out();
        assert!(!is_signed_in());
    }

    #[test]
    fn deep_links_and_the_catalog_cross_the_bridge() {
        let stack = open("edges://conversations/ab12".into()).unwrap();
        assert_eq!(stack[1].route, "conversation");
        assert_eq!(stack[1].params["id"], "ab12");
        assert!(matches!(
            open("edges://nope".into()),
            Err(CoreError::Failed { kind, .. }) if kind == "unknown_route"
        ));
        let ids: Vec<_> = routes().into_iter().map(|route| route.id).collect();
        assert_eq!(ids, ["conversations", "new", "conversation"]);
    }
}
