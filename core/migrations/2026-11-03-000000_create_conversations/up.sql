CREATE TABLE conversations (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    updated_at BIGINT NOT NULL
);

-- What the person sees: their messages, the replies, and tool activity.
CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    text TEXT NOT NULL
);

-- What ChatGPT sees: every input item, including tool calls and their results.
CREATE TABLE items (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    payload TEXT NOT NULL
);

CREATE INDEX messages_by_conversation ON messages (conversation_id, id);
CREATE INDEX items_by_conversation ON items (conversation_id, id);
