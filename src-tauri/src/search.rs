use crate::expander::{build_fts_query_from_terms, QueryExpander};
use crate::models::{Artifact, DbStats, SearchResult};
use crate::segmenter::{jaccard_similarity, Segmenter};
use crate::semantic;
use rusqlite::types::ToSql;
use rusqlite::{params, Connection, Result};
use std::collections::HashMap;

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
        page_category: row.get(14)?,
        noise_score: row.get(15)?,
        extracted_query: row.get(16)?,
        canonical_url: row.get(17)?,
        referrer_domain: row.get(18)?,
    })
}

#[derive(Clone)]
struct RankedArtifact {
    artifact: Artifact,
    score: f64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Full-text search via BM25 + query expansion + RRF
// ─────────────────────────────────────────────────────────────────────────────

/// Search artifacts using two BM25 layers:
/// 1. literal query terms segmented for search
/// 2. expanded terms from synonyms, local co-occurrence, and PRF
///
/// The layers are merged with Reciprocal Rank Fusion, then high-noise pages
/// are downweighted using Phase 3A's noise_score.
pub fn search(
    conn: &Connection,
    segmenter: &Segmenter,
    semantic_query: Option<&[f32]>,
    query: &str,
    date_from: Option<&str>,
    date_to: Option<&str>,
    source: Option<&str>,
    context_min: i64,
) -> Result<Vec<SearchResult>> {
    let literal_terms = segmenter.cut_for_search(query);
    let literal_fts_query = build_fts_query_from_terms(&literal_terms);
    let layer1 = search_bm25(conn, &literal_fts_query, date_from, date_to, source, 50)?;

    let expanded = QueryExpander::new().expand(conn, segmenter, query);
    let layer2 = if expanded.fts_query.is_empty() || expanded.fts_query == literal_fts_query {
        Vec::new()
    } else {
        search_bm25(conn, &expanded.fts_query, date_from, date_to, source, 50)?
    };

    let layer3 = if let Some(query_vector) = semantic_query {
        semantic::search_semantic(conn, query_vector, date_from, date_to, source, 50)?
            .into_iter()
            .map(|(artifact, score)| RankedArtifact { artifact, score })
            .collect()
    } else {
        Vec::new()
    };

    let ranked = reciprocal_rank_fusion(vec![layer1, layer2, layer3], 60);
    let mut results = Vec::new();

    for ranked in ranked.into_iter().take(50) {
        let context = get_context(conn, segmenter, &ranked.artifact.id, context_min)?;
        let quests = crate::quest::get_quest_for_artifact(conn, &ranked.artifact.id)
            .ok()
            .filter(|q| !q.is_empty());

        results.push(SearchResult {
            artifact: ranked.artifact,
            score: ranked.score,
            context,
            quests,
        });
    }

    Ok(results)
}

fn search_bm25(
    conn: &Connection,
    fts_query: &str,
    date_from: Option<&str>,
    date_to: Option<&str>,
    source: Option<&str>,
    limit: i64,
) -> Result<Vec<RankedArtifact>> {
    let mut sql = String::from(
        r#"SELECT a.id, a.type, a.title, a.url, a.domain, a.created_at,
                  a.visited_at, a.is_bookmarked, a.visit_count, a.source,
                  a.content, a.user_note, a.folder_path, a.import_batch,
                  a.page_category, a.noise_score, a.extracted_query,
                  a.canonical_url, a.referrer_domain,
                  bm25(artifacts_fts) AS score
           FROM artifacts_fts fts
           JOIN artifacts a ON fts.rowid = a.rowid
           WHERE artifacts_fts MATCH ?"#,
    );

    let mut owned_params: Vec<Box<dyn ToSql>> = vec![Box::new(fts_query.to_string())];

    if let Some(df) = date_from {
        sql.push_str(" AND a.visited_at >= ?");
        owned_params.push(Box::new(df.to_string()));
    }
    if let Some(dt) = date_to {
        sql.push_str(" AND a.visited_at <= ?");
        owned_params.push(Box::new(dt.to_string()));
    }
    if let Some(s) = source {
        sql.push_str(" AND a.source = ?");
        owned_params.push(Box::new(s.to_string()));
    }
    sql.push_str(" ORDER BY score LIMIT ?");
    owned_params.push(Box::new(limit));

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn ToSql> = owned_params.iter().map(|p| p.as_ref()).collect();
    let mut rows = stmt.query(param_refs.as_slice())?;

    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        let artifact = row_to_artifact(row)?;
        let score: f64 = row.get(19)?;
        results.push(RankedArtifact { artifact, score });
    }

    Ok(results)
}

fn reciprocal_rank_fusion(ranked_lists: Vec<Vec<RankedArtifact>>, k: i64) -> Vec<RankedArtifact> {
    let mut scores: HashMap<String, (Artifact, f64)> = HashMap::new();

    for list in ranked_lists {
        for (idx, ranked) in list.into_iter().enumerate() {
            let rank = idx as f64 + 1.0;
            let rrf = 1.0 / (k as f64 + rank);
            let noise_multiplier = 1.0 - ranked.artifact.noise_score.clamp(0.0, 1.0) * 0.5;
            let adjusted = rrf * noise_multiplier;

            scores
                .entry(ranked.artifact.id.clone())
                .and_modify(|(_, score)| *score += adjusted)
                .or_insert((ranked.artifact, adjusted));
        }
    }

    let mut merged: Vec<RankedArtifact> = scores
        .into_iter()
        .map(|(_, (artifact, score))| RankedArtifact { artifact, score })
        .collect();
    merged.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    merged
}

// ─────────────────────────────────────────────────────────────────────────────
// Context: "what else were you browsing at the same time?"
// ─────────────────────────────────────────────────────────────────────────────

/// Retrieve artifacts accessed within ±window_minutes of the given artifact's
/// visited_at timestamp. This is the core memory-trigger feature.
pub fn get_context(
    conn: &Connection,
    segmenter: &Segmenter,
    artifact_id: &str,
    window_minutes: i64,
) -> Result<Vec<Artifact>> {
    let Some(target) = get_artifact(conn, artifact_id)? else {
        return Ok(vec![]);
    };

    let Some(ts) = target.visited_at.clone() else {
        return Ok(vec![]);
    };

    let mut stmt = conn.prepare(
        r#"SELECT id, type, title, url, domain, created_at, visited_at,
                  is_bookmarked, visit_count, source, content, user_note,
                  folder_path, import_batch, page_category, noise_score,
                  extracted_query, canonical_url, referrer_domain
           FROM artifacts
           WHERE id != ?1
             AND visited_at IS NOT NULL
             AND ABS(
                   (JULIANDAY(visited_at) - JULIANDAY(?2)) * 1440
                 ) <= ?3
           ORDER BY ABS(JULIANDAY(visited_at) - JULIANDAY(?2))
           LIMIT 40"#,
    )?;

    let artifact_iter = stmt.query_map(params![artifact_id, ts, window_minutes], |row| {
        row_to_artifact(row)
    })?;

    let mut candidates = Vec::new();
    for a in artifact_iter {
        candidates.push(a?);
    }

    let target_tokens = context_tokens(segmenter, &target);
    let mut scored: Vec<(Artifact, f64)> = candidates
        .into_iter()
        .map(|artifact| {
            let score = score_context_artifact(segmenter, &target_tokens, &artifact);
            (artifact, score)
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    Ok(scored
        .into_iter()
        .take(20)
        .map(|(artifact, _)| artifact)
        .collect())
}

fn get_artifact(conn: &Connection, artifact_id: &str) -> Result<Option<Artifact>> {
    let mut stmt = conn.prepare(
        r#"SELECT id, type, title, url, domain, created_at, visited_at,
                  is_bookmarked, visit_count, source, content, user_note,
                  folder_path, import_batch, page_category, noise_score,
                  extracted_query, canonical_url, referrer_domain
           FROM artifacts
           WHERE id = ?1
           LIMIT 1"#,
    )?;

    let mut rows = stmt.query(params![artifact_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_artifact(row)?))
    } else {
        Ok(None)
    }
}

fn context_tokens(segmenter: &Segmenter, artifact: &Artifact) -> Vec<String> {
    let text = [
        artifact.title.as_deref().unwrap_or(""),
        artifact.extracted_query.as_deref().unwrap_or(""),
        artifact.domain.as_deref().unwrap_or(""),
    ]
    .join(" ");
    segmenter.cut_for_search(&text)
}

fn score_context_artifact(
    segmenter: &Segmenter,
    target_tokens: &[String],
    artifact: &Artifact,
) -> f64 {
    let tokens = context_tokens(segmenter, artifact);
    let topic_sim = jaccard_similarity(target_tokens, &tokens);

    3.0 * topic_sim
        + 1.5 * if artifact.is_bookmarked { 1.0 } else { 0.0 }
        + 0.5 * (artifact.visit_count as f64).min(10.0) / 10.0
        + 1.0
            * if artifact.page_category.as_deref() == Some("search_query") {
                1.0
            } else {
                0.0
            }
        - 2.0 * artifact.noise_score.clamp(0.0, 1.0)
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
        "UPDATE artifacts SET user_note = ?1, embedding_version = 0 WHERE id = ?2",
        params![note, artifact_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_artifact(id: &str, noise_score: f64) -> Artifact {
        Artifact {
            id: id.to_string(),
            r#type: "history".to_string(),
            title: Some(id.to_string()),
            url: Some(format!("https://example.com/{}", id)),
            domain: Some("example.com".to_string()),
            created_at: "2026-01-01T00:00:00".to_string(),
            visited_at: Some("2026-01-01T00:00:00".to_string()),
            is_bookmarked: false,
            visit_count: 1,
            source: Some("edge".to_string()),
            content: None,
            user_note: None,
            folder_path: None,
            import_batch: None,
            page_category: Some("content".to_string()),
            noise_score,
            extracted_query: None,
            canonical_url: None,
            referrer_domain: None,
        }
    }

    #[test]
    fn test_rrf_merges_duplicate_results() {
        let a = RankedArtifact {
            artifact: test_artifact("a", 0.0),
            score: -1.0,
        };
        let b = RankedArtifact {
            artifact: test_artifact("b", 0.0),
            score: -2.0,
        };

        let merged = reciprocal_rank_fusion(vec![vec![a.clone(), b], vec![a]], 60);

        assert_eq!(merged[0].artifact.id, "a");
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn test_rrf_downweights_noise() {
        let noisy = RankedArtifact {
            artifact: test_artifact("noisy", 1.0),
            score: -1.0,
        };
        let clean = RankedArtifact {
            artifact: test_artifact("clean", 0.0),
            score: -1.0,
        };

        let merged = reciprocal_rank_fusion(vec![vec![noisy, clean]], 60);

        assert_eq!(merged[0].artifact.id, "clean");
    }
}
