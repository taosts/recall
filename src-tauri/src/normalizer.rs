use crate::segmenter::Segmenter;
use rusqlite::{params, Connection, Result};
use serde::Serialize;
use std::collections::HashMap;
use url::Url;

const TRACKING_PARAMS: &[&str] = &[
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_content",
    "utm_term",
    "fbclid",
    "gclid",
    "msclkid",
    "ref",
    "_ga",
    "spm",
    "scm",
];

#[derive(Debug, Serialize)]
pub struct NormalizeStats {
    pub total_scanned: usize,
    pub updated: usize,
    pub search_queries: usize,
    pub redirects: usize,
    pub login_pages: usize,
    pub utility_pages: usize,
    pub high_noise: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchQueryInfo {
    query: String,
}

#[derive(Debug, Clone, PartialEq)]
struct PageClassification {
    category: String,
    noise_score: f64,
}

struct NormalizerInput {
    id: String,
    title: Option<String>,
    url: Option<String>,
    domain: Option<String>,
    folder_path: Option<String>,
    user_note: Option<String>,
    content: Option<String>,
}

/// Recompute standardization metadata for every artifact.
pub fn normalize_all(conn: &Connection, segmenter: &Segmenter) -> Result<NormalizeStats> {
    let title_frequencies = load_title_frequencies(conn)?;
    let mut stmt = conn.prepare(
        r#"SELECT id, title, url, domain, folder_path, user_note, content
           FROM artifacts
           ORDER BY visited_at ASC"#,
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(NormalizerInput {
            id: row.get(0)?,
            title: row.get(1)?,
            url: row.get(2)?,
            domain: row.get(3)?,
            folder_path: row.get(4)?,
            user_note: row.get(5)?,
            content: row.get(6)?,
        })
    })?;

    let mut inputs = Vec::new();
    for row in rows {
        inputs.push(row?);
    }
    drop(stmt);

    let mut stats = NormalizeStats {
        total_scanned: inputs.len(),
        updated: 0,
        search_queries: 0,
        redirects: 0,
        login_pages: 0,
        utility_pages: 0,
        high_noise: 0,
    };

    for input in inputs {
        let url = input.url.as_deref().unwrap_or("");
        let extracted_query = extract_search_query(url).map(|q| q.query);
        let classification = classify_page(
            url,
            input.title.as_deref(),
            input.domain.as_deref(),
            &title_frequencies,
            stats.total_scanned.max(1),
        );
        let canonical_url = if url.is_empty() {
            None
        } else {
            Some(canonicalize_url(url))
        };

        match classification.category.as_str() {
            "search_query" => stats.search_queries += 1,
            "redirect" => stats.redirects += 1,
            "login" => stats.login_pages += 1,
            "utility" => stats.utility_pages += 1,
            _ => {}
        }
        if classification.noise_score > 0.7 {
            stats.high_noise += 1;
        }

        let search_text = build_search_text(
            segmenter,
            input.title.as_deref(),
            extracted_query.as_deref(),
            input.folder_path.as_deref(),
            input.user_note.as_deref(),
            input.content.as_deref(),
        );

        conn.execute(
            r#"UPDATE artifacts
               SET page_category = ?1,
                   noise_score = ?2,
                   extracted_query = ?3,
                   canonical_url = ?4,
                   referrer_domain = ?5,
                   search_text = ?6,
                   embedding_version = 0
               WHERE id = ?7"#,
            params![
                classification.category,
                classification.noise_score,
                extracted_query,
                canonical_url,
                Option::<String>::None,
                search_text,
                input.id,
            ],
        )?;
        stats.updated += 1;
    }

    Ok(stats)
}

/// Build the jieba-segmented, space-joined searchable text indexed by FTS5.
///
/// The `unicode61` tokenizer does not segment CJK text, so we pre-segment the
/// human-language fields here. The result is always a (possibly empty) string,
/// never NULL, so [`backfill_needed`] stays idempotent after a run.
fn build_search_text(
    segmenter: &Segmenter,
    title: Option<&str>,
    extracted_query: Option<&str>,
    folder_path: Option<&str>,
    user_note: Option<&str>,
    content: Option<&str>,
) -> String {
    let combined = [
        title.unwrap_or(""),
        extracted_query.unwrap_or(""),
        folder_path.unwrap_or(""),
        user_note.unwrap_or(""),
        content.unwrap_or(""),
    ]
    .join(" ");

    if combined.trim().is_empty() {
        return String::new();
    }
    segmenter.cut(&combined).join(" ")
}

/// True if any artifact still lacks a computed `search_text` (i.e. a legacy DB
/// or freshly imported rows that have not been normalized yet).
pub fn backfill_needed(conn: &Connection) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM artifacts WHERE search_text IS NULL LIMIT 1)",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|exists| exists != 0)
}

fn load_title_frequencies(conn: &Connection) -> Result<HashMap<String, usize>> {
    let mut stmt = conn.prepare(
        r#"SELECT LOWER(TRIM(title)) AS title_pattern, COUNT(*)
           FROM artifacts
           WHERE title IS NOT NULL AND TRIM(title) != ''
           GROUP BY title_pattern"#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
    })?;

    let mut frequencies = HashMap::new();
    for row in rows {
        let (title, count) = row?;
        frequencies.insert(title, count);
    }
    Ok(frequencies)
}

fn extract_search_query(url: &str) -> Option<SearchQueryInfo> {
    let parsed = Url::parse(url).ok()?;
    let host = parsed
        .host_str()?
        .strip_prefix("www.")
        .unwrap_or(parsed.host_str()?);
    let path = parsed.path();

    let param = if (host == "bing.com" || host == "cn.bing.com") && path.starts_with("/search") {
        Some("q")
    } else if host == "google.com" && path.starts_with("/search") {
        Some("q")
    } else if host == "baidu.com" && path.starts_with("/s") {
        Some("wd")
    } else if host == "sogou.com" && path.starts_with("/web") {
        Some("query")
    } else if host == "so.com" && path.starts_with("/s") {
        Some("q")
    } else if host == "search.yahoo.com" {
        Some("p")
    } else if host == "duckduckgo.com" {
        Some("q")
    } else {
        None
    }?;

    parsed
        .query_pairs()
        .find(|(key, _)| key == param)
        .and_then(|(_, value)| {
            let query = value.trim().to_string();
            if query.is_empty() {
                None
            } else {
                Some(SearchQueryInfo { query })
            }
        })
}

fn classify_page(
    url: &str,
    title: Option<&str>,
    domain: Option<&str>,
    title_frequencies: &HashMap<String, usize>,
    total_artifacts: usize,
) -> PageClassification {
    let lower_url = url.to_lowercase();
    let lower_title = title.unwrap_or("").trim().to_lowercase();
    let lower_domain = domain.unwrap_or("").to_lowercase();

    let mut category = "content";
    let mut noise_score: f64 = 0.0;

    if lower_url.starts_with("about:")
        || lower_url.starts_with("chrome://")
        || lower_url.starts_with("edge://")
    {
        category = "utility";
        noise_score += 1.0;
    } else if is_redirect_url(&lower_url) {
        category = "redirect";
        noise_score += 0.8;
    } else if extract_search_query(url).is_some() {
        category = "search_query";
    } else if is_login_page(&lower_title, &lower_domain, &lower_url) {
        category = "login";
        noise_score += 0.6;
    }

    if title.unwrap_or("").trim().chars().count() < 3 {
        noise_score += 0.3;
    }

    if matches_login_title(&lower_title) {
        noise_score += 0.5;
        if category == "content" {
            category = "login";
        }
    }

    if !lower_title.is_empty() {
        if let Some(count) = title_frequencies.get(&lower_title) {
            let frequency = *count as f64 / total_artifacts as f64;
            if frequency > 0.01 {
                noise_score += 0.3 * frequency;
            }
        }
    }

    PageClassification {
        category: category.to_string(),
        noise_score: noise_score.clamp(0.0, 1.0),
    }
}

fn is_redirect_url(lower_url: &str) -> bool {
    lower_url.contains("bing.com/ck/a")
        || lower_url.contains("google.com/url")
        || lower_url.contains("link.zhihu.com/")
        || lower_url.contains("redirect")
        || lower_url.contains("callback")
        || lower_url.contains("return_url")
}

fn is_login_page(lower_title: &str, lower_domain: &str, lower_url: &str) -> bool {
    matches_login_title(lower_title)
        || lower_domain.contains("sso")
        || lower_domain.contains("login")
        || lower_domain.contains("auth")
        || lower_domain.contains("cas")
        || lower_domain.contains("account")
        || lower_url.contains("/login")
        || lower_url.contains("/signin")
        || lower_url.contains("/oauth")
        || lower_url.contains("/sso")
}

fn matches_login_title(lower_title: &str) -> bool {
    [
        "登录",
        "认证",
        "统一认证",
        "欢迎使用",
        "sign in",
        "signin",
        "login",
        "sso",
    ]
    .iter()
    .any(|needle| lower_title.contains(needle))
}

fn canonicalize_url(url: &str) -> String {
    let Ok(mut parsed) = Url::parse(url) else {
        return url.to_string();
    };

    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(key, _)| !TRACKING_PARAMS.contains(&key.as_ref()))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();

    parsed.set_query(None);
    parsed.set_fragment(None);

    if !pairs.is_empty() {
        let mut serializer = parsed.query_pairs_mut();
        for (key, value) in pairs {
            serializer.append_pair(&key, &value);
        }
    }

    parsed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn empty_freqs() -> HashMap<String, usize> {
        HashMap::new()
    }

    #[test]
    fn test_extract_bing_query() {
        let info =
            extract_search_query("https://cn.bing.com/search?q=%E9%A9%BE%E8%80%83&form=QBLH")
                .expect("query");
        assert_eq!(info.query, "驾考");
    }

    #[test]
    fn test_extract_google_query() {
        let info =
            extract_search_query("https://www.google.com/search?q=openwrt+dns").expect("query");
        assert_eq!(info.query, "openwrt dns");
    }

    #[test]
    fn test_non_search_url_has_no_query() {
        assert_eq!(extract_search_query("https://example.com/search?q=x"), None);
    }

    #[test]
    fn test_canonicalize_removes_tracking_params() {
        let url = canonicalize_url("https://example.com/page?utm_source=x&q=keep&fbclid=abc#frag");
        assert_eq!(url, "https://example.com/page?q=keep");
    }

    #[test]
    fn test_classify_redirect() {
        let c = classify_page(
            "https://www.google.com/url?q=https%3A%2F%2Fexample.com",
            Some("Redirect"),
            Some("google.com"),
            &empty_freqs(),
            1,
        );
        assert_eq!(c.category, "redirect");
        assert!(c.noise_score >= 0.8);
    }

    #[test]
    fn test_classify_login() {
        let c = classify_page(
            "https://sso.example.edu/login",
            Some("统一认证登录"),
            Some("sso.example.edu"),
            &empty_freqs(),
            1,
        );
        assert_eq!(c.category, "login");
        assert!(c.noise_score >= 0.6);
    }

    #[test]
    fn test_normalize_all_updates_artifact() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE artifacts (
                id TEXT PRIMARY KEY,
                title TEXT,
                url TEXT,
                domain TEXT,
                visited_at TEXT,
                folder_path TEXT,
                user_note TEXT,
                content TEXT,
                page_category TEXT DEFAULT 'content',
                noise_score REAL NOT NULL DEFAULT 0.0,
                extracted_query TEXT,
                canonical_url TEXT,
                referrer_domain TEXT,
                search_text TEXT,
                embedding_version INTEGER NOT NULL DEFAULT 1
            );
            INSERT INTO artifacts (id, title, url, domain, visited_at)
            VALUES (
                'a1',
                'Bing Search',
                'https://cn.bing.com/search?q=%E9%A9%BE%E8%80%83&utm_source=x',
                'cn.bing.com',
                '2026-01-01T00:00:00'
            );
        "#,
        )
        .unwrap();

        let stats = normalize_all(&conn, &Segmenter::new()).unwrap();
        assert_eq!(stats.total_scanned, 1);
        assert_eq!(stats.search_queries, 1);

        let (category, query, canonical, search_text): (String, String, String, String) = conn
            .query_row(
                "SELECT page_category, extracted_query, canonical_url, search_text FROM artifacts WHERE id = 'a1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();

        assert_eq!(category, "search_query");
        assert_eq!(query, "驾考");
        assert_eq!(canonical, "https://cn.bing.com/search?q=%E9%A9%BE%E8%80%83");
        // search_text must contain the segmented extracted query so a literal
        // FTS MATCH on "驾考" can hit this row.
        assert!(
            search_text.contains("驾考"),
            "search_text should contain the segmented query token, got {:?}",
            search_text
        );
    }

    #[test]
    fn test_backfill_needed_flips_after_normalize() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE artifacts (
                id TEXT PRIMARY KEY,
                title TEXT,
                url TEXT,
                domain TEXT,
                visited_at TEXT,
                folder_path TEXT,
                user_note TEXT,
                content TEXT,
                page_category TEXT DEFAULT 'content',
                noise_score REAL NOT NULL DEFAULT 0.0,
                extracted_query TEXT,
                canonical_url TEXT,
                referrer_domain TEXT,
                search_text TEXT,
                embedding_version INTEGER NOT NULL DEFAULT 1
            );
            INSERT INTO artifacts (id, title, url, domain, visited_at)
            VALUES ('a1', '驾考宝典的题库哪里来的', 'https://example.com/a', 'example.com', '2026-01-01T00:00:00');
        "#,
        )
        .unwrap();

        assert!(backfill_needed(&conn).unwrap());
        normalize_all(&conn, &Segmenter::new()).unwrap();
        assert!(!backfill_needed(&conn).unwrap());
    }
}
