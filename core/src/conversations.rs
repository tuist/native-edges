//! Conversations with ChatGPT, stored in the core's SQLite database. The agent
//! loop runs here: ChatGPT can call tools that read the conversations on this device.

use crate::chatgpt::{self, Session};
use crate::routes::{self, Params};
use crate::store::{Store, schema, storage};
use crate::{Error, Result};
use diesel::prelude::*;
use serde::Serialize;
use serde_json::{Value, json};

const INSTRUCTIONS: &str = "You are the assistant in Native Edges, a small sample app. \
Keep answers short and friendly. Conversations are stored on this device; use the tools \
to look at earlier ones when the person refers to them.";
const MAX_STEPS: usize = 6;

#[derive(Queryable, Selectable, Insertable, AsChangeset)]
#[diesel(table_name = schema::conversations)]
struct Conversation {
    id: String,
    title: String,
    updated_at: i64,
}

#[derive(Queryable, Selectable, Insertable, Serialize)]
#[diesel(table_name = schema::messages)]
struct Message {
    #[serde(skip)]
    conversation_id: String,
    role: String,
    text: String,
}

#[derive(Insertable)]
#[diesel(table_name = schema::items)]
struct Item {
    conversation_id: String,
    payload: String,
}

/// The conversations screen.
pub fn list(store: &Store) -> Result<Value> {
    let connection = &mut *store.connection();
    let conversations: Vec<Conversation> = schema::conversations::table
        .order(schema::conversations::updated_at.desc())
        .select(Conversation::as_select())
        .load(connection)
        .map_err(db)?;
    let mut rows = vec![];
    for conversation in conversations {
        let reply: Option<String> = schema::messages::table
            .filter(schema::messages::conversation_id.eq(&conversation.id))
            .filter(schema::messages::role.eq("assistant"))
            .order(schema::messages::id.desc())
            .select(schema::messages::text)
            .first(connection)
            .optional()
            .map_err(db)?;
        rows.push(json!({
            "id": conversation.id,
            "title": conversation.title,
            "subtitle": reply.unwrap_or_default().chars().take(80).collect::<String>(),
            "url": link(&conversation.id),
        }));
    }
    Ok(json!({ "title": "Conversations", "rows": rows }))
}

/// One conversation's screen.
pub fn show(store: &Store, id: &str) -> Result<Value> {
    let connection = &mut *store.connection();
    let conversation = find(connection, id)?.ok_or_else(not_found)?;
    Ok(screen(&conversation, &messages(connection, id)?))
}

/// Sends a message, lets ChatGPT call tools until it answers, and saves the
/// exchange in one transaction. Without an `id`, starts a new conversation.
pub fn send(
    session: &Session,
    store: &Store,
    id: Option<&str>,
    text: &str,
) -> Result<(Value, Option<Session>)> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::new("invalid", "Write a message first"));
    }
    // Read what's saved, then let go of the database while ChatGPT thinks.
    let (mut conversation, mut input) = {
        let connection = &mut *store.connection();
        match id {
            Some(id) => {
                let conversation = find(connection, id)?.ok_or_else(not_found)?;
                let input: Vec<String> = schema::items::table
                    .filter(schema::items::conversation_id.eq(id))
                    .order(schema::items::id)
                    .select(schema::items::payload)
                    .load(connection)
                    .map_err(db)?;
                let input = input
                    .iter()
                    .filter_map(|item| serde_json::from_str(item).ok())
                    .collect();
                (conversation, input)
            }
            None => (
                Conversation {
                    id: new_id()?,
                    title: text.chars().take(40).collect(),
                    updated_at: 0,
                },
                vec![],
            ),
        }
    };
    let saved_items = input.len();
    let mut added = vec![message(&conversation.id, "user", text)];
    input.push(json!({ "role": "user", "content": text }));

    let mut session = session.clone();
    let mut renewed = None;
    let mut answered = false;
    for _ in 0..MAX_STEPS {
        let (output, refreshed) = chatgpt::respond(
            &session,
            json!({
                "instructions": INSTRUCTIONS,
                "input": input,
                "tools": tools(),
                "tool_choice": "auto",
                "parallel_tool_calls": false,
                "reasoning": { "effort": "low" },
            }),
        )?;
        if let Some(refreshed) = refreshed {
            session = refreshed.clone();
            renewed = Some(refreshed);
        }
        let mut called = false;
        for mut item in output {
            // Items aren't stored server-side, so their IDs mean nothing on replay.
            if let Some(item) = item.as_object_mut() {
                item.remove("id");
            }
            match item["type"].as_str() {
                Some("function_call") => {
                    called = true;
                    let name = item["name"].as_str().unwrap_or_default().to_string();
                    let result = run_tool(
                        store,
                        &conversation.id,
                        &name,
                        item["arguments"].as_str().unwrap_or("{}"),
                    );
                    let output = json!({ "type": "function_call_output", "call_id": item["call_id"], "output": result.to_string() });
                    added.push(message(&conversation.id, "tool", &name));
                    input.extend([item, output]);
                }
                Some("message") => {
                    let reply: String = item["content"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|part| part["text"].as_str())
                        .collect();
                    if !reply.is_empty() {
                        added.push(message(&conversation.id, "assistant", &reply));
                    }
                    input.push(item);
                }
                _ => input.push(item),
            }
        }
        if !called {
            answered = true;
            break;
        }
    }
    if !answered {
        return Err(Error::new(
            "server",
            "ChatGPT kept calling tools without answering",
        ));
    }

    conversation.updated_at = chatgpt::now() as i64;
    let items: Vec<Item> = input[saved_items..]
        .iter()
        .map(|item| Item {
            conversation_id: conversation.id.clone(),
            payload: item.to_string(),
        })
        .collect();
    let connection = &mut *store.connection();
    connection
        .transaction::<_, diesel::result::Error, _>(|connection| {
            diesel::insert_into(schema::conversations::table)
                .values(&conversation)
                .on_conflict(schema::conversations::id)
                .do_update()
                .set(schema::conversations::updated_at.eq(conversation.updated_at))
                .execute(connection)?;
            diesel::insert_into(schema::messages::table)
                .values(&added)
                .execute(connection)?;
            diesel::insert_into(schema::items::table)
                .values(&items)
                .execute(connection)?;
            Ok(())
        })
        .map_err(db)?;
    Ok((
        screen(&conversation, &messages(connection, &conversation.id)?),
        renewed,
    ))
}

fn tools() -> Value {
    json!([
        {
            "type": "function",
            "name": "list_conversations",
            "description": "List the person's earlier conversations stored on this device, newest first.",
            "parameters": { "type": "object", "properties": {}, "additionalProperties": false },
        },
        {
            "type": "function",
            "name": "read_conversation",
            "description": "Read the messages of one earlier conversation.",
            "parameters": {
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"],
                "additionalProperties": false,
            },
        },
    ])
}

fn run_tool(store: &Store, current: &str, name: &str, arguments: &str) -> Value {
    let connection = &mut *store.connection();
    let result = match name {
        "list_conversations" => schema::conversations::table
            .filter(schema::conversations::id.ne(current))
            .order(schema::conversations::updated_at.desc())
            .select(Conversation::as_select())
            .load(connection)
            .map(|all| {
                json!(
                    all.iter()
                        .map(|c| json!({ "id": c.id, "title": c.title }))
                        .collect::<Vec<_>>()
                )
            })
            .map_err(db),
        "read_conversation" => {
            let id = serde_json::from_str::<Value>(arguments)
                .ok()
                .and_then(|a| a["id"].as_str().map(str::to_string));
            messages(connection, id.as_deref().unwrap_or_default()).map(|all| {
                json!(
                    all.iter()
                        .filter(|m| m.role != "tool")
                        .map(|m| json!({ "role": m.role, "text": m.text }))
                        .collect::<Vec<_>>()
                )
            })
        }
        _ => Err(Error::new("unknown_tool", format!("Unknown tool {name}"))),
    };
    result.unwrap_or_else(|error| json!({ "error": error.message }))
}

fn find(connection: &mut SqliteConnection, id: &str) -> Result<Option<Conversation>> {
    schema::conversations::table
        .find(id)
        .select(Conversation::as_select())
        .first(connection)
        .optional()
        .map_err(db)
}

fn messages(connection: &mut SqliteConnection, id: &str) -> Result<Vec<Message>> {
    schema::messages::table
        .filter(schema::messages::conversation_id.eq(id))
        .order(schema::messages::id)
        .select(Message::as_select())
        .load(connection)
        .map_err(db)
}

fn message(conversation_id: &str, role: &str, text: &str) -> Message {
    Message {
        conversation_id: conversation_id.into(),
        role: role.into(),
        text: text.into(),
    }
}

fn screen(conversation: &Conversation, messages: &[Message]) -> Value {
    json!({
        "id": conversation.id,
        "title": conversation.title,
        "messages": messages,
        "url": link(&conversation.id),
    })
}

fn link(id: &str) -> String {
    routes::url(
        "conversation",
        &Params::from([("id".to_string(), id.to_string())]),
    )
    .unwrap_or_default()
}

fn new_id() -> Result<String> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|_| Error::new("internal", "No randomness available"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn not_found() -> Error {
    Error::new("not_found", "That conversation doesn't exist")
}

fn db(error: diesel::result::Error) -> Error {
    storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_conversations_list_newest_first_and_feed_the_tools() {
        let (store, _) = Store::open(":memory:").unwrap();
        for (id, updated_at) in [("aa", 1), ("bb", 2)] {
            let connection = &mut *store.connection();
            diesel::insert_into(schema::conversations::table)
                .values(Conversation {
                    id: id.into(),
                    title: format!("Chat {id}"),
                    updated_at,
                })
                .execute(connection)
                .unwrap();
            diesel::insert_into(schema::messages::table)
                .values([
                    message(id, "user", "Hi"),
                    message(id, "tool", "list_conversations"),
                    message(id, "assistant", "Hello"),
                ])
                .execute(connection)
                .unwrap();
        }
        let rows = list(&store).unwrap()["rows"].clone();
        assert_eq!(rows[0]["id"], "bb");
        assert_eq!(rows[1]["url"], "edges://conversations/aa");
        assert_eq!(rows[1]["subtitle"], "Hello");
        assert_eq!(
            show(&store, "aa").unwrap()["messages"][2],
            json!({ "role": "assistant", "text": "Hello" })
        );
        assert_eq!(
            run_tool(&store, "bb", "list_conversations", "{}"),
            json!([{ "id": "aa", "title": "Chat aa" }])
        );
        assert_eq!(
            run_tool(&store, "bb", "read_conversation", r#"{"id":"aa"}"#),
            json!([{ "role": "user", "text": "Hi" }, { "role": "assistant", "text": "Hello" }])
        );
        assert_eq!(show(&store, "missing").unwrap_err().kind, "not_found");
    }
}
