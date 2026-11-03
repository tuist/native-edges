//! The headless core of Native Edges: navigation and Tuist business logic,
//! with no UI and no platform frameworks.

mod bridge;
pub mod routes;
pub mod tuist;

pub use bridge::execute;
