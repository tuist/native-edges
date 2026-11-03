//! The one bridge between the edges and the core: a JSON command in, a JSON
//! result out. Swift calls `edges_call`, Kotlin calls `Core.call` through JNI,
//! and the CLI calls `execute` directly.

use crate::routes::{self, Params};
use crate::tuist::{self, Error, PendingSignIn, Session};
use serde::Deserialize;
use serde_json::{Value, json};
use std::ffi::{CStr, CString, c_char};

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum Command {
    /// The navigation catalog.
    Routes,
    /// Resolves a deep link into a navigation stack.
    Open {
        url: String,
    },
    /// Builds the deep link for a route.
    Url {
        route: String,
        #[serde(default)]
        params: Params,
    },
    SignIn {
        redirect_uri: String,
    },
    CompleteSignIn {
        pending: PendingSignIn,
        callback_url: String,
    },
    Projects {
        session: Session,
    },
    Project {
        session: Session,
        account: String,
        project: String,
    },
}

/// Runs one command and returns `{"ok": ...}` or `{"error": {"kind", "message"}}`.
/// Commands that talk to Tuist block, so edges call this off the main thread.
pub fn execute(input: &str) -> String {
    let result = std::panic::catch_unwind(|| run(input))
        .unwrap_or_else(|_| Err(Error::new("internal", "The core failed unexpectedly")));
    match result {
        Ok(value) => json!({ "ok": value }),
        Err(error) => json!({ "error": error }),
    }
    .to_string()
}

fn run(input: &str) -> Result<Value, Error> {
    let command: Command = serde_json::from_str(input)
        .map_err(|error| Error::new("invalid_command", error.to_string()))?;
    match command {
        Command::Routes => Ok(json!({ "scheme": routes::SCHEME, "routes": routes::ROUTES })),
        Command::Open { url } => routes::open(&url)
            .map(|stack| json!({ "stack": stack }))
            .map_err(|_| Error::new("unknown_route", format!("No screen matches {url}"))),
        Command::Url { route, params } => routes::url(&route, &params)
            .map(|url| json!({ "url": url }))
            .map_err(|error| Error::new("invalid_route", format!("{error:?}"))),
        Command::SignIn { redirect_uri } => {
            let (url, pending) = tuist::sign_in(&redirect_uri)?;
            Ok(json!({ "authorize_url": url, "pending": pending }))
        }
        Command::CompleteSignIn {
            pending,
            callback_url,
        } => {
            let session = tuist::complete_sign_in(pending, &callback_url)?;
            Ok(json!({ "session": session }))
        }
        Command::Projects { session } => screen(tuist::projects(&session)?),
        Command::Project {
            session,
            account,
            project,
        } => screen(tuist::project(&session, &account, &project)?),
    }
}

fn screen((screen, session): (Value, Option<Session>)) -> Result<Value, Error> {
    Ok(json!({ "screen": screen, "session": session }))
}

/// # Safety
/// `input` must be a NUL-terminated UTF-8 string that stays alive for the call.
/// Release the result exactly once with `edges_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edges_call(input: *const c_char) -> *mut c_char {
    let output = if input.is_null() {
        json!({"error": Error::new("invalid_command", "No input")}).to_string()
    } else {
        // SAFETY: the caller guarantees a live C string.
        match unsafe { CStr::from_ptr(input) }.to_str() {
            Ok(input) => execute(input),
            Err(_) => {
                json!({"error": Error::new("invalid_command", "Input is not UTF-8")}).to_string()
            }
        }
    };
    CString::new(output)
        .expect("JSON never contains NUL bytes")
        .into_raw()
}

/// # Safety
/// Pass a pointer returned by `edges_call` exactly once, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edges_free(output: *mut c_char) {
    if !output.is_null() {
        // SAFETY: the pointer came from CString::into_raw in edges_call.
        drop(unsafe { CString::from_raw(output) });
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_tuist_edges_Core_call(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    input: jni::objects::JString,
) -> jni::sys::jstring {
    let output = match env.get_string(&input) {
        Ok(input) => execute(&String::from(input)),
        Err(_) => return std::ptr::null_mut(),
    };
    env.new_string(output)
        .map_or(std::ptr::null_mut(), |value| value.into_raw())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(input: &str) -> Value {
        let input = CString::new(input).unwrap();
        // SAFETY: the input is a live C string and the output is freed once.
        unsafe {
            let output = edges_call(input.as_ptr());
            let value = serde_json::from_str(CStr::from_ptr(output).to_str().unwrap()).unwrap();
            edges_free(output);
            value
        }
    }

    #[test]
    fn the_c_bridge_resolves_deep_links() {
        let result = call(r#"{"command":"open","url":"edges://projects/acme/app"}"#);
        let stack = result["ok"]["stack"].as_array().unwrap();
        assert_eq!(stack.len(), 2);
        assert_eq!(stack[1]["route"], "project");
        assert_eq!(stack[1]["params"]["project"], "app");
    }

    #[test]
    fn the_c_bridge_reports_errors_as_data() {
        assert_eq!(
            call(r#"{"command":"open","url":"edges://nope"}"#)["error"]["kind"],
            "unknown_route"
        );
        assert_eq!(
            call(r#"{"command":"teleport"}"#)["error"]["kind"],
            "invalid_command"
        );
        assert_eq!(call("not json")["error"]["kind"], "invalid_command");
    }

    #[test]
    fn the_catalog_lists_every_route() {
        let result = call(r#"{"command":"routes"}"#);
        let ids: Vec<_> = result["ok"]["routes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|route| route["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids, ["projects", "project"]);
    }
}
