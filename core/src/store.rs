//! The core's local database: SQLite through Diesel. Each edge calls `launch`
//! at startup with a directory it may write to; the core opens the database
//! there and applies any migrations the installed version hasn't run yet.

use crate::{Error, Result};
use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// The SQL files under `core/migrations`, compiled into the core.
const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

pub mod schema {
    diesel::table! {
        conversations (id) {
            id -> Text,
            title -> Text,
            updated_at -> BigInt,
        }
    }

    diesel::table! {
        messages (id) {
            id -> Integer,
            conversation_id -> Text,
            role -> Text,
            text -> Text,
        }
    }

    diesel::table! {
        items (id) {
            id -> Integer,
            conversation_id -> Text,
            payload -> Text,
        }
    }

    diesel::joinable!(messages -> conversations (conversation_id));
    diesel::joinable!(items -> conversations (conversation_id));
    diesel::allow_tables_to_appear_in_same_query!(conversations, messages, items);
}

pub struct Store(Mutex<SqliteConnection>);

static STORE: OnceLock<Store> = OnceLock::new();

impl Store {
    /// Opens the database at `path` (or `:memory:`) and runs pending migrations.
    pub fn open(path: &str) -> Result<(Store, Vec<String>)> {
        let mut connection =
            SqliteConnection::establish(path).map_err(|e| storage(e.to_string()))?;
        diesel::sql_query("PRAGMA foreign_keys = ON")
            .execute(&mut connection)
            .map_err(|e| storage(e.to_string()))?;
        let applied = connection
            .run_pending_migrations(MIGRATIONS)
            .map_err(|e| storage(e.to_string()))?
            .iter()
            .map(ToString::to_string)
            .collect();
        Ok((Store(Mutex::new(connection)), applied))
    }

    pub fn connection(&self) -> MutexGuard<'_, SqliteConnection> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Opens the app's database once per launch. Returns the migrations it applied.
pub fn launch(data_dir: &str) -> Result<Vec<String>> {
    if STORE.get().is_some() {
        return Ok(vec![]);
    }
    std::fs::create_dir_all(data_dir).map_err(|e| storage(e.to_string()))?;
    let (store, applied) = Store::open(&format!("{data_dir}/edges.sqlite3"))?;
    let _ = STORE.set(store);
    Ok(applied)
}

/// The database opened by `launch`.
pub fn current() -> Result<&'static Store> {
    STORE
        .get()
        .ok_or_else(|| Error::new("not_launched", "Call `launch` before using the database"))
}

pub fn storage(message: String) -> Error {
    Error::new(
        "storage",
        format!("Could not use the local database: {message}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_run_once_and_create_the_tables() {
        let dir = std::env::temp_dir().join(format!("edges-store-{}", std::process::id()));
        let path = dir.join("edges.sqlite3");
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(&path);
        let (_, first) = Store::open(path.to_str().unwrap()).unwrap();
        assert_eq!(first.len(), 1);
        let (store, second) = Store::open(path.to_str().unwrap()).unwrap();
        assert!(second.is_empty());
        let count: i64 = schema::conversations::table
            .count()
            .get_result(&mut *store.connection())
            .unwrap();
        assert_eq!(count, 0);
    }
}
