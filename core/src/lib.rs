//! The headless core of Native Edges: navigation, sign-in with ChatGPT, and
//! conversations, with no UI and no platform frameworks.

mod bridge;
pub mod chatgpt;
pub mod conversations;
pub mod routes;
pub mod store;

pub use bridge::execute;

/// Every failure crosses the bridge as data: a stable kind and a message to show.
#[derive(Debug, serde::Serialize)]
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
