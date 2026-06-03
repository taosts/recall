use crate::models::{Artifact, DbStats, SearchResult};
use rusqlite::{params, Connection, Result};

// ─────────────────────────────────────────────────────────────────────────────
// Row → Artifact helper
// ─────────────────────────────────────────────────────────────────────────────

pub fn row_to_artifact(row: &rusqlite::Row) -> Result<Artifact> {
    Ok(Artifact {
        id: row.get(0)?,
        r#type: row.get(1)?,
        title: row.get(2)?,
        url: row.get(3)?,
        domain: row.get(4)?,
        created_at: row.get(5)?,
        visited_at: row.get(6)?,
        is_bookmarked: {
            let v: i64 = row.get(7)?;
            v != 0
        },
        visit_count: row.get(8)?,
        source: row.get(9)?,
        content: row.get(10)?,
        user_note: row.get(11)?,
        folder_path: row.get(12)?,
        import_batch: row.get(13)?,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Full-text search via FTS5 + BM25 ranking
// ─────────────────────────────────────────────────────────────────────────────

/// Search artifacts using FTS5 MATCH with optional time and source filters.
///
/// `query`       — user's raw search text (passed straight to FTS5 MATCH)
/// `date_from`   — optional ISO 8601 lower bound for visited_at
/// `date_to`     — optional ISO 8601 upper bound for visited_at
/// `source`      — optional source filter ("edge" | "chrome" | null = all)
/// `context_min` — minutes for the context window (default 30)
///
/// Returns ranked SearchResult list, each enriched with temporal context.
pub fn search(
    conn: &Connection,
    query: &str,
    date_from: Option<&str>,
    date_to: Option<&str>,
    source: Option<&str>,
    context_min: i64,
) -> Result<Vec<SearchResult>> {
    // Build dynamic WHERE clauses
    let filters = vec!["fts.rowid = a.rowid"];
    let mut extra = String::new();
    if date_from.is_some() {
        extra.push_str(" AND a.visited_at >= ?3");
    }
    if date_to.is_some() {
        extra.push_str(if date_from.is_some() {
            " AND a.visited_at <= ?4"
        } else {
            " AND a.visited_at <= ?3"
        });
    }

    let source_clause = if source.is_some() {
        " AND a.source = ?5"
    } else {
        ""
    };
    let _ = filters; // used for clarity above

    let sql = format!(
        r#"SELECT a.id, a.type, a.title, a.url, a.domain, a.created_at,
                  a.visited_at, a.is_bookmarked, a.visit_count, a.source,
                  a.content, a.user_note, a.folder_path, a.import_batch,
                  bm25(artifacts_fts) AS score
           FROM artifacts_fts fts
           JOIN artifacts a ON fts.rowid = a.rowid
           WHERE artifacts_fts MATCH ?1
           {}{}
           ORDER BY score
           LIMIT 50"#,
        extra, source_clause
    );

    let mut stmt = conn.prepare(&sql)?;

    // Bind parameters in order
    let query_escaped = escape_fts_query(query);

    let mut rows = match (date_from, date_to, source) {
        (Some(df), Some(dt), Some(s)) => stmt.query(params![query_escaped, "", df, dt, s])?,
        (Some(df), Some(dt), None) => stmt.query(params![query_escaped, "", df, dt])?,
        (Some(df), None, Some(s)) => stmt.query(params![query_escaped, "", df, s])?,
        (None, Some(dt), Some(s)) => stmt.query(params![query_escaped, "", dt, s])?,
        (Some(df), None, None) => stmt.query(params![query_escaped, "", df])?,
        (None, Some(dt), None) => stmt.query(params![query_escaped, "", dt])?,
        (None, None, Some(s)) => stmt.query(params![query_escaped, "", s])?,
        (None, None, None) => stmt.query(params![query_escaped])?,
    };

    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        let artifact = row_to_artifact(row)?;
        let score: f64 = row.get(14)?;
        let context = get_context(conn, &artifact.id, context_min)?;
        results.push(SearchResult {
            artifact,
            score,
            context,
        });
    }

    Ok(results)
}

/// Escape special FTS5 characters in user input to prevent query syntax errors.
fn escape_fts_query(input: &str) -> String {
    // Wrap each whitespace-separated token in double quotes for exact matching
    let tokens: Vec<String> = input
        .split_whitespace()
        .map(|t| {
            let clean = t.replace('"', "");
            if clean.is_empty() {
                String::new()
            } else {
                format!("\"{}\"", clean)
            }
        })
        .filter(|t| !t.is_empty())
        .collect();

    if tokens.is_empty() {
        String::new()
    } else {
        tokens.join(" OR ")
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Context: "what else were you browsing at the same time?"
// ─────────────────────────────────────────────────────────────────────────────

/// Retrieve artifacts accessed within ±window_minutes of the given artifact's
/// visited_at timestamp. This is the core memory-trigger feature.
///
/// The window is user-configurable (15 / 30 / 60 / 120 min).
/// Future: auto-adapt window size based on Quest type.
pub fn get_context(
    conn: &Connection,
    artifact_id: &str,
    window_minutes: i64,
) -> Result<Vec<Artifact>> {
    // First, get the visited_at of the target artifact
    let visited_at: Option<String> = conn
        .query_row(
            "SELECT visited_at FROM artifacts WHERE id = ?1",
            params![artifact_id],
            |row| row.get(0),
        )
        .unwrap_or(None);

    let Some(ts) = visited_at else {
        return Ok(vec![]);
    };

    let mut stmt = conn.prepare(
        r#"SELECT id, type, title, url, domain, created_at, visited_at,
                  is_bookmarked, visit_count, source, content, user_note,
                  folder_path, import_batch
           FROM artifacts
           WHERE id != ?1
             AND visited_at IS NOT NULL
             AND ABS(
                   (JULIANDAY(visited_at) - JULIANDAY(?2)) * 1440
                 ) <= ?3
           ORDER BY ABS(JULIANDAY(visited_at) - JULIANDAY(?2))
           LIMIT 20"#,
    )?;

    let artifact_iter = stmt.query_map(params![artifact_id, ts, window_minutes], |row| {
        row_to_artifact(row)
    })?;

    let mut context = Vec::new();
    for a in artifact_iter {
        context.push(a?);
    }
    Ok(context)
}

// ─────────────────────────────────────────────────────────────────────────────
// Database statistics
// ─────────────────────────────────────────────────────────────────────────────

pub fn get_stats(conn: &Connection) -> Result<DbStats> {
    let total_artifacts: i64 =
        conn.query_row("SELECT COUNT(*) FROM artifacts", [], |r| r.get(0))?;
    let total_bookmarks: i64 = conn.query_row(
        "SELECT COUNT(*) FROM artifacts WHERE is_bookmarked = 1",
        [],
        |r| r.get(0),
    )?;
    let total_history: i64 = conn.query_row(
        "SELECT COUNT(*) FROM artifacts WHERE type = 'history'",
        [],
        |r| r.get(0),
    )?;
    let oldest_record: Option<String> = conn.query_row(
        "SELECT MIN(visited_at) FROM artifacts WHERE visited_at IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    let newest_record: Option<String> = conn.query_row(
        "SELECT MAX(visited_at) FROM artifacts WHERE visited_at IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    let last_import: Option<String> =
        conn.query_row("SELECT MAX(created_at) FROM artifacts", [], |r| r.get(0))?;

    Ok(DbStats {
        total_artifacts,
        total_bookmarks,
        total_history,
        oldest_record,
        newest_record,
        last_import,
    })
}

/// Update user_note for a specific artifact.
pub fn set_user_note(conn: &Connection, artifact_id: &str, note: &str) -> Result<()> {
    conn.execute(
        "UPDATE artifacts SET user_note = ?1 WHERE id = ?2",
        params![note, artifact_id],
    )?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // 1. Single token → wrapped in double quotes, no OR
    #[test]
    fn test_escape_single_word() {
        assert_eq!(escape_fts_query("hello"), r#""hello""#);
    }

    // 2. Two tokens → each quoted, joined with OR
    #[test]
    fn test_escape_multi_words() {
        assert_eq!(escape_fts_query("ZFS cache"), r#""ZFS" OR "cache""#);
    }

    // 3. Embedded double quotes are stripped before wrapping
    #[test]
    fn test_escape_strips_quotes() {
        assert_eq!(escape_fts_query(r#"he"llo"#), r#""hello""#);
    }

    // 4. Empty string → empty output (no tokens)
    #[test]
    fn test_escape_empty_string() {
        assert_eq!(escape_fts_query(""), "");
    }

    // 5. Whitespace-only input → empty output (split_whitespace yields nothing)
    #[test]
    fn test_escape_only_whitespace() {
        assert_eq!(escape_fts_query("   "), "");
    }

    // 6. Tabs and multiple spaces are treated as whitespace delimiters
    #[test]
    fn test_escape_mixed_spaces_tabs() {
        assert_eq!(
            escape_fts_query("alpha\t\tbeta   gamma"),
            r#""alpha" OR "beta" OR "gamma""#,
        );
    }

    // 7. CJK / non-ASCII tokens are handled correctly
    #[test]
    fn test_escape_chinese_characters() {
        assert_eq!(escape_fts_query("ZFS 缓存"), r#""ZFS" OR "缓存""#);
    }

    // 8. Characters that would break bare FTS5 syntax (parens, colons, etc.)
    //    are safely wrapped inside double quotes so FTS5 treats them as literals.
    #[test]
    fn test_escape_special_fts_chars() {
        assert_eq!(
            escape_fts_query("foo:bar (baz) qux*"),
            r#""foo:bar" OR "(baz)" OR "qux*""#,
        );
    }
}
