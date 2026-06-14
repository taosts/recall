use rusqlite::{params, Connection, Result};
use std::path::Path;

/// Initialize the Recall SQLite database.
/// Creates the file if it doesn't exist and runs all DDL migrations.
/// Returns an open connection ready for use.
pub fn init_db(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path)?;

    // Enable WAL mode for better concurrent read performance.
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS artifacts (
            id              TEXT PRIMARY KEY,
            type            TEXT NOT NULL DEFAULT 'history',
            title           TEXT,
            url             TEXT,
            domain          TEXT,
            created_at      TEXT NOT NULL,
            visited_at      TEXT,
            is_bookmarked   INTEGER NOT NULL DEFAULT 0,
            visit_count     INTEGER NOT NULL DEFAULT 0,
            source          TEXT,
            content         TEXT,
            user_note       TEXT,
            folder_path     TEXT,
            import_batch    TEXT,
            page_category   TEXT DEFAULT 'content',
            noise_score     REAL NOT NULL DEFAULT 0.0,
            extracted_query TEXT,
            canonical_url   TEXT,
            referrer_domain TEXT,
            search_text     TEXT,
            embedding_version INTEGER NOT NULL DEFAULT 0
        );
    "#,
    )?;

    ensure_artifact_phase3_columns(&conn)?;
    ensure_embedding_schema(&conn)?;
    ensure_fts_schema(&conn)?;

    conn.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_artifacts_visited_at
            ON artifacts(visited_at);
        CREATE INDEX IF NOT EXISTS idx_artifacts_domain
            ON artifacts(domain);
        CREATE INDEX IF NOT EXISTS idx_artifacts_type
            ON artifacts(type);
        CREATE INDEX IF NOT EXISTS idx_artifacts_source
            ON artifacts(source);
        CREATE INDEX IF NOT EXISTS idx_artifacts_url
            ON artifacts(url);
        CREATE INDEX IF NOT EXISTS idx_artifacts_canonical_url
            ON artifacts(canonical_url);
        CREATE INDEX IF NOT EXISTS idx_artifacts_page_category
            ON artifacts(page_category);
        CREATE INDEX IF NOT EXISTS idx_artifacts_noise_score
            ON artifacts(noise_score);
        CREATE INDEX IF NOT EXISTS idx_artifacts_embedding_version
            ON artifacts(embedding_version);

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

        CREATE TABLE IF NOT EXISTS concept_synonyms (
            term       TEXT NOT NULL,
            synonym    TEXT NOT NULL,
            weight     REAL NOT NULL DEFAULT 1.0,
            source     TEXT NOT NULL DEFAULT 'manual',
            PRIMARY KEY (term, synonym)
        );

        CREATE TABLE IF NOT EXISTS search_log (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            query      TEXT NOT NULL,
            result_ids TEXT,
            created_at TEXT NOT NULL
        );
    "#,
    )?;

    Ok(conn)
}

fn ensure_artifact_phase3_columns(conn: &Connection) -> Result<()> {
    ensure_column(conn, "artifacts", "page_category", "TEXT DEFAULT 'content'")?;
    ensure_column(
        conn,
        "artifacts",
        "noise_score",
        "REAL NOT NULL DEFAULT 0.0",
    )?;
    ensure_column(conn, "artifacts", "extracted_query", "TEXT")?;
    ensure_column(conn, "artifacts", "canonical_url", "TEXT")?;
    ensure_column(conn, "artifacts", "referrer_domain", "TEXT")?;
    ensure_column(conn, "artifacts", "search_text", "TEXT")?;
    Ok(())
}

fn ensure_embedding_schema(conn: &Connection) -> Result<()> {
    ensure_column(
        conn,
        "artifacts",
        "embedding_version",
        "INTEGER NOT NULL DEFAULT 0",
    )?;

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS artifact_embeddings (
            artifact_id TEXT PRIMARY KEY REFERENCES artifacts(id) ON DELETE CASCADE,
            model       TEXT NOT NULL,
            dims        INTEGER NOT NULL,
            embedding   BLOB NOT NULL,
            updated_at  TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_artifact_embeddings_model
            ON artifact_embeddings(model);
    "#,
    )?;

    Ok(())
}

fn ensure_fts_schema(conn: &Connection) -> Result<()> {
    let fts_ready = table_has_column(conn, "artifacts_fts", "search_text")?;

    conn.execute_batch(
        r#"
        DROP TRIGGER IF EXISTS artifacts_ai;
        DROP TRIGGER IF EXISTS artifacts_ad;
        DROP TRIGGER IF EXISTS artifacts_au;
    "#,
    )?;

    if !fts_ready {
        conn.execute_batch("DROP TABLE IF EXISTS artifacts_fts;")?;
    }

    conn.execute_batch(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS artifacts_fts USING fts5(
            title,
            url,
            domain,
            content,
            user_note,
            folder_path,
            extracted_query,
            search_text,
            content='artifacts',
            content_rowid='rowid',
            tokenize='unicode61'
        );

        CREATE TRIGGER artifacts_ai
        AFTER INSERT ON artifacts BEGIN
            INSERT INTO artifacts_fts(rowid, title, url, domain, content, user_note, folder_path, extracted_query, search_text)
            VALUES (new.rowid, new.title, new.url, new.domain,
                    new.content, new.user_note, new.folder_path, new.extracted_query, new.search_text);
        END;

        CREATE TRIGGER artifacts_ad
        AFTER DELETE ON artifacts BEGIN
            INSERT INTO artifacts_fts(artifacts_fts, rowid, title, url, domain, content, user_note, folder_path, extracted_query, search_text)
            VALUES ('delete', old.rowid, old.title, old.url, old.domain,
                    old.content, old.user_note, old.folder_path, old.extracted_query, old.search_text);
        END;

        CREATE TRIGGER artifacts_au
        AFTER UPDATE ON artifacts BEGIN
            INSERT INTO artifacts_fts(artifacts_fts, rowid, title, url, domain, content, user_note, folder_path, extracted_query, search_text)
            VALUES ('delete', old.rowid, old.title, old.url, old.domain,
                    old.content, old.user_note, old.folder_path, old.extracted_query, old.search_text);
            INSERT INTO artifacts_fts(rowid, title, url, domain, content, user_note, folder_path, extracted_query, search_text)
            VALUES (new.rowid, new.title, new.url, new.domain,
                    new.content, new.user_note, new.folder_path, new.extracted_query, new.search_text);
        END;

        INSERT INTO artifacts_fts(artifacts_fts) VALUES('rebuild');
    "#,
    )?;

    Ok(())
}

fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<()> {
    if !table_has_column(conn, table, column)? {
        conn.execute(
            &format!("ALTER TABLE {} ADD COLUMN {} {}", table, column, definition),
            [],
        )?;
    }
    Ok(())
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Upsert a single artifact URL. Returns true if inserted, false if updated.
pub fn upsert_artifact(conn: &Connection, a: &crate::models::Artifact) -> Result<bool> {
    let inserted = conn.execute(
        r#"INSERT INTO artifacts
               (id, type, title, url, domain, created_at, visited_at,
                is_bookmarked, visit_count, source, content, user_note,
                folder_path, import_batch, page_category, noise_score,
                extracted_query, canonical_url, referrer_domain)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)
           ON CONFLICT(id) DO UPDATE SET
               visit_count   = visit_count + excluded.visit_count,
               visited_at    = CASE
                                 WHEN excluded.visited_at > visited_at
                                 THEN excluded.visited_at
                                 ELSE visited_at
                               END,
               is_bookmarked = MAX(is_bookmarked, excluded.is_bookmarked),
               title         = COALESCE(excluded.title, title),
               user_note     = COALESCE(user_note, excluded.user_note),
               page_category = COALESCE(excluded.page_category, page_category),
               noise_score   = excluded.noise_score,
               extracted_query = COALESCE(excluded.extracted_query, extracted_query),
               canonical_url = COALESCE(excluded.canonical_url, canonical_url),
               referrer_domain = COALESCE(excluded.referrer_domain, referrer_domain),
               embedding_version = 0"#,
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
            a.page_category,
            a.noise_score,
            a.extracted_query,
            a.canonical_url,
            a.referrer_domain,
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

/// Delete all user-imported data and derived recall state.
///
/// This keeps the schema, concept synonyms, and local model cache intact, but
/// removes imported artifacts, Quest state, embeddings, and search logs so the
/// user can retry browser import from a clean database.
///
/// Runs VACUUM after deletion to reclaim disk space.
pub fn clear_user_data(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM quest_artifacts", [])?;
    tx.execute("DELETE FROM quests", [])?;
    tx.execute("DELETE FROM artifact_embeddings", [])?;
    tx.execute("DELETE FROM search_log", [])?;
    tx.execute("DELETE FROM artifacts", [])?;
    tx.execute(
        "INSERT INTO artifacts_fts(artifacts_fts) VALUES('rebuild')",
        [],
    )?;
    tx.commit()?;

    // Reclaim disk space from deleted rows. VACUUM must run outside a transaction.
    conn.execute_batch("VACUUM")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("recall-db-test-{}.db", uuid::Uuid::new_v4()))
    }

    #[test]
    fn test_init_db_migrates_embedding_schema_from_old_artifacts_table() {
        let db_path = temp_db_path();
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                r#"
                CREATE TABLE artifacts (
                    id              TEXT PRIMARY KEY,
                    type            TEXT NOT NULL DEFAULT 'history',
                    title           TEXT,
                    url             TEXT,
                    domain          TEXT,
                    created_at      TEXT NOT NULL,
                    visited_at      TEXT,
                    is_bookmarked   INTEGER NOT NULL DEFAULT 0,
                    visit_count     INTEGER NOT NULL DEFAULT 0,
                    source          TEXT,
                    content         TEXT,
                    user_note       TEXT,
                    folder_path     TEXT,
                    import_batch    TEXT,
                    page_category   TEXT DEFAULT 'content',
                    noise_score     REAL NOT NULL DEFAULT 0.0,
                    extracted_query TEXT,
                    canonical_url   TEXT,
                    referrer_domain TEXT
                );
            "#,
            )
            .unwrap();
        }

        let conn = init_db(&db_path).unwrap();

        assert!(table_has_column(&conn, "artifacts", "embedding_version").unwrap());
        assert!(table_has_column(&conn, "artifacts", "search_text").unwrap());
        assert!(table_has_column(&conn, "artifacts_fts", "search_text").unwrap());
        let embedding_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'artifact_embeddings'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(embedding_tables, 1);

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[test]
    fn test_clear_user_data_removes_imported_and_derived_rows() {
        let db_path = temp_db_path();
        let mut conn = init_db(&db_path).unwrap();

        conn.execute(
            r#"INSERT INTO artifacts
                   (id, type, title, url, domain, created_at, visited_at, search_text)
               VALUES ('a1', 'history', '驾考宝典', 'https://example.com/a1',
                       'example.com', '2025-01-01T00:00:00', '2025-01-01T00:00:00',
                       '驾考 宝典')"#,
            [],
        )
        .unwrap();
        conn.execute(
            r#"INSERT INTO artifact_embeddings
                   (artifact_id, model, dims, embedding, updated_at)
               VALUES ('a1', 'test', 1, X'00000000', '2025-01-01T00:00:00')"#,
            [],
        )
        .unwrap();
        conn.execute(
            r#"INSERT INTO quests
                   (id, name, auto_name, started_at, ended_at, status, created_at, updated_at)
               VALUES ('q1', NULL, 'Quest', NULL, NULL, 'auto',
                       '2025-01-01T00:00:00', '2025-01-01T00:00:00')"#,
            [],
        )
        .unwrap();
        conn.execute(
            r#"INSERT INTO quest_artifacts
                   (quest_id, artifact_id, added_at, is_anchor)
               VALUES ('q1', 'a1', '2025-01-01T00:00:00', 1)"#,
            [],
        )
        .unwrap();
        conn.execute(
            r#"INSERT INTO search_log (query, result_ids, created_at)
               VALUES ('驾考', 'a1', '2025-01-01T00:00:00')"#,
            [],
        )
        .unwrap();
        conn.execute(
            r#"INSERT INTO concept_synonyms (term, synonym, weight, source)
               VALUES ('驾考', '驾驶证考试', 1.0, 'manual')"#,
            [],
        )
        .unwrap();

        clear_user_data(&mut conn).unwrap();

        for table in [
            "artifacts",
            "artifact_embeddings",
            "quests",
            "quest_artifacts",
            "search_log",
        ] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "{} should be empty after clear", table);
        }

        let fts_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM artifacts_fts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fts_count, 0);

        let synonym_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM concept_synonyms", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(synonym_count, 1, "manual synonyms should be preserved");

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }
}
