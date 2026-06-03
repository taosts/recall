use rusqlite::{Connection, Result, params};
use std::path::Path;

/// Initialize the Recall SQLite database.
/// Creates the file if it doesn't exist and runs all DDL migrations.
/// Returns an open connection ready for use.
pub fn init_db(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path)?;

    // Enable WAL mode for better concurrent read performance
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;

    conn.execute_batch(r#"
        -- ─────────────────────────────────────────────────────────────────
        -- Core table: information traces
        -- ─────────────────────────────────────────────────────────────────
        CREATE TABLE IF NOT EXISTS artifacts (
            id            TEXT PRIMARY KEY,
            type          TEXT NOT NULL DEFAULT 'history',
            title         TEXT,
            url           TEXT,
            domain        TEXT,
            created_at    TEXT NOT NULL,
            visited_at    TEXT,
            is_bookmarked INTEGER NOT NULL DEFAULT 0,
            visit_count   INTEGER NOT NULL DEFAULT 0,
            source        TEXT,
            content       TEXT,
            user_note     TEXT,
            folder_path   TEXT,
            import_batch  TEXT
        );

        -- ─────────────────────────────────────────────────────────────────
        -- FTS5 full-text index
        -- tokenize='unicode61' handles Unicode text; good enough for MVP.
        -- Future: replace with trigram or a custom tokenizer for CJK.
        -- ─────────────────────────────────────────────────────────────────
        CREATE VIRTUAL TABLE IF NOT EXISTS artifacts_fts USING fts5(
            title,
            url,
            domain,
            content,
            user_note,
            folder_path,
            content='artifacts',
            content_rowid='rowid',
            tokenize='unicode61'
        );

        -- ─────────────────────────────────────────────────────────────────
        -- Triggers to keep FTS index in sync with the main table
        -- ─────────────────────────────────────────────────────────────────
        CREATE TRIGGER IF NOT EXISTS artifacts_ai
        AFTER INSERT ON artifacts BEGIN
            INSERT INTO artifacts_fts(rowid, title, url, domain, content, user_note, folder_path)
            VALUES (new.rowid, new.title, new.url, new.domain,
                    new.content, new.user_note, new.folder_path);
        END;

        CREATE TRIGGER IF NOT EXISTS artifacts_ad
        AFTER DELETE ON artifacts BEGIN
            INSERT INTO artifacts_fts(artifacts_fts, rowid, title, url, domain, content, user_note, folder_path)
            VALUES ('delete', old.rowid, old.title, old.url, old.domain,
                    old.content, old.user_note, old.folder_path);
        END;

        CREATE TRIGGER IF NOT EXISTS artifacts_au
        AFTER UPDATE ON artifacts BEGIN
            INSERT INTO artifacts_fts(artifacts_fts, rowid, title, url, domain, content, user_note, folder_path)
            VALUES ('delete', old.rowid, old.title, old.url, old.domain,
                    old.content, old.user_note, old.folder_path);
            INSERT INTO artifacts_fts(rowid, title, url, domain, content, user_note, folder_path)
            VALUES (new.rowid, new.title, new.url, new.domain,
                    new.content, new.user_note, new.folder_path);
        END;

        -- ─────────────────────────────────────────────────────────────────
        -- Indexes for time-range and filter queries
        -- ─────────────────────────────────────────────────────────────────
        CREATE INDEX IF NOT EXISTS idx_artifacts_visited_at ON artifacts(visited_at);
        CREATE INDEX IF NOT EXISTS idx_artifacts_domain     ON artifacts(domain);
        CREATE INDEX IF NOT EXISTS idx_artifacts_type       ON artifacts(type);
        CREATE INDEX IF NOT EXISTS idx_artifacts_source     ON artifacts(source);
        CREATE INDEX IF NOT EXISTS idx_artifacts_url        ON artifacts(url);

        -- ─────────────────────────────────────────────────────────────────
        -- Phase 2: Quest (探索任务) tables
        -- ─────────────────────────────────────────────────────────────────
        CREATE TABLE IF NOT EXISTS quests (
            id          TEXT PRIMARY KEY,
            name        TEXT,
            auto_name   TEXT,
            started_at  TEXT,
            ended_at    TEXT,
            status      TEXT NOT NULL DEFAULT 'auto',
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS quest_artifacts (
            quest_id    TEXT NOT NULL REFERENCES quests(id) ON DELETE CASCADE,
            artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
            added_at    TEXT NOT NULL,
            is_anchor   INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (quest_id, artifact_id)
        );

        CREATE INDEX IF NOT EXISTS idx_quest_artifacts_artifact
            ON quest_artifacts(artifact_id);
        CREATE INDEX IF NOT EXISTS idx_quests_started_at
            ON quests(started_at);
        CREATE INDEX IF NOT EXISTS idx_quests_status
            ON quests(status);
    "#)?;

    Ok(conn)
}

/// Upsert a single artifact URL — update visit_count / visited_at if URL already exists.
/// Returns true if inserted, false if updated.
pub fn upsert_artifact(conn: &Connection, a: &crate::models::Artifact) -> Result<bool> {
    let inserted = conn.execute(
        r#"INSERT INTO artifacts
               (id, type, title, url, domain, created_at, visited_at,
                is_bookmarked, visit_count, source, content, user_note,
                folder_path, import_batch)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
           ON CONFLICT(id) DO UPDATE SET
               visit_count   = visit_count + excluded.visit_count,
               visited_at    = CASE
                                 WHEN excluded.visited_at > visited_at
                                 THEN excluded.visited_at
                                 ELSE visited_at
                               END,
               is_bookmarked = MAX(is_bookmarked, excluded.is_bookmarked),
               title         = COALESCE(excluded.title, title),
               user_note     = COALESCE(user_note, excluded.user_note)"#,
        params![
            a.id,
            a.r#type,
            a.title,
            a.url,
            a.domain,
            a.created_at,
            a.visited_at,
            a.is_bookmarked as i64,
            a.visit_count,
            a.source,
            a.content,
            a.user_note,
            a.folder_path,
            a.import_batch,
        ],
    )?;
    Ok(inserted == 1)
}

/// Insert-or-ignore by URL (for history deduplication).
/// Returns the existing artifact id if a record with the same URL already exists.
pub fn find_by_url(conn: &Connection, url: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT id FROM artifacts WHERE url = ?1 LIMIT 1")?;
    let mut rows = stmt.query(params![url])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}
