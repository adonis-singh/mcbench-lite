//! The SQL of `server/store.js`, copied from it mechanically (by a script that
//! cut each template literal out of the file): the text SQLite keeps in
//! `sqlite_master` is the text of each `CREATE`, so a database this crate
//! creates matches one `server/store.js` created.

/// `SEARCH_TABLES`.
pub const SEARCH_TABLES: &str = r#"
  CREATE TABLE search_docs (
    id INTEGER PRIMARY KEY,
    owner TEXT NOT NULL,
    source TEXT NOT NULL UNIQUE,
    body TEXT NOT NULL
  );
  CREATE VIRTUAL TABLE search USING fts5(body, content='search_docs', content_rowid='id', tokenize='porter unicode61');
  CREATE TRIGGER search_docs_insert AFTER INSERT ON search_docs BEGIN
    INSERT INTO search(rowid, body) VALUES (new.id, new.body);
  END;
  CREATE TRIGGER search_docs_delete AFTER DELETE ON search_docs BEGIN
    INSERT INTO search(search, rowid, body) VALUES ('delete', old.id, old.body);
  END;
  CREATE TRIGGER search_docs_update AFTER UPDATE ON search_docs BEGIN
    INSERT INTO search(search, rowid, body) VALUES ('delete', old.id, old.body);
    INSERT INTO search(rowid, body) VALUES (new.id, new.body);
  END;
"#;

/// `BENCHES_TABLE`.
pub const BENCHES_TABLE: &str = r#"
  CREATE TABLE benches (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    created_at TEXT NOT NULL,
    rounds_json TEXT NOT NULL,
    archived_at INTEGER,
    head_round_id TEXT,
    blind INTEGER NOT NULL DEFAULT 0,
    revealed_at INTEGER,
    order_json TEXT NOT NULL DEFAULT '[]'
  );
"#;

/// `VOTES_TABLE`.
pub const VOTES_TABLE: &str = r#"
  CREATE TABLE bench_votes (
    round_id TEXT PRIMARY KEY,
    bench_id TEXT NOT NULL REFERENCES benches(id),
    pick TEXT,
    members_json TEXT NOT NULL,
    blind INTEGER NOT NULL,
    created_at INTEGER NOT NULL
  );
"#;

/// `LOOKUP_INDEXES`.
pub const LOOKUP_INDEXES: &str = r#"
  CREATE INDEX messages_parent ON messages(parent_id);
  CREATE INDEX conversations_bench ON conversations(bench_id, bench_slot);
  CREATE INDEX bench_votes_bench ON bench_votes(bench_id);
  CREATE INDEX search_docs_owner ON search_docs(owner);
"#;

/// `MESSAGE_PROMPTS_TABLE`.
pub const MESSAGE_PROMPTS_TABLE: &str = r#"
  CREATE TABLE message_prompts (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    prompt_id TEXT NOT NULL,
    title TEXT NOT NULL,
    UNIQUE (message_id, prompt_id)
  );
"#;

/// The `database.exec` of a fresh database in `initialize()`, its
/// `${...}` filled with the constants above.
pub fn fresh() -> String {
    let mut sql = String::new();
    sql.push_str(
        r#"
      "#,
    );
    sql.push_str(BENCHES_TABLE);
    sql.push_str(
        r#"
      "#,
    );
    sql.push_str(VOTES_TABLE);
    sql.push_str(
        r#"

      CREATE TABLE conversations (
        id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        provider TEXT NOT NULL,
        model TEXT NOT NULL,
        effort TEXT,
        resolution INTEGER NOT NULL DEFAULT 1,
        created_at TEXT NOT NULL,
        current_message_id TEXT,
        world_manifest_json TEXT NOT NULL,
        archived_at INTEGER,
        finished_at INTEGER,
        seen_at INTEGER,
        bench_id TEXT REFERENCES benches(id),
        bench_slot INTEGER
      );

      CREATE TABLE messages (
        id TEXT PRIMARY KEY,
        conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
        parent_id TEXT REFERENCES messages(id),
        position INTEGER NOT NULL CHECK (position >= 0),
        payload_json TEXT NOT NULL,
        checkpoint_manifest_json TEXT,
        world_manifest_json TEXT,
        UNIQUE (conversation_id, position)
      );

      CREATE INDEX conversations_active_created
        ON conversations(archived_at, created_at DESC);

      "#,
    );
    sql.push_str(SEARCH_TABLES);
    sql.push_str(
        r#"
      "#,
    );
    sql.push_str(LOOKUP_INDEXES);
    sql.push_str(
        r#"
      "#,
    );
    sql.push_str(MESSAGE_PROMPTS_TABLE);
    sql.push_str(
        r#"
    "#,
    );
    sql
}

/// `LINK_PROMPT`.
pub const LINK_PROMPT: &str =
    r#"INSERT OR IGNORE INTO message_prompts (message_id, prompt_id, title) VALUES (?, ?, ?)"#;
