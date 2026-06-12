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
    /// "search_query" | "content" | "redirect" | "login" | "utility"
    pub page_category: Option<String>,
    /// 0.0 = useful content, 1.0 = pure noise
    pub noise_score: f64,
    /// Search engine query extracted from the URL, if any
    pub extracted_query: Option<String>,
    /// URL normalized by removing tracking parameters
    pub canonical_url: Option<String>,
    /// Inferred referrer/source domain for future context work
    pub referrer_domain: Option<String>,
}

/// Describes which search layer contributed to a result.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MatchLayer {
    /// "literal" | "expanded" | "semantic"
    pub layer: String,
    /// 1-based rank within this layer.
    pub rank: usize,
    /// Raw score from the layer (BM25 magnitude or cosine similarity).
    pub raw_score: f64,
}

/// Explanation of why a result was returned.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchExplanation {
    pub match_layers: Vec<MatchLayer>,
    pub expanded_terms: Vec<String>,
    pub literal_query: String,
    /// Empty string if identical to literal_query.
    pub expanded_query: String,
    pub semantic_score: Option<f64>,
    pub noise_applied: bool,
    pub noise_score: f64,
    /// Query/expansion terms that actually appear in this result's text — the
    /// honest "matched on" set surfaced in the Why panel.
    pub matched_terms: Vec<String>,
}

/// A search result enriched with relevance score, temporal context, and explanation.
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub artifact: Artifact,
    /// BM25 score from FTS5 (lower magnitude = better match in SQLite)
    pub score: f64,
    /// Artifacts accessed within the context time window of this result
    pub context: Vec<Artifact>,
    /// Phase 2: Quest associations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quests: Option<Vec<QuestSummary>>,
    /// Phase 4: visible explanation for result ranking.
    pub explanation: SearchExplanation,
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
    /// The first search query that kicked off this exploration.
    pub origin_query: Option<String>,
    /// IDs of anchor artifacts: bookmarks, high-visit pages, or search queries.
    pub anchor_ids: Vec<String>,
    /// Top domains by weighted visit count.
    pub top_domains: Vec<(String, i64)>,
    /// Number of low-value pages hidden by default in the Quest view.
    pub noise_count: i64,
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
            page_category: Some("content".into()),
            noise_score: 0.0,
            extracted_query: None,
            canonical_url: Some("https://doc.rust-lang.org/book/".into()),
            referrer_domain: None,
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
        assert_eq!(
            roundtripped.folder_path.as_deref(),
            Some("Bookmarks Bar/Dev/Rust")
        );
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
            page_category: None,
            noise_score: 0.0,
            extracted_query: None,
            canonical_url: None,
            referrer_domain: None,
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
            "page_category",
            "extracted_query",
            "canonical_url",
            "referrer_domain",
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
            origin_query: Some("驾考宝典的题库哪里来的".into()),
            anchor_ids: vec!["art-001".into()],
            top_domains: vec![("doc.rust-lang.org".into(), 5)],
            noise_count: 0,
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
            quests: None,
            explanation: SearchExplanation {
                match_layers: vec![MatchLayer {
                    layer: "literal".into(),
                    rank: 1,
                    raw_score: -3.14,
                }],
                expanded_terms: vec!["考驾照".into(), "驾考".into()],
                literal_query: "\"考驾照\"".into(),
                expanded_query: "\"考驾照\" OR \"驾考\" OR \"驾照\"".into(),
                semantic_score: None,
                noise_applied: false,
                noise_score: 0.0,
                matched_terms: vec!["考驾照".into()],
            },
        };

        let json = serde_json::to_string(&result).expect("serialize");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse");

        assert!((v["score"].as_f64().unwrap() - (-3.14)).abs() < f64::EPSILON);
        assert_eq!(v["artifact"]["id"], "art-001");
        assert!(v["context"].is_array());
        assert_eq!(v["context"].as_array().unwrap().len(), 1);
        assert!(v["explanation"]["match_layers"].is_array());
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
