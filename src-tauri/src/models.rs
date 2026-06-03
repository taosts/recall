use serde::{Deserialize, Serialize};

/// Core data unit — an "information trace" left by the user.
/// Could be a bookmark, a visited page, a download, or a manual note.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Artifact {
    pub id: String,
    /// "bookmark" | "history" | "download" | "note"
    pub r#type: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub domain: Option<String>,
    /// ISO 8601 string, when the record was created/imported
    pub created_at: String,
    /// ISO 8601 string, most recent visit time
    pub visited_at: Option<String>,
    pub is_bookmarked: bool,
    pub visit_count: i64,
    /// "edge" | "chrome" | "manual"
    pub source: Option<String>,
    /// Page snippet or body text (future extension)
    pub content: Option<String>,
    /// User-written note attached to this artifact
    pub user_note: Option<String>,
    /// Bookmark folder path, e.g. "Bookmarks Bar/Dev/Rust"
    pub folder_path: Option<String>,
    /// Import batch identifier
    pub import_batch: Option<String>,
}

/// A search result enriched with BM25 relevance score and temporal context.
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub artifact: Artifact,
    /// BM25 score from FTS5 (lower magnitude = better match in SQLite)
    pub score: f64,
    /// Artifacts accessed within the context time window of this result
    pub context: Vec<Artifact>,
}

/// Statistics returned after a browser import operation.
#[derive(Debug, Serialize, Deserialize)]
pub struct ImportStats {
    pub browser: String,
    pub bookmarks_imported: usize,
    pub history_imported: usize,
    pub duplicates_skipped: usize,
    pub errors: Vec<String>,
}

/// Detected browser installation information.
#[derive(Debug, Serialize, Deserialize)]
pub struct BrowserInfo {
    /// "edge" | "chrome"
    pub id: String,
    pub name: String,
    pub bookmarks_path: String,
    pub history_path: String,
    pub available: bool,
}

/// Overall database statistics shown in the status bar.
#[derive(Debug, Serialize, Deserialize)]
pub struct DbStats {
    pub total_artifacts: i64,
    pub total_bookmarks: i64,
    pub total_history: i64,
    pub oldest_record: Option<String>,
    pub newest_record: Option<String>,
    pub last_import: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 2: Quest (探索任务) data structures
// ─────────────────────────────────────────────────────────────────────────────

/// Full Quest object with its complete artifact list.
/// Used when viewing a single Quest's detail page.
#[derive(Debug, Serialize, Deserialize)]
pub struct Quest {
    pub id: String,
    /// User-set name (None until user explicitly renames)
    pub name: Option<String>,
    /// System-generated name based on domains/keywords
    pub auto_name: Option<String>,
    /// ISO 8601 timestamp of earliest artifact in this Quest
    pub started_at: Option<String>,
    /// ISO 8601 timestamp of latest artifact in this Quest
    pub ended_at: Option<String>,
    /// "auto" | "confirmed" | "archived"
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    /// All artifacts belonging to this Quest, ordered by visited_at
    pub artifacts: Vec<Artifact>,
}

/// Lightweight Quest summary for list views (no artifact details).
/// Used in the Quest list panel and as search result annotations.
#[derive(Debug, Serialize, Deserialize)]
pub struct QuestSummary {
    pub id: String,
    /// Display name: prefers user-set name, falls back to auto_name
    pub display_name: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub status: String,
    pub artifact_count: i64,
    pub anchor_count: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    /// Helper: build a fully-populated Artifact for reuse across tests.
    fn sample_artifact() -> Artifact {
        Artifact {
            id: "art-001".into(),
            r#type: "bookmark".into(),
            title: Some("Rust Book".into()),
            url: Some("https://doc.rust-lang.org/book/".into()),
            domain: Some("doc.rust-lang.org".into()),
            created_at: "2025-01-01T00:00:00Z".into(),
            visited_at: Some("2025-06-01T12:00:00Z".into()),
            is_bookmarked: true,
            visit_count: 42,
            source: Some("edge".into()),
            content: Some("The Rust Programming Language".into()),
            user_note: Some("Must read".into()),
            folder_path: Some("Bookmarks Bar/Dev/Rust".into()),
            import_batch: Some("batch-abc".into()),
        }
    }

    #[test]
    fn test_artifact_serialize_deserialize() {
        let original = sample_artifact();
        let json = serde_json::to_string(&original).expect("serialize");
        let roundtripped: Artifact = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(roundtripped.id, "art-001");
        assert_eq!(roundtripped.r#type, "bookmark");
        assert_eq!(roundtripped.title.as_deref(), Some("Rust Book"));
        assert_eq!(
            roundtripped.url.as_deref(),
            Some("https://doc.rust-lang.org/book/")
        );
        assert!(roundtripped.is_bookmarked);
        assert_eq!(roundtripped.visit_count, 42);
        assert_eq!(roundtripped.source.as_deref(), Some("edge"));
        assert_eq!(roundtripped.folder_path.as_deref(), Some("Bookmarks Bar/Dev/Rust"));
    }

    #[test]
    fn test_artifact_optional_fields_null() {
        let artifact = Artifact {
            id: "art-002".into(),
            r#type: "history".into(),
            title: None,
            url: None,
            domain: None,
            created_at: "2025-01-01T00:00:00Z".into(),
            visited_at: None,
            is_bookmarked: false,
            visit_count: 0,
            source: None,
            content: None,
            user_note: None,
            folder_path: None,
            import_batch: None,
        };

        let json = serde_json::to_string(&artifact).expect("serialize");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse");

        for field in &[
            "title",
            "url",
            "domain",
            "visited_at",
            "source",
            "content",
            "user_note",
            "folder_path",
            "import_batch",
        ] {
            assert!(
                v[field].is_null(),
                "expected field '{}' to be null, got {:?}",
                field,
                v[field]
            );
        }
    }

    #[test]
    fn test_quest_serialize() {
        let quest = Quest {
            id: "quest-001".into(),
            name: Some("Rust deep-dive".into()),
            auto_name: Some("doc.rust-lang.org research".into()),
            started_at: Some("2025-01-01T00:00:00Z".into()),
            ended_at: Some("2025-06-01T00:00:00Z".into()),
            status: "confirmed".into(),
            created_at: "2025-01-01T00:00:00Z".into(),
            updated_at: "2025-06-01T00:00:00Z".into(),
            artifacts: vec![sample_artifact()],
        };

        let json = serde_json::to_string(&quest).expect("serialize");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse");

        assert_eq!(v["id"], "quest-001");
        assert_eq!(v["name"], "Rust deep-dive");
        assert_eq!(v["status"], "confirmed");
        assert!(v["artifacts"].is_array());
        assert_eq!(v["artifacts"].as_array().unwrap().len(), 1);
        assert_eq!(v["artifacts"][0]["id"], "art-001");
    }

    #[test]
    fn test_quest_summary_serialize() {
        let summary = QuestSummary {
            id: "quest-002".into(),
            display_name: "Evening browsing".into(),
            started_at: Some("2025-05-01T20:00:00Z".into()),
            ended_at: None,
            status: "auto".into(),
            artifact_count: 7,
            anchor_count: 2,
        };

        let json = serde_json::to_string(&summary).expect("serialize");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse");

        assert_eq!(v["display_name"], "Evening browsing");
        assert_eq!(v["artifact_count"], 7);
        assert_eq!(v["anchor_count"], 2);
        assert!(v["ended_at"].is_null());
    }

    #[test]
    fn test_search_result_serialize() {
        let result = SearchResult {
            artifact: sample_artifact(),
            score: -3.14,
            context: vec![sample_artifact()],
        };

        let json = serde_json::to_string(&result).expect("serialize");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse");

        assert!((v["score"].as_f64().unwrap() - (-3.14)).abs() < f64::EPSILON);
        assert_eq!(v["artifact"]["id"], "art-001");
        assert!(v["context"].is_array());
        assert_eq!(v["context"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_import_stats_default() {
        let stats = ImportStats {
            browser: "edge".into(),
            bookmarks_imported: 0,
            history_imported: 0,
            duplicates_skipped: 0,
            errors: vec![],
        };

        let json = serde_json::to_string(&stats).expect("serialize");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse");

        assert_eq!(v["browser"], "edge");
        assert_eq!(v["bookmarks_imported"], 0);
        assert_eq!(v["history_imported"], 0);
        assert_eq!(v["duplicates_skipped"], 0);
        assert!(v["errors"].as_array().unwrap().is_empty());
    }
}
