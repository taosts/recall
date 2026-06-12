// ═══════════════════════════════════════════════════════════════════════════════
// quest.rs — Quest (探索任务) System
// ═══════════════════════════════════════════════════════════════════════════════

use crate::models::{Artifact, Quest, QuestSummary};
use crate::search::row_to_artifact;
use crate::segmenter::{jaccard_similarity, Segmenter};
use rusqlite::{params, Connection, Result};
use std::collections::HashMap;
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// Time gap (minutes) between consecutive artifacts that triggers a Quest boundary.
const GAP_THRESHOLD_MINUTES: i64 = 45;

/// Minimum number of artifacts required to form a Quest.
const MIN_ARTIFACTS_PER_QUEST: usize = 3;

/// Maximum Quest duration in hours before force-splitting.
const MAX_QUEST_DURATION_HOURS: i64 = 8;

// ─────────────────────────────────────────────────────────────────────────────
// 1. generate_quests — Main clustering algorithm
// ─────────────────────────────────────────────────────────────────────────────

pub fn generate_quests(conn: &Connection, segmenter: &Segmenter) -> Result<usize> {
    // Option A: Delete all auto-generated Quests and regenerate
    conn.execute("DELETE FROM quests WHERE status = 'auto'", [])?;

    // Query all artifacts with visited_at, ordered by time ASC
    let mut stmt = conn.prepare(
        r#"SELECT id, type, title, url, domain, created_at, visited_at,
                  is_bookmarked, visit_count, source, content, user_note,
                  folder_path, import_batch, page_category, noise_score,
                  extracted_query, canonical_url, referrer_domain
           FROM artifacts
           WHERE visited_at IS NOT NULL
             AND COALESCE(noise_score, 0.0) <= 0.7
             AND COALESCE(page_category, 'content') != 'utility'
           ORDER BY visited_at ASC"#,
    )?;

    let artifact_iter = stmt.query_map([], |row| row_to_artifact(row))?;

    let mut artifacts: Vec<Artifact> = Vec::new();
    for a in artifact_iter {
        artifacts.push(a?);
    }
    // Release the statement borrow so conn is free for gap calculations
    drop(stmt);

    if artifacts.is_empty() {
        return Ok(0);
    }

    // Cluster artifacts by time gaps
    let mut clusters: Vec<Vec<&Artifact>> = Vec::new();
    let mut current_cluster: Vec<&Artifact> = Vec::new();

    for i in 0..artifacts.len() {
        let a = &artifacts[i];
        if current_cluster.is_empty() {
            current_cluster.push(a);
            continue;
        }

        let prev = current_cluster.last().unwrap();
        let gap_minutes = calc_gap_minutes(conn, &prev.visited_at, &a.visited_at)?;

        // Check if we should split: time gap too large, or cluster duration too long
        let cluster_start = current_cluster.first().unwrap().visited_at.as_deref();
        let cluster_duration = match cluster_start {
            Some(start) => calc_gap_minutes(conn, &Some(start.to_string()), &a.visited_at)?,
            None => 0,
        };

        let force_split = cluster_duration > MAX_QUEST_DURATION_HOURS * 60;

        let intent_split = should_split_by_intent(segmenter, &current_cluster, a);

        if gap_minutes > GAP_THRESHOLD_MINUTES || force_split || intent_split {
            // Close current cluster, start a new one
            clusters.push(std::mem::take(&mut current_cluster));
        }
        current_cluster.push(a);
    }

    // Don't forget the last cluster
    if !current_cluster.is_empty() {
        clusters.push(current_cluster);
    }

    // Filter: discard clusters with fewer than MIN_ARTIFACTS_PER_QUEST
    let valid_clusters: Vec<&Vec<&Artifact>> = clusters
        .iter()
        .filter(|c| c.len() >= MIN_ARTIFACTS_PER_QUEST && c.iter().any(|a| is_anchor(a)))
        .collect();

    let now = chrono_now();
    let mut created = 0usize;

    for cluster in &valid_clusters {
        let quest_id = Uuid::new_v4().to_string();
        let started_at = cluster.first().and_then(|a| a.visited_at.clone());
        let ended_at = cluster.last().and_then(|a| a.visited_at.clone());

        // Generate auto_name (pass cluster artifact_ids for domain/keyword extraction)
        let auto_name = auto_name_quest(conn, segmenter, cluster)?;

        conn.execute(
            r#"INSERT INTO quests (id, auto_name, started_at, ended_at, status, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, 'auto', ?5, ?5)"#,
            params![quest_id, auto_name, started_at, ended_at, now],
        )?;

        // Insert artifact associations
        for a in *cluster {
            conn.execute(
                r#"INSERT OR IGNORE INTO quest_artifacts (quest_id, artifact_id, added_at, is_anchor)
                   VALUES (?1, ?2, ?3, ?4)"#,
                params![quest_id, a.id, now, if is_anchor(a) { 1 } else { 0 }],
            )?;
        }

        created += 1;
    }

    Ok(created)
}

fn should_split_by_intent(
    segmenter: &Segmenter,
    current_cluster: &[&Artifact],
    current: &Artifact,
) -> bool {
    if current.page_category.as_deref() != Some("search_query") {
        return false;
    }

    let Some(prev) = current_cluster
        .iter()
        .rev()
        .find(|a| a.page_category.as_deref() == Some("search_query"))
    else {
        return false;
    };

    let prev_text = quest_intent_text(prev);
    let current_text = quest_intent_text(current);
    let prev_tokens = segmenter.cut_for_search(&prev_text);
    let current_tokens = segmenter.cut_for_search(&current_text);

    jaccard_similarity(&prev_tokens, &current_tokens) < 0.1
}

fn quest_intent_text(artifact: &Artifact) -> String {
    artifact
        .extracted_query
        .as_deref()
        .or(artifact.title.as_deref())
        .unwrap_or("")
        .to_string()
}

fn is_anchor(artifact: &Artifact) -> bool {
    artifact.page_category.as_deref() == Some("search_query")
        || artifact.is_bookmarked
        || artifact.visit_count >= 3
}

/// Calculate time gap in minutes between two ISO 8601 timestamps using SQLite JULIANDAY.
fn calc_gap_minutes(conn: &Connection, ts1: &Option<String>, ts2: &Option<String>) -> Result<i64> {
    let (Some(t1), Some(t2)) = (ts1, ts2) else {
        return Ok(i64::MAX); // large gap if either timestamp is missing
    };
    let gap: f64 = conn.query_row(
        "SELECT ABS((JULIANDAY(?1) - JULIANDAY(?2)) * 1440)",
        params![t1, t2],
        |row| row.get(0),
    )?;
    Ok(gap as i64)
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. auto_name_quest — Generate human-readable Quest name
// ─────────────────────────────────────────────────────────────────────────────

fn auto_name_quest(
    conn: &Connection,
    segmenter: &Segmenter,
    cluster: &[&Artifact],
) -> Result<String> {
    // Collect artifact IDs
    let ids: Vec<&str> = cluster.iter().map(|a| a.id.as_str()).collect();
    if ids.is_empty() {
        return Ok("Unknown Quest".to_string());
    }

    if let Some(query) = cluster
        .iter()
        .find(|a| a.page_category.as_deref() == Some("search_query"))
        .and_then(|a| a.extracted_query.as_deref())
    {
        let compact = compact_query_name(segmenter, query);
        if !compact.is_empty() {
            return Ok(compact);
        }
    }

    // Get top 2-3 most frequent non-generic domains
    let placeholders: Vec<String> = ids
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect();
    let sql = format!(
        r#"SELECT domain, COUNT(*) as cnt
           FROM artifacts
           WHERE id IN ({})
             AND domain IS NOT NULL
             AND domain NOT IN ('google.com', 'bing.com', 'baidu.com', 'youtube.com', 'facebook.com', 'twitter.com', 'x.com')
           GROUP BY domain
           ORDER BY cnt DESC
           LIMIT 3"#,
        placeholders.join(",")
    );

    let mut stmt = conn.prepare(&sql)?;
    let domain_rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), |row| {
        let domain: String = row.get(0)?;
        let count: i64 = row.get(1)?;
        Ok((domain, count))
    })?;

    let mut domains: Vec<String> = Vec::new();
    for dr in domain_rows {
        let (d, _) = dr?;
        // Extract short name from domain (e.g. "reddit.com" -> "reddit")
        let short = d.split('.').next().unwrap_or(&d).to_string();
        domains.push(short);
    }

    // Get titles of anchor (bookmarked) artifacts
    let anchor_ids: Vec<&str> = cluster
        .iter()
        .filter(|a| a.is_bookmarked)
        .map(|a| a.id.as_str())
        .collect();

    let mut keyword = String::new();
    if !anchor_ids.is_empty() {
        let a_placeholders: Vec<String> = anchor_ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();
        let a_sql = format!(
            r#"SELECT title FROM artifacts WHERE id IN ({}) AND title IS NOT NULL LIMIT 1"#,
            a_placeholders.join(",")
        );
        let mut a_stmt = conn.prepare(&a_sql)?;
        let title: Option<String> = a_stmt
            .query_row(rusqlite::params_from_iter(anchor_ids.iter()), |row| {
                row.get(0)
            })
            .ok()
            .flatten();

        if let Some(t) = title {
            // Extract a meaningful word from title
            keyword = extract_keyword(&t);
        }
    }

    // If no bookmark keyword, try any artifact title
    if keyword.is_empty() {
        for a in cluster.iter().take(5) {
            if let Some(ref t) = a.title {
                keyword = extract_keyword(t);
                if !keyword.is_empty() {
                    break;
                }
            }
        }
    }

    // Build name
    let name = match (domains.is_empty(), keyword.is_empty()) {
        (true, true) => {
            // Fallback to date range
            let start = cluster
                .first()
                .and_then(|a| a.visited_at.as_deref())
                .unwrap_or("?");
            let start_short = &start[..start.len().min(10)]; // "YYYY-MM-DD"
            format!("{} browsing", start_short)
        }
        (_, true) => domains.join(" + "),
        (true, _) => keyword,
        (false, false) => format!("{}: {}", domains.join(" + "), keyword),
    };

    Ok(name)
}

fn compact_query_name(segmenter: &Segmenter, query: &str) -> String {
    let keywords = segmenter.extract_keywords(query, 4);
    if keywords.is_empty() {
        query.trim().to_string()
    } else {
        keywords.join(" ")
    }
}

/// Extract a meaningful keyword from a title string.
fn extract_keyword(title: &str) -> String {
    // Split by common delimiters and find the longest non-noise word
    let noise: &[&str] = &[
        "the", "a", "an", "is", "of", "to", "in", "for", "on", "and", "or", "with", "how", "what",
        "why", "guide", "tutorial", "howto",
    ];

    let mut best = String::new();
    for part in title.split(|c: char| !c.is_alphanumeric()) {
        let lower = part.to_lowercase();
        if part.len() >= 3 && !noise.contains(&lower.as_str()) && part.len() > best.len() {
            best = part.to_string();
        }
    }
    best
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. list_quests — Paginated Quest list
// ─────────────────────────────────────────────────────────────────────────────

pub fn list_quests(conn: &Connection, limit: i64, offset: i64) -> Result<Vec<QuestSummary>> {
    let mut stmt = conn.prepare(
        r#"SELECT q.id,
                  COALESCE(q.name, q.auto_name) AS display_name,
                  q.started_at,
                  q.ended_at,
                  q.status,
                  COUNT(qa.artifact_id) AS artifact_count,
                  SUM(qa.is_anchor) AS anchor_count
           FROM quests q
           LEFT JOIN quest_artifacts qa ON q.id = qa.quest_id
           WHERE q.status != 'archived'
           GROUP BY q.id
           ORDER BY q.started_at DESC
           LIMIT ?1 OFFSET ?2"#,
    )?;

    let rows = stmt.query_map(params![limit, offset], |row| {
        Ok(QuestSummary {
            id: row.get(0)?,
            display_name: row.get::<_, String>(1).unwrap_or_default(),
            started_at: row.get(2)?,
            ended_at: row.get(3)?,
            status: row.get::<_, String>(4).unwrap_or_default(),
            artifact_count: row.get::<_, i64>(5).unwrap_or(0),
            anchor_count: row.get::<_, i64>(6).unwrap_or(0),
        })
    })?;

    let mut results = Vec::new();
    for r in rows {
        results.push(r?);
    }
    Ok(results)
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. get_quest — Full Quest with artifact list
// ─────────────────────────────────────────────────────────────────────────────

pub fn get_quest(conn: &Connection, quest_id: &str) -> Result<Quest> {
    // Fetch quest header
    let (id, name, auto_name, started_at, ended_at, status, created_at, updated_at) = conn
        .query_row(
            r#"SELECT id, name, auto_name, started_at, ended_at, status, created_at, updated_at
           FROM quests WHERE id = ?1"#,
            params![quest_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )?;

    // Fetch artifacts in timeline order
    let mut stmt = conn.prepare(
        r#"SELECT a.id, a.type, a.title, a.url, a.domain, a.created_at, a.visited_at,
                  a.is_bookmarked, a.visit_count, a.source, a.content, a.user_note,
                  a.folder_path, a.import_batch, a.page_category, a.noise_score,
                  a.extracted_query, a.canonical_url, a.referrer_domain
           FROM artifacts a
           JOIN quest_artifacts qa ON a.id = qa.artifact_id
           WHERE qa.quest_id = ?1
           ORDER BY a.visited_at ASC"#,
    )?;

    let artifact_iter = stmt.query_map(params![quest_id], |row| row_to_artifact(row))?;
    let mut artifacts = Vec::new();
    for a in artifact_iter {
        artifacts.push(a?);
    }

    let origin_query = artifacts
        .iter()
        .find(|a| a.page_category.as_deref() == Some("search_query"))
        .and_then(|a| a.extracted_query.clone());

    let anchor_ids: Vec<String> = artifacts
        .iter()
        .filter(|a| {
            a.is_bookmarked
                || a.visit_count >= 3
                || a.page_category.as_deref() == Some("search_query")
        })
        .map(|a| a.id.clone())
        .collect();

    let mut domain_counts: HashMap<String, i64> = HashMap::new();
    for artifact in &artifacts {
        if let Some(domain) = artifact.domain.as_ref() {
            *domain_counts.entry(domain.clone()).or_insert(0) += artifact.visit_count.max(1);
        }
    }
    let mut top_domains: Vec<(String, i64)> = domain_counts.into_iter().collect();
    top_domains.sort_by(|a, b| b.1.cmp(&a.1));
    top_domains.truncate(5);

    let noise_count = artifacts
        .iter()
        .filter(|artifact| artifact.noise_score > 0.5)
        .count() as i64;

    Ok(Quest {
        id,
        name,
        auto_name,
        started_at,
        ended_at,
        status,
        created_at,
        updated_at,
        artifacts,
        origin_query,
        anchor_ids,
        top_domains,
        noise_count,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. rename_quest — User renames a Quest
// ─────────────────────────────────────────────────────────────────────────────

pub fn rename_quest(conn: &Connection, quest_id: &str, name: &str) -> Result<()> {
    let now = chrono_now();
    conn.execute(
        "UPDATE quests SET name = ?1, status = 'confirmed', updated_at = ?2 WHERE id = ?3",
        params![name, now, quest_id],
    )?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. merge_quests — Merge multiple Quests into one
// ─────────────────────────────────────────────────────────────────────────────

pub fn merge_quests(conn: &Connection, quest_ids: Vec<String>) -> Result<String> {
    if quest_ids.len() < 2 {
        return Err(rusqlite::Error::InvalidParameterName(
            "need at least 2 quest IDs".into(),
        ));
    }

    // Find the survivor: the Quest with the earliest started_at
    let placeholders: Vec<String> = quest_ids
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect();
    let sql = format!(
        r#"SELECT id FROM quests WHERE id IN ({}) ORDER BY started_at ASC LIMIT 1"#,
        placeholders.join(",")
    );

    let survivor: String =
        conn.query_row(&sql, rusqlite::params_from_iter(quest_ids.iter()), |row| {
            row.get(0)
        })?;

    // Collect absorbed IDs (all except survivor)
    let absorbed: Vec<&str> = quest_ids
        .iter()
        .filter(|id| **id != survivor)
        .map(|s| s.as_str())
        .collect();

    // Move artifact associations to survivor
    let abs_placeholders: Vec<String> = absorbed
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect();
    let update_sql = format!(
        r#"UPDATE quest_artifacts SET quest_id = ?{} WHERE quest_id IN ({})"#,
        absorbed.len() + 1,
        abs_placeholders.join(",")
    );

    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    for a in &absorbed {
        all_params.push(Box::new(a.to_string()));
    }
    all_params.push(Box::new(survivor.clone()));
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        all_params.iter().map(|p| p.as_ref()).collect();
    conn.execute(&update_sql, param_refs.as_slice())?;

    // Delete absorbed Quests
    let del_sql = format!(
        r#"DELETE FROM quests WHERE id IN ({})"#,
        abs_placeholders.join(",")
    );
    let abs_params: Vec<&dyn rusqlite::types::ToSql> = absorbed
        .iter()
        .map(|s| s as &dyn rusqlite::types::ToSql)
        .collect();
    conn.execute(&del_sql, abs_params.as_slice())?;

    // Recalculate started_at / ended_at for survivor
    let now = chrono_now();
    conn.execute(
        r#"UPDATE quests SET
               started_at = (SELECT MIN(a.visited_at) FROM artifacts a
                             JOIN quest_artifacts qa ON a.id = qa.artifact_id
                             WHERE qa.quest_id = ?1),
               ended_at   = (SELECT MAX(a.visited_at) FROM artifacts a
                             JOIN quest_artifacts qa ON a.id = qa.artifact_id
                             WHERE qa.quest_id = ?1),
               updated_at = ?2
           WHERE id = ?1"#,
        params![survivor, now],
    )?;

    Ok(survivor)
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. archive_quest — Soft-delete a Quest
// ─────────────────────────────────────────────────────────────────────────────

pub fn archive_quest(conn: &Connection, quest_id: &str) -> Result<()> {
    let now = chrono_now();
    conn.execute(
        "UPDATE quests SET status = 'archived', updated_at = ?1 WHERE id = ?2",
        params![now, quest_id],
    )?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// 8. get_quest_for_artifact — Find Quests containing an artifact
// ─────────────────────────────────────────────────────────────────────────────

pub fn get_quest_for_artifact(conn: &Connection, artifact_id: &str) -> Result<Vec<QuestSummary>> {
    let mut stmt = conn.prepare(
        r#"SELECT q.id,
                  COALESCE(q.name, q.auto_name) AS display_name,
                  q.started_at,
                  q.ended_at,
                  q.status,
                  (SELECT COUNT(*) FROM quest_artifacts qa2 WHERE qa2.quest_id = q.id) AS artifact_count,
                  (SELECT SUM(is_anchor) FROM quest_artifacts qa3 WHERE qa3.quest_id = q.id) AS anchor_count
           FROM quests q
           JOIN quest_artifacts qa ON q.id = qa.quest_id
           WHERE qa.artifact_id = ?1
             AND q.status != 'archived'"#,
    )?;

    let rows = stmt.query_map(params![artifact_id], |row| {
        Ok(QuestSummary {
            id: row.get(0)?,
            display_name: row.get::<_, String>(1).unwrap_or_default(),
            started_at: row.get(2)?,
            ended_at: row.get(3)?,
            status: row.get::<_, String>(4).unwrap_or_default(),
            artifact_count: row.get::<_, i64>(5).unwrap_or(0),
            anchor_count: row.get::<_, i64>(6).unwrap_or(0),
        })
    })?;

    let mut results = Vec::new();
    for r in rows {
        results.push(r?);
    }
    Ok(results)
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Get current time as ISO 8601 string.
fn chrono_now() -> String {
    // Use a simple manual format to avoid adding chrono dependency
    // SQLite accepts ISO 8601 format
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    let days_since_epoch = secs / 86400;
    let secs_of_day = secs % 86400;
    let hours = secs_of_day / 3600;
    let mins = (secs_of_day % 3600) / 60;
    let secs_rem = secs_of_day % 60;

    // Calculate year, month, day from days since epoch
    let (year, month, day) = days_to_date(days_since_epoch as i64);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        year, month, day, hours, mins, secs_rem
    )
}

/// Simple date conversion: days since Unix epoch to (year, month, day).
fn days_to_date(days: i64) -> (i64, u32, u32) {
    // Algorithm from Howard Hinnant
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Unit tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_keyword ─────────────────────────────────────────────────

    #[test]
    fn test_extract_keyword_basic() {
        // "ZFS" (3), "Cache" (5), "Tuning" (6), "Guide" (noise)
        assert_eq!(extract_keyword("ZFS Cache Tuning Guide"), "Tuning");
    }

    #[test]
    fn test_extract_keyword_all_noise() {
        // Every token is either a noise word or < 3 chars → empty
        assert_eq!(extract_keyword("The Guide To How"), "");
    }

    #[test]
    fn test_extract_keyword_empty() {
        assert_eq!(extract_keyword(""), "");
    }

    #[test]
    fn test_extract_keyword_with_symbols() {
        // Splitting "OpenWrt - DNS Configuration (v2)" on non-alphanumeric gives:
        //   "OpenWrt" (7), "DNS" (3), "Configuration" (13), "v2" (2)
        // "Configuration" is the longest valid token.
        assert_eq!(
            extract_keyword("OpenWrt - DNS Configuration (v2)"),
            "Configuration"
        );
    }

    #[test]
    fn test_extract_keyword_chinese() {
        // CJK characters are alphanumeric per Rust's `char::is_alphanumeric`,
        // so the whole run "ZFS缓存调优指南" stays as one token (len > 3 bytes).
        let result = extract_keyword("ZFS缓存调优指南");
        assert!(!result.is_empty(), "expected a non-empty keyword");
    }

    #[test]
    fn test_extract_keyword_single_long_word() {
        assert_eq!(extract_keyword("stackoverflow"), "stackoverflow");
    }

    // ── days_to_date ────────────────────────────────────────────────────

    #[test]
    fn test_days_to_date_unix_epoch() {
        assert_eq!(days_to_date(0), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_date_known_date() {
        // 2024-12-03 is day 20060 since 1970-01-01 (20059 = Dec 2)
        assert_eq!(days_to_date(20060), (2024, 12, 3));
    }

    #[test]
    fn test_days_to_date_leap_year() {
        // 2000-02-29 is day 11016 since epoch
        // 1970-01-01 + 11016 days = 2000-02-29
        assert_eq!(days_to_date(11016), (2000, 2, 29));
    }

    #[test]
    fn test_days_to_date_y2k() {
        // 2000-01-01 is day 10957 since epoch
        assert_eq!(days_to_date(10957), (2000, 1, 1));
    }

    // ── chrono_now ──────────────────────────────────────────────────────

    #[test]
    fn test_chrono_now_format() {
        let now = chrono_now();
        // Must match ISO 8601 without timezone: YYYY-MM-DDTHH:MM:SS
        let re = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}$").unwrap();
        assert!(
            re.is_match(&now),
            "chrono_now() returned '{}' which doesn't match ISO 8601",
            now
        );
    }

    #[test]
    fn test_chrono_now_is_recent() {
        let now = chrono_now();
        let year: i32 = now[..4].parse().expect("year should be numeric");
        assert!(
            year >= 2025 && year <= 2030,
            "expected year in 2025..=2030, got {}",
            year
        );
    }
}
