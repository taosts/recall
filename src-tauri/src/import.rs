use chrono::{DateTime, TimeZone, Utc};
use rusqlite::Connection;
use serde_json::Value;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::db::{find_by_url, upsert_artifact};
use crate::models::{Artifact, BrowserInfo, ImportStats};

// ─────────────────────────────────────────────────────────────────────────────
// Browser path resolution
// ─────────────────────────────────────────────────────────────────────────────

fn local_app_data() -> Option<PathBuf> {
    dirs::data_local_dir()
}

fn browser_paths(browser_id: &str) -> Option<(PathBuf, PathBuf)> {
    let base = local_app_data()?;
    let profile = match browser_id {
        "edge" => base
            .join("Microsoft")
            .join("Edge")
            .join("User Data")
            .join("Default"),
        "chrome" => base
            .join("Google")
            .join("Chrome")
            .join("User Data")
            .join("Default"),
        _ => return None,
    };
    let bookmarks = profile.join("Bookmarks");
    let history = profile.join("History");
    Some((bookmarks, history))
}

/// Detect which browsers are installed and return their path info.
pub fn detect_browsers() -> Vec<BrowserInfo> {
    ["edge", "chrome"]
        .iter()
        .filter_map(|&id| {
            let (bk, hi) = browser_paths(id)?;
            Some(BrowserInfo {
                id: id.to_string(),
                name: match id {
                    "edge" => "Microsoft Edge".to_string(),
                    "chrome" => "Google Chrome".to_string(),
                    _ => id.to_string(),
                },
                bookmarks_path: bk.to_string_lossy().into_owned(),
                history_path: hi.to_string_lossy().into_owned(),
                available: bk.exists() || hi.exists(),
            })
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Timestamp conversion
// Chrome/Edge use microseconds since 1601-01-01 (Windows FILETIME epoch)
// ─────────────────────────────────────────────────────────────────────────────

const FILETIME_TO_UNIX_OFFSET: i64 = 11_644_473_600;

fn filetime_micros_to_iso(micros: i64) -> Option<String> {
    let unix_secs = micros / 1_000_000 - FILETIME_TO_UNIX_OFFSET;
    let unix_nanos = ((micros % 1_000_000) * 1000) as u32;
    let dt: DateTime<Utc> = Utc.timestamp_opt(unix_secs, unix_nanos).single()?;
    Some(dt.to_rfc3339())
}

fn extract_domain(url: &str) -> Option<String> {
    // Naive but fast domain extractor — no external dep needed
    let after_scheme = url.split("://").nth(1)?;
    let host = after_scheme.split('/').next()?;
    // Strip "www."
    let host = host.strip_prefix("www.").unwrap_or(host);
    // Strip port
    let host = host.split(':').next()?;
    Some(host.to_lowercase())
}

// ─────────────────────────────────────────────────────────────────────────────
// Bookmarks import
// ─────────────────────────────────────────────────────────────────────────────

/// Import bookmarks from a Chrome/Edge Bookmarks JSON file.
pub fn import_bookmarks(
    browser: &str,
    conn: &Connection,
    batch: &str,
) -> Result<ImportStats, String> {
    let (bk_path, _) =
        browser_paths(browser).ok_or_else(|| format!("Unknown browser: {}", browser))?;

    if !bk_path.exists() {
        return Err(format!("Bookmarks file not found: {}", bk_path.display()));
    }

    let raw = std::fs::read_to_string(&bk_path)
        .map_err(|e| format!("Failed to read bookmarks: {}", e))?;
    let json: Value =
        serde_json::from_str(&raw).map_err(|e| format!("Failed to parse bookmarks JSON: {}", e))?;

    let mut stats = ImportStats {
        browser: browser.to_string(),
        bookmarks_imported: 0,
        history_imported: 0,
        duplicates_skipped: 0,
        errors: vec![],
    };

    let roots = &json["roots"];
    for root_key in ["bookmark_bar", "other", "synced"] {
        if let Some(root) = roots.get(root_key) {
            walk_bookmark_node(root, "", browser, batch, conn, &mut stats);
        }
    }

    Ok(stats)
}

fn walk_bookmark_node(
    node: &Value,
    folder_path: &str,
    browser: &str,
    batch: &str,
    conn: &Connection,
    stats: &mut ImportStats,
) {
    let node_type = node["type"].as_str().unwrap_or("");

    match node_type {
        "url" => {
            let url = node["url"].as_str().unwrap_or("").to_string();
            let title = node["name"].as_str().map(|s| s.to_string());

            if url.is_empty() || (!url.starts_with("http://") && !url.starts_with("https://")) {
                return;
            }

            let date_added = node["date_added"]
                .as_str()
                .and_then(|s| s.parse::<i64>().ok())
                .and_then(filetime_micros_to_iso);
            let date_last_used = node["date_last_used"]
                .as_str()
                .and_then(|s| s.parse::<i64>().ok())
                .filter(|&v| v > 0)
                .and_then(filetime_micros_to_iso);

            let now = Utc::now().to_rfc3339();
            let artifact = Artifact {
                id: Uuid::new_v4().to_string(),
                r#type: "bookmark".to_string(),
                title,
                domain: extract_domain(&url),
                url: Some(url.clone()),
                created_at: date_added.clone().unwrap_or_else(|| now.clone()),
                visited_at: date_last_used.or(date_added),
                is_bookmarked: true,
                visit_count: 0,
                source: Some(browser.to_string()),
                content: None,
                user_note: None,
                folder_path: if folder_path.is_empty() {
                    None
                } else {
                    Some(folder_path.to_string())
                },
                import_batch: Some(batch.to_string()),
            };

            // Deduplicate by URL
            match find_by_url(conn, &url) {
                Ok(Some(_existing_id)) => {
                    // URL exists — still want to mark it as bookmarked
                    let _ = conn.execute(
                        "UPDATE artifacts SET is_bookmarked=1, folder_path=COALESCE(?1, folder_path) WHERE url=?2",
                        rusqlite::params![artifact.folder_path, url],
                    );
                    stats.duplicates_skipped += 1;
                }
                Ok(None) => match upsert_artifact(conn, &artifact) {
                    Ok(_) => stats.bookmarks_imported += 1,
                    Err(e) => stats.errors.push(format!("Insert error: {}", e)),
                },
                Err(e) => stats.errors.push(format!("DB error: {}", e)),
            }
        }
        "folder" => {
            let name = node["name"].as_str().unwrap_or("Unnamed");
            let new_path = if folder_path.is_empty() {
                name.to_string()
            } else {
                format!("{}/{}", folder_path, name)
            };
            if let Some(children) = node["children"].as_array() {
                for child in children {
                    walk_bookmark_node(child, &new_path, browser, batch, conn, stats);
                }
            }
        }
        _ => {}
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// History import
// ─────────────────────────────────────────────────────────────────────────────

/// Import browsing history from a Chrome/Edge History SQLite file.
/// Copies the file to a temp location first to avoid locking issues.
pub fn import_history(
    browser: &str,
    conn: &Connection,
    batch: &str,
    limit_days: Option<i64>,
) -> Result<ImportStats, String> {
    let (_, hi_path) =
        browser_paths(browser).ok_or_else(|| format!("Unknown browser: {}", browser))?;

    if !hi_path.exists() {
        return Err(format!("History file not found: {}", hi_path.display()));
    }

    // Copy to temp to avoid SQLite lock from running browser
    let temp_path = std::env::temp_dir().join(format!("recall_{}_history_copy", browser));
    std::fs::copy(&hi_path, &temp_path)
        .map_err(|e| format!("Failed to copy History file: {}", e))?;

    let result = read_history_from_copy(&temp_path, browser, conn, batch, limit_days);

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_path);

    result
}

fn read_history_from_copy(
    copy_path: &Path,
    browser: &str,
    conn: &Connection,
    batch: &str,
    limit_days: Option<i64>,
) -> Result<ImportStats, String> {
    use rusqlite::{Connection as RConn, OpenFlags};

    let src = RConn::open_with_flags(copy_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("Failed to open History copy: {}", e))?;

    let mut stats = ImportStats {
        browser: browser.to_string(),
        bookmarks_imported: 0,
        history_imported: 0,
        duplicates_skipped: 0,
        errors: vec![],
    };

    // Calculate time cutoff if limit_days provided
    // Chrome timestamps: microseconds since 1601-01-01
    let cutoff: i64 = if let Some(days) = limit_days {
        let unix_cutoff = Utc::now().timestamp() - days * 86400;
        (unix_cutoff + FILETIME_TO_UNIX_OFFSET) * 1_000_000
    } else {
        0
    };

    let sql = if cutoff > 0 {
        format!(
            r#"SELECT u.url, u.title, u.visit_count, u.last_visit_time
               FROM urls u
               WHERE u.hidden = 0 AND u.last_visit_time > {}
               ORDER BY u.last_visit_time DESC"#,
            cutoff
        )
    } else {
        r#"SELECT u.url, u.title, u.visit_count, u.last_visit_time
           FROM urls u
           WHERE u.hidden = 0
           ORDER BY u.last_visit_time DESC"#
            .to_string()
    };

    let mut stmt = src
        .prepare(&sql)
        .map_err(|e| format!("Failed to prepare History query: {}", e))?;

    let rows_result: Result<Vec<(String, Option<String>, i64, i64)>, _> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .and_then(|it| it.collect());

    let rows = rows_result.map_err(|e| format!("Failed to read History rows: {}", e))?;

    let now = Utc::now().to_rfc3339();

    for (url, title, visit_count, last_visit_time) in rows {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            continue;
        }

        let visited_at = filetime_micros_to_iso(last_visit_time);
        let domain = extract_domain(&url);

        let artifact = Artifact {
            id: Uuid::new_v4().to_string(),
            r#type: "history".to_string(),
            title,
            domain,
            url: Some(url.clone()),
            created_at: now.clone(),
            visited_at,
            is_bookmarked: false,
            visit_count,
            source: Some(browser.to_string()),
            content: None,
            user_note: None,
            folder_path: None,
            import_batch: Some(batch.to_string()),
        };

        match find_by_url(conn, &url) {
            Ok(Some(_)) => {
                stats.duplicates_skipped += 1;
            }
            Ok(None) => match upsert_artifact(conn, &artifact) {
                Ok(_) => stats.history_imported += 1,
                Err(e) => stats
                    .errors
                    .push(format!("Insert error for {}: {}", url, e)),
            },
            Err(e) => stats.errors.push(format!("DB lookup error: {}", e)),
        }
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_domain ──────────────────────────────────────────────────

    #[test]
    fn test_domain_simple_https() {
        assert_eq!(
            extract_domain("https://example.com/path"),
            Some("example.com".into())
        );
    }

    #[test]
    fn test_domain_http() {
        assert_eq!(
            extract_domain("http://example.com"),
            Some("example.com".into())
        );
    }

    #[test]
    fn test_domain_strips_www() {
        assert_eq!(
            extract_domain("https://www.example.com/x"),
            Some("example.com".into())
        );
    }

    #[test]
    fn test_domain_strips_port() {
        assert_eq!(
            extract_domain("https://localhost:8080/api"),
            Some("localhost".into())
        );
    }

    #[test]
    fn test_domain_subdomain() {
        assert_eq!(
            extract_domain("https://docs.rust-lang.org/book"),
            Some("docs.rust-lang.org".into())
        );
    }

    #[test]
    fn test_domain_no_scheme() {
        // No "://" means split("://").nth(1) returns None
        assert_eq!(extract_domain("example.com/path"), None);
    }

    #[test]
    fn test_domain_empty() {
        assert_eq!(extract_domain(""), None);
    }

    #[test]
    fn test_domain_lowercase() {
        assert_eq!(
            extract_domain("https://EXAMPLE.COM"),
            Some("example.com".into())
        );
    }

    // ── filetime_micros_to_iso ──────────────────────────────────────────

    #[test]
    fn test_filetime_known_value() {
        // 2024-01-01T00:00:00 UTC
        // chrome_micros = (unix_secs + FILETIME_TO_UNIX_OFFSET) * 1_000_000
        let micros: i64 = (1_704_067_200_i64 + FILETIME_TO_UNIX_OFFSET) * 1_000_000;
        let iso = filetime_micros_to_iso(micros).expect("should parse");
        assert!(
            iso.contains("2024-01-01"),
            "expected 2024-01-01, got {}",
            iso
        );
    }

    #[test]
    fn test_filetime_zero() {
        // 0 microseconds → 1601-01-01T00:00:00 UTC (Windows FILETIME epoch)
        let iso = filetime_micros_to_iso(0).expect("should parse epoch 1601");
        assert!(
            iso.contains("1601-01-01"),
            "expected 1601-01-01, got {}",
            iso
        );
    }

    #[test]
    fn test_filetime_negative() {
        // A very large negative value should fail timestamp_opt → None
        assert_eq!(filetime_micros_to_iso(i64::MIN), None);
    }

    #[test]
    fn test_filetime_recent() {
        // Known Chrome timestamp: 13_348_044_000_000_000
        // unix_secs = 13_348_044_000 - 11_644_473_600 = 1_703_570_400
        // ≈ 2023-12-26T06:00:00 UTC
        let micros: i64 = 13_348_044_000_000_000;
        let iso = filetime_micros_to_iso(micros).expect("should parse");
        assert!(iso.starts_with("2023-"), "expected 2023-xx-xx, got {}", iso);
    }
}
