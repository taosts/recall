// ═══════════════════════════════════════════════════════════════════════════════
// Integration tests for Recall — Quest System + Core DB/Search pipeline
//
// These tests use in-memory SQLite databases (:memory:) so they run
// without any OS state, browser data, or Tauri runtime.
//
// Run with:   cd src-tauri && cargo test
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {

    use recall_app_lib::quest;
    use recall_app_lib::search;
    use recall_app_lib::segmenter::Segmenter;

    use rusqlite::Connection;

    // ─────────────────────────────────────────────────────────────────────
    // Helpers
    // ─────────────────────────────────────────────────────────────────────

    /// Create an in-memory SQLite database with the full Recall schema.
    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        // Run the same DDL as the real app
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        conn.execute_batch(r#"
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
                import_batch  TEXT,
                page_category TEXT DEFAULT 'content',
                noise_score REAL NOT NULL DEFAULT 0.0,
                extracted_query TEXT,
                canonical_url TEXT,
                referrer_domain TEXT
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS artifacts_fts USING fts5(
                title, url, domain, content, user_note, folder_path, extracted_query,
                content='artifacts', content_rowid='rowid',
                tokenize='unicode61'
            );

            CREATE TRIGGER IF NOT EXISTS artifacts_ai AFTER INSERT ON artifacts BEGIN
                INSERT INTO artifacts_fts(rowid, title, url, domain, content, user_note, folder_path, extracted_query)
                VALUES (new.rowid, new.title, new.url, new.domain,
                        new.content, new.user_note, new.folder_path, new.extracted_query);
            END;

            CREATE TRIGGER IF NOT EXISTS artifacts_ad AFTER DELETE ON artifacts BEGIN
                INSERT INTO artifacts_fts(artifacts_fts, rowid, title, url, domain, content, user_note, folder_path, extracted_query)
                VALUES ('delete', old.rowid, old.title, old.url, old.domain,
                        old.content, old.user_note, old.folder_path, old.extracted_query);
            END;

            CREATE TRIGGER IF NOT EXISTS artifacts_au AFTER UPDATE ON artifacts BEGIN
                INSERT INTO artifacts_fts(artifacts_fts, rowid, title, url, domain, content, user_note, folder_path, extracted_query)
                VALUES ('delete', old.rowid, old.title, old.url, old.domain,
                        old.content, old.user_note, old.folder_path, old.extracted_query);
                INSERT INTO artifacts_fts(rowid, title, url, domain, content, user_note, folder_path, extracted_query)
                VALUES (new.rowid, new.title, new.url, new.domain,
                        new.content, new.user_note, new.folder_path, new.extracted_query);
            END;

            CREATE INDEX IF NOT EXISTS idx_artifacts_visited_at ON artifacts(visited_at);
            CREATE INDEX IF NOT EXISTS idx_artifacts_domain     ON artifacts(domain);
            CREATE INDEX IF NOT EXISTS idx_artifacts_url        ON artifacts(url);

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
        "#).unwrap();
        conn
    }

    /// Insert a test artifact with minimal fields.
    /// `visited_at` should be an ISO 8601 timestamp like "2025-12-03T10:30:00".
    fn insert_artifact(
        conn: &Connection,
        id: &str,
        title: &str,
        url: &str,
        domain: &str,
        visited_at: &str,
        is_bookmarked: bool,
    ) {
        conn.execute(
            r#"INSERT INTO artifacts
                   (id, type, title, url, domain, created_at, visited_at,
                    is_bookmarked, visit_count, source)
               VALUES (?1, 'history', ?2, ?3, ?4, ?5, ?5, ?6, 1, 'edge')"#,
            rusqlite::params![
                id,
                title,
                url,
                domain,
                visited_at,
                if is_bookmarked { 1 } else { 0 },
            ],
        )
        .unwrap();
    }

    /// Insert a cluster of artifacts spaced `spacing_min` minutes apart,
    /// starting at the given base time. Returns the list of IDs.
    fn insert_cluster(
        conn: &Connection,
        prefix: &str,
        count: usize,
        base_time: &str,
        spacing_min: u32,
        domain: &str,
    ) -> Vec<String> {
        let base = chrono::NaiveDateTime::parse_from_str(base_time, "%Y-%m-%dT%H:%M:%S").unwrap();
        let mut ids = Vec::new();
        for i in 0..count {
            let ts = base + chrono::Duration::minutes(i as i64 * spacing_min as i64);
            let id = format!("{}-{}", prefix, i);
            let title = format!("Page {} about {}", i, domain);
            let url = format!("https://{}/page/{}", domain, i);
            insert_artifact(
                conn,
                &id,
                &title,
                &url,
                domain,
                &ts.format("%Y-%m-%dT%H:%M:%S").to_string(),
                i == 0, // first one is bookmarked
            );
            ids.push(id);
        }
        ids
    }

    // ═════════════════════════════════════════════════════════════════════
    // TEST GROUP 1: Database Schema & Initialization
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn test_db_init_creates_all_tables() {
        let conn = test_db();

        // Verify artifacts table exists
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM artifacts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);

        // Verify quests table exists
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM quests", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);

        // Verify quest_artifacts table exists
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM quest_artifacts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_artifact_insert_and_fts_trigger() {
        let conn = test_db();
        insert_artifact(
            &conn,
            "a1",
            "ZFS Cache Guide",
            "https://example.com/zfs",
            "example.com",
            "2025-12-03T10:00:00",
            false,
        );

        // FTS should be populated by trigger
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM artifacts_fts WHERE artifacts_fts MATCH '\"ZFS\"'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "FTS trigger should index the title");
    }

    // ═════════════════════════════════════════════════════════════════════
    // TEST GROUP 2: Quest Generation (Clustering Algorithm)
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn test_generate_quests_empty_db() {
        let conn = test_db();
        let created = quest::generate_quests(&conn).unwrap();
        assert_eq!(created, 0, "No artifacts → no Quests");
    }

    #[test]
    fn test_generate_quests_too_few_artifacts() {
        let conn = test_db();
        // Only 2 artifacts — below MIN_ARTIFACTS_PER_QUEST (3)
        insert_artifact(
            &conn,
            "a1",
            "Page 1",
            "https://a.com/1",
            "a.com",
            "2025-12-03T10:00:00",
            false,
        );
        insert_artifact(
            &conn,
            "a2",
            "Page 2",
            "https://a.com/2",
            "a.com",
            "2025-12-03T10:05:00",
            false,
        );

        let created = quest::generate_quests(&conn).unwrap();
        assert_eq!(created, 0, "Fewer than 3 artifacts should not form a Quest");
    }

    #[test]
    fn test_generate_quests_single_cluster() {
        let conn = test_db();
        // 5 artifacts, 10 minutes apart → all within 45min gap → 1 Quest
        insert_cluster(&conn, "c1", 5, "2025-12-03T10:00:00", 10, "rust-lang.org");

        let created = quest::generate_quests(&conn).unwrap();
        assert_eq!(created, 1, "5 closely-spaced artifacts should form 1 Quest");

        let quests = quest::list_quests(&conn, 10, 0).unwrap();
        assert_eq!(quests.len(), 1);
        assert_eq!(quests[0].artifact_count, 5);
        assert!(
            quests[0].anchor_count >= 1,
            "First artifact was bookmarked → anchor"
        );
    }

    #[test]
    fn test_generate_quests_two_clusters_by_gap() {
        let conn = test_db();
        // Cluster A: 4 artifacts at 10:00, 10:05, 10:10, 10:15
        insert_cluster(&conn, "a", 4, "2025-12-03T10:00:00", 5, "docs.rs");
        // Gap of 2 hours (>> 45 min)
        // Cluster B: 3 artifacts at 12:15, 12:20, 12:25
        insert_cluster(&conn, "b", 3, "2025-12-03T12:15:00", 5, "crates.io");

        let created = quest::generate_quests(&conn).unwrap();
        assert_eq!(created, 2, "2-hour gap should split into 2 Quests");

        let quests = quest::list_quests(&conn, 10, 0).unwrap();
        assert_eq!(quests.len(), 2);
        // Most recent first (started_at DESC)
        assert_eq!(quests[0].artifact_count, 3); // cluster B
        assert_eq!(quests[1].artifact_count, 4); // cluster A
    }

    #[test]
    fn test_generate_quests_max_duration_split() {
        let conn = test_db();
        // 20 artifacts, 30 minutes apart → total duration = 9.5 hours > MAX_QUEST_DURATION_HOURS (8)
        // Expect at least 2 Quests due to the 8h hard cap
        insert_cluster(
            &conn,
            "long",
            20,
            "2025-12-03T08:00:00",
            30,
            "stackoverflow.com",
        );

        let created = quest::generate_quests(&conn).unwrap();
        assert!(
            created >= 2,
            "8h max duration should force-split long browsing sessions, got {}",
            created
        );
    }

    #[test]
    fn test_generate_quests_idempotent() {
        let conn = test_db();
        insert_cluster(&conn, "c", 5, "2025-12-03T10:00:00", 10, "example.com");

        let created1 = quest::generate_quests(&conn).unwrap();
        assert_eq!(created1, 1);

        // Run again — should delete old auto Quests and recreate
        let created2 = quest::generate_quests(&conn).unwrap();
        assert_eq!(created2, 1);

        // Should still be exactly 1 Quest total (not 2)
        let quests = quest::list_quests(&conn, 10, 0).unwrap();
        assert_eq!(quests.len(), 1);
    }

    // ═════════════════════════════════════════════════════════════════════
    // TEST GROUP 3: Quest CRUD Operations
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn test_get_quest_returns_artifacts() {
        let conn = test_db();
        insert_cluster(&conn, "g", 4, "2025-12-03T10:00:00", 10, "github.com");
        quest::generate_quests(&conn).unwrap();

        let quests = quest::list_quests(&conn, 10, 0).unwrap();
        assert_eq!(quests.len(), 1);

        let full = quest::get_quest(&conn, &quests[0].id).unwrap();
        assert_eq!(full.artifacts.len(), 4);
        assert_eq!(full.status, "auto");
        // Artifacts should be time-ordered
        for i in 1..full.artifacts.len() {
            assert!(
                full.artifacts[i].visited_at >= full.artifacts[i - 1].visited_at,
                "Artifacts should be sorted by visited_at ASC"
            );
        }
    }

    #[test]
    fn test_rename_quest_sets_confirmed() {
        let conn = test_db();
        insert_cluster(&conn, "r", 4, "2025-12-03T10:00:00", 10, "reddit.com");
        quest::generate_quests(&conn).unwrap();

        let quests = quest::list_quests(&conn, 10, 0).unwrap();
        let quest_id = &quests[0].id;

        quest::rename_quest(&conn, quest_id, "研究 ZFS 缓存").unwrap();

        let updated = quest::get_quest(&conn, quest_id).unwrap();
        assert_eq!(updated.name, Some("研究 ZFS 缓存".to_string()));
        assert_eq!(updated.status, "confirmed");
    }

    #[test]
    fn test_archive_quest_hides_from_list() {
        let conn = test_db();
        insert_cluster(&conn, "ar", 4, "2025-12-03T10:00:00", 10, "archlinux.org");
        quest::generate_quests(&conn).unwrap();

        let quests = quest::list_quests(&conn, 10, 0).unwrap();
        assert_eq!(quests.len(), 1);

        quest::archive_quest(&conn, &quests[0].id).unwrap();

        // Archived Quests should not appear in list
        let quests_after = quest::list_quests(&conn, 10, 0).unwrap();
        assert_eq!(
            quests_after.len(),
            0,
            "Archived Quest should be hidden from list"
        );
    }

    #[test]
    fn test_archive_does_not_delete() {
        let conn = test_db();
        insert_cluster(&conn, "nd", 4, "2025-12-03T10:00:00", 10, "nixos.org");
        quest::generate_quests(&conn).unwrap();

        let quests = quest::list_quests(&conn, 10, 0).unwrap();
        let quest_id = quests[0].id.clone();

        quest::archive_quest(&conn, &quest_id).unwrap();

        // get_quest should still work (it doesn't filter by status)
        let q = quest::get_quest(&conn, &quest_id).unwrap();
        assert_eq!(q.status, "archived");
        assert_eq!(q.artifacts.len(), 4);
    }

    #[test]
    fn test_rename_preserves_through_regenerate() {
        let conn = test_db();
        insert_cluster(&conn, "p", 4, "2025-12-03T10:00:00", 10, "python.org");
        quest::generate_quests(&conn).unwrap();

        let quests = quest::list_quests(&conn, 10, 0).unwrap();
        quest::rename_quest(&conn, &quests[0].id, "Learning Python").unwrap();

        // Regenerate — should NOT delete confirmed Quests
        quest::generate_quests(&conn).unwrap();

        // The confirmed Quest should survive
        let after = quest::list_quests(&conn, 10, 0).unwrap();
        let confirmed: Vec<_> = after
            .iter()
            .filter(|q| q.display_name == "Learning Python")
            .collect();
        assert_eq!(
            confirmed.len(),
            1,
            "Confirmed (user-renamed) Quest should survive regeneration"
        );
    }

    // ═════════════════════════════════════════════════════════════════════
    // TEST GROUP 4: Quest Merge
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn test_merge_quests() {
        let conn = test_db();
        insert_cluster(&conn, "m1", 4, "2025-12-03T10:00:00", 5, "docs.rs");
        // 2 hours later — separate Quest
        insert_cluster(&conn, "m2", 3, "2025-12-03T12:00:00", 5, "docs.rs");

        quest::generate_quests(&conn).unwrap();
        let quests = quest::list_quests(&conn, 10, 0).unwrap();
        assert_eq!(quests.len(), 2);

        let ids: Vec<String> = quests.iter().map(|q| q.id.clone()).collect();
        let survivor_id = quest::merge_quests(&conn, ids).unwrap();

        // Should now be 1 Quest with 7 artifacts
        let after = quest::list_quests(&conn, 10, 0).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].id, survivor_id);
        assert_eq!(after[0].artifact_count, 7);
    }

    #[test]
    fn test_merge_quests_less_than_2_fails() {
        let conn = test_db();
        let result = quest::merge_quests(&conn, vec!["single-id".to_string()]);
        assert!(result.is_err(), "Merging fewer than 2 Quests should fail");
    }

    // ═════════════════════════════════════════════════════════════════════
    // TEST GROUP 5: Quest ↔ Artifact Lookup
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn test_get_quest_for_artifact() {
        let conn = test_db();
        let ids = insert_cluster(&conn, "qa", 4, "2025-12-03T10:00:00", 10, "example.com");
        quest::generate_quests(&conn).unwrap();

        // Each artifact should belong to exactly 1 Quest
        let quests = quest::get_quest_for_artifact(&conn, &ids[0]).unwrap();
        assert_eq!(quests.len(), 1, "Artifact should belong to 1 Quest");
        assert_eq!(quests[0].artifact_count, 4);
    }

    #[test]
    fn test_get_quest_for_unknown_artifact() {
        let conn = test_db();
        let quests = quest::get_quest_for_artifact(&conn, "nonexistent-id").unwrap();
        assert_eq!(quests.len(), 0, "Unknown artifact should return empty");
    }

    // ═════════════════════════════════════════════════════════════════════
    // TEST GROUP 6: Auto-Naming
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn test_auto_name_contains_domain() {
        let conn = test_db();
        insert_cluster(&conn, "dn", 5, "2025-12-03T10:00:00", 10, "rust-lang.org");
        quest::generate_quests(&conn).unwrap();

        let quests = quest::list_quests(&conn, 10, 0).unwrap();
        assert_eq!(quests.len(), 1);
        // auto_name should contain the domain or its prefix
        let name = &quests[0].display_name;
        assert!(
            name.to_lowercase().contains("rust") || name.contains("rust-lang"),
            "Auto-name '{}' should reference the primary domain 'rust-lang.org'",
            name
        );
    }

    // ═════════════════════════════════════════════════════════════════════
    // TEST GROUP 7: Search + FTS Pipeline (Phase 1 regression)
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn test_search_returns_results() {
        let conn = test_db();
        insert_artifact(
            &conn,
            "s1",
            "OpenWrt DNS configuration guide",
            "https://openwrt.org/docs/dns",
            "openwrt.org",
            "2025-12-03T10:00:00",
            true,
        );
        insert_artifact(
            &conn,
            "s2",
            "Rust programming tutorial",
            "https://doc.rust-lang.org/book",
            "doc.rust-lang.org",
            "2025-12-03T10:05:00",
            false,
        );

        let segmenter = Segmenter::new();
        let results =
            search::search(&conn, &segmenter, "OpenWrt DNS", None, None, None, 30).unwrap();
        assert!(
            !results.is_empty(),
            "FTS search for 'OpenWrt DNS' should find the matching artifact"
        );
        assert_eq!(results[0].artifact.domain, Some("openwrt.org".to_string()));
    }

    #[test]
    fn test_search_empty_query_errors_at_fts_layer() {
        let conn = test_db();
        insert_artifact(
            &conn,
            "e1",
            "Some page",
            "https://a.com",
            "a.com",
            "2025-12-03T10:00:00",
            false,
        );
        // The empty-query guard is in lib.rs (Tauri command layer), not search::search().
        // Direct FTS5 call with empty/whitespace input should return an error.
        let segmenter = Segmenter::new();
        let result = search::search(&conn, &segmenter, "   ", None, None, None, 30);
        assert!(
            result.is_err(),
            "Raw FTS5 search with empty query should error (guard is in lib.rs)"
        );
    }

    #[test]
    fn test_context_window() {
        let conn = test_db();
        // Insert 3 artifacts: 10:00, 10:10, 12:00
        insert_artifact(
            &conn,
            "ctx1",
            "Page A",
            "https://a.com",
            "a.com",
            "2025-12-03T10:00:00",
            false,
        );
        insert_artifact(
            &conn,
            "ctx2",
            "Page B",
            "https://b.com",
            "b.com",
            "2025-12-03T10:10:00",
            false,
        );
        insert_artifact(
            &conn,
            "ctx3",
            "Page C",
            "https://c.com",
            "c.com",
            "2025-12-03T12:00:00",
            false,
        );

        // Context of ctx1 with 30-min window should include ctx2 but NOT ctx3
        let context = search::get_context(&conn, "ctx1", 30).unwrap();
        let context_ids: Vec<&str> = context.iter().map(|a| a.id.as_str()).collect();
        assert!(
            context_ids.contains(&"ctx2"),
            "ctx2 (10 min away) should be in 30-min context"
        );
        assert!(
            !context_ids.contains(&"ctx3"),
            "ctx3 (2 hours away) should NOT be in 30-min context"
        );
    }

    #[test]
    fn test_user_note_roundtrip() {
        let conn = test_db();
        insert_artifact(
            &conn,
            "note1",
            "Some page",
            "https://a.com",
            "a.com",
            "2025-12-03T10:00:00",
            false,
        );

        search::set_user_note(&conn, "note1", "This was useful for debugging").unwrap();

        // Search and verify note is returned
        let segmenter = Segmenter::new();
        let results = search::search(&conn, &segmenter, "page", None, None, None, 30).unwrap();
        assert!(!results.is_empty());
        assert_eq!(
            results[0].artifact.user_note,
            Some("This was useful for debugging".to_string())
        );
    }

    #[test]
    fn test_db_stats() {
        let conn = test_db();
        insert_artifact(
            &conn,
            "st1",
            "Page 1",
            "https://a.com/1",
            "a.com",
            "2025-12-03T10:00:00",
            true,
        );
        insert_artifact(
            &conn,
            "st2",
            "Page 2",
            "https://a.com/2",
            "a.com",
            "2025-12-04T10:00:00",
            false,
        );

        let stats = search::get_stats(&conn).unwrap();
        assert_eq!(stats.total_artifacts, 2);
        assert_eq!(stats.total_bookmarks, 1);
        assert!(stats.oldest_record.is_some());
        assert!(stats.newest_record.is_some());
    }

    // ═════════════════════════════════════════════════════════════════════
    // TEST GROUP 8: End-to-End Pipeline
    // ═════════════════════════════════════════════════════════════════════

    #[test]
    fn test_e2e_import_search_quest_flow() {
        let conn = test_db();

        // Step 1: Simulate importing browsing data
        insert_cluster(&conn, "e2e-zfs", 5, "2025-12-03T20:00:00", 8, "reddit.com");
        insert_cluster(
            &conn,
            "e2e-openwrt",
            4,
            "2025-12-04T14:00:00",
            10,
            "openwrt.org",
        );

        // Step 2: Search works
        let segmenter = Segmenter::new();
        let results = search::search(&conn, &segmenter, "reddit", None, None, None, 30).unwrap();
        assert!(!results.is_empty(), "Should find reddit pages");

        // Step 3: Generate Quests
        let created = quest::generate_quests(&conn).unwrap();
        assert_eq!(created, 2, "Should create 2 Quests (different days)");

        // Step 4: List and verify
        let quests = quest::list_quests(&conn, 10, 0).unwrap();
        assert_eq!(quests.len(), 2);

        // Step 5: Get full Quest
        let full = quest::get_quest(&conn, &quests[0].id).unwrap();
        assert!(!full.artifacts.is_empty());

        // Step 6: Rename a Quest
        quest::rename_quest(&conn, &quests[0].id, "OpenWrt 旁路由研究").unwrap();
        let renamed = quest::get_quest(&conn, &quests[0].id).unwrap();
        assert_eq!(renamed.name, Some("OpenWrt 旁路由研究".to_string()));

        // Step 7: Artifact → Quest reverse lookup
        let first_artifact_id = &full.artifacts[0].id;
        let linked_quests = quest::get_quest_for_artifact(&conn, first_artifact_id).unwrap();
        assert_eq!(linked_quests.len(), 1);

        // Step 8: Archive one Quest
        quest::archive_quest(&conn, &quests[1].id).unwrap();
        let remaining = quest::list_quests(&conn, 10, 0).unwrap();
        assert_eq!(
            remaining.len(),
            1,
            "Only non-archived Quest should remain in list"
        );
    }
}
