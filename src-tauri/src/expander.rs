use crate::segmenter::Segmenter;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};

const MAX_EXPANDED_TERMS: usize = 32;

const BUILTIN_SYNONYMS: &[(&str, &[&str])] = &[
    (
        "驾考",
        &[
            "驾照",
            "考驾照",
            "驾驶证",
            "驾驶证考试",
            "科目一",
            "科目二",
            "科目三",
            "科目四",
            "交规",
            "刷题",
            "驾校",
        ],
    ),
    (
        "NAS",
        &[
            "存储",
            "硬盘",
            "文件服务器",
            "TrueNAS",
            "群晖",
            "威联通",
            "QNAP",
            "Synology",
        ],
    ),
    ("OpenWrt", &["软路由", "旁路由", "路由器", "固件", "LEDE"]),
    (
        "Docker",
        &["容器", "镜像", "docker-compose", "k8s", "kubernetes"],
    ),
    (
        "VPN",
        &[
            "翻墙",
            "代理",
            "梯子",
            "科学上网",
            "Clash",
            "V2Ray",
            "WireGuard",
        ],
    ),
    (
        "ZFS",
        &["文件系统", "ARC", "L2ARC", "SLOG", "存储池", "zpool"],
    ),
    ("编程", &["代码", "开发", "coding", "programming", "开发者"]),
    ("Python", &["py", "pip", "django", "flask", "pytorch"]),
    ("Rust", &["cargo", "crate", "rustup", "borrow checker"]),
];

/// Platform/structural words that pollute co-occurrence / PRF expansion.
/// These are not topical, so the auto-miners must never add them as expansion
/// terms (they were the source of the "知乎"/generic-word noise). Curated
/// synonyms and the user's own query tokens are unaffected by this list.
const EXPANSION_STOPLIST: &[&str] = &[
    "知乎",
    "微博",
    "贴吧",
    "bilibili",
    "哔哩哔哩",
    "b站",
    "csdn",
    "简书",
    "掘金",
    "博客园",
    "百度",
    "谷歌",
    "搜索",
    "视频",
    "图片",
    "下载",
    "官网",
    "官方",
    "首页",
    "登录",
    "注册",
    "论坛",
    "在线",
    "免费",
    "大全",
    "最新",
    "v2ex",
    "太平洋",
    // Generic interrogatives / structural Chinese — never topical. (Only mined
    // expansion terms are filtered; the user's own query tokens are untouched,
    // so queries like "科目一怎么考" still work.)
    "哪里",
    "怎么",
    "怎么办",
    "为什么",
    "是什么",
    "如何",
    "多少",
    "www",
    "com",
    "cn",
    "http",
    "https",
    "html",
    "index",
];

fn is_expansion_stopword(term: &str) -> bool {
    let lower = term.to_lowercase();
    EXPANSION_STOPLIST.iter().any(|word| *word == lower)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandedQuery {
    pub original: String,
    pub expanded_terms: Vec<String>,
    pub fts_query: String,
}

pub struct QueryExpander;

impl QueryExpander {
    pub fn new() -> Self {
        Self
    }

    pub fn expand(&self, conn: &Connection, segmenter: &Segmenter, query: &str) -> ExpandedQuery {
        let mut terms = segmenter.cut_for_search(query);
        terms.extend(self.expand_by_synonyms(conn, &terms));
        terms.extend(self.expand_by_cooccurrence(conn, segmenter, &terms));
        terms.extend(self.expand_by_prf(conn, segmenter, query));

        let expanded_terms = dedupe_terms(terms)
            .into_iter()
            .take(MAX_EXPANDED_TERMS)
            .collect::<Vec<_>>();
        let fts_query = build_fts_query_from_terms(&expanded_terms);

        ExpandedQuery {
            original: query.to_string(),
            expanded_terms,
            fts_query,
        }
    }

    fn expand_by_synonyms(&self, conn: &Connection, tokens: &[String]) -> Vec<String> {
        let mut expanded = Vec::new();

        for token in tokens {
            expanded.extend(builtin_synonyms_for(token));
        }

        if let Ok(mut stmt) = conn.prepare(
            r#"SELECT synonym
               FROM concept_synonyms
               WHERE LOWER(term) = LOWER(?1)
               ORDER BY weight DESC
               LIMIT 12"#,
        ) {
            for token in tokens {
                if let Ok(rows) = stmt.query_map(params![token], |row| row.get::<_, String>(0)) {
                    for synonym in rows.flatten() {
                        expanded.push(synonym);
                    }
                }
            }
        }

        expanded
    }

    fn expand_by_cooccurrence(
        &self,
        conn: &Connection,
        segmenter: &Segmenter,
        tokens: &[String],
    ) -> Vec<String> {
        let mut counts: HashMap<String, usize> = HashMap::new();

        let Ok(mut stmt) = conn.prepare(
            r#"SELECT COALESCE(title, ''), COALESCE(extracted_query, '')
               FROM artifacts
               WHERE title LIKE ?1 OR extracted_query LIKE ?1
               LIMIT 40"#,
        ) else {
            return vec![];
        };

        for token in tokens.iter().take(5) {
            let pattern = format!("%{}%", token);
            let Ok(rows) = stmt.query_map(params![pattern], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            }) else {
                continue;
            };

            for row in rows.flatten() {
                let text = format!("{} {}", row.0, row.1);
                for keyword in segmenter.extract_keywords(&text, 8) {
                    if is_expansion_stopword(&keyword) {
                        continue;
                    }
                    if !tokens.iter().any(|t| t.eq_ignore_ascii_case(&keyword)) {
                        *counts.entry(keyword).or_insert(0) += 1;
                    }
                }
            }
        }

        let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.len().cmp(&a.0.len())));
        ranked
            .into_iter()
            .filter(|(_, count)| *count >= 3)
            .take(6)
            .map(|(term, _)| term)
            .collect()
    }

    fn expand_by_prf(
        &self,
        conn: &Connection,
        segmenter: &Segmenter,
        original_query: &str,
    ) -> Vec<String> {
        let base_terms = segmenter.cut_for_search(original_query);
        let fts_query = build_fts_query_from_terms(&base_terms);
        if fts_query.is_empty() {
            return vec![];
        }

        let Ok(mut stmt) = conn.prepare(
            r#"SELECT COALESCE(a.title, ''), COALESCE(a.extracted_query, '')
               FROM artifacts_fts fts
               JOIN artifacts a ON fts.rowid = a.rowid
               WHERE artifacts_fts MATCH ?1
               ORDER BY bm25(artifacts_fts)
               LIMIT 5"#,
        ) else {
            return vec![];
        };

        let Ok(rows) = stmt.query_map(params![fts_query], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }) else {
            return vec![];
        };

        let mut text = String::new();
        for row in rows.flatten() {
            text.push_str(&row.0);
            text.push(' ');
            text.push_str(&row.1);
            text.push(' ');
        }

        segmenter
            .extract_keywords(&text, 8)
            .into_iter()
            .filter(|keyword| !is_expansion_stopword(keyword))
            .collect()
    }
}

impl Default for QueryExpander {
    fn default() -> Self {
        Self::new()
    }
}

pub fn build_fts_query_from_terms(terms: &[String]) -> String {
    let quoted = terms
        .iter()
        .filter_map(|term| {
            let clean = sanitize_fts_term(term);
            if clean.is_empty() {
                None
            } else {
                Some(format!("\"{}\"", clean))
            }
        })
        .collect::<Vec<_>>();

    quoted.join(" OR ")
}

fn builtin_synonyms_for(token: &str) -> Vec<String> {
    let token_lower = token.to_lowercase();
    let mut expanded = Vec::new();

    for (term, synonyms) in BUILTIN_SYNONYMS {
        let term_lower = term.to_lowercase();
        let matched = token_lower == term_lower
            || synonyms
                .iter()
                .any(|synonym| token_lower == synonym.to_lowercase());

        if matched {
            expanded.push((*term).to_string());
            expanded.extend(synonyms.iter().map(|s| (*s).to_string()));
        }
    }

    expanded
}

fn sanitize_fts_term(term: &str) -> String {
    term.replace('"', "").trim().to_string()
}

fn dedupe_terms(terms: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();

    for term in terms {
        let clean = sanitize_fts_term(&term);
        if clean.is_empty() {
            continue;
        }
        let key = clean.to_lowercase();
        if seen.insert(key) {
            unique.push(clean);
        }
    }

    unique
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn test_build_fts_query_from_terms() {
        let terms = vec!["ZFS".to_string(), "缓存".to_string(), "he\"llo".to_string()];
        assert_eq!(
            build_fts_query_from_terms(&terms),
            r#""ZFS" OR "缓存" OR "hello""#
        );
    }

    #[test]
    fn test_builtin_synonyms_are_bidirectional() {
        let terms = builtin_synonyms_for("考驾照");
        assert!(terms.iter().any(|t| t == "驾考"));
        assert!(terms.iter().any(|t| t == "科目一"));
    }

    #[test]
    fn test_expand_uses_db_synonyms() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE concept_synonyms (
                term TEXT NOT NULL,
                synonym TEXT NOT NULL,
                weight REAL NOT NULL DEFAULT 1.0,
                source TEXT NOT NULL DEFAULT 'manual',
                PRIMARY KEY (term, synonym)
            );
            INSERT INTO concept_synonyms (term, synonym, weight)
            VALUES ('旁路由', 'OpenWrt', 1.0);
        "#,
        )
        .unwrap();

        let segmenter = Segmenter::new();
        let expanded = QueryExpander::new().expand(&conn, &segmenter, "旁路由");

        assert!(expanded.expanded_terms.iter().any(|t| t == "OpenWrt"));
        assert!(expanded.fts_query.contains("OpenWrt"));
    }

    #[test]
    fn test_is_expansion_stopword() {
        assert!(is_expansion_stopword("知乎"));
        assert!(is_expansion_stopword("Bilibili"));
        assert!(is_expansion_stopword("WWW"));
        // Contentful words must never be treated as stopwords.
        assert!(!is_expansion_stopword("驾考"));
        assert!(!is_expansion_stopword("题库"));
    }

    #[test]
    fn test_expand_excludes_platform_stopwords() {
        let db_path = std::env::temp_dir().join(format!("recall-exp-{}.db", uuid::Uuid::new_v4()));
        let conn = crate::db::init_db(&db_path).unwrap();

        // Several pages that share the topic word AND a platform word ("知乎").
        // Without the stoplist, co-occurrence mining would surface "知乎" and
        // pull in unrelated 知乎 pages.
        for i in 0..5 {
            conn.execute(
                "INSERT INTO artifacts (id, type, title, url, domain, created_at)
                 VALUES (?1, 'history', ?2, ?3, 'www.zhihu.com', '2025-01-01T00:00:00')",
                rusqlite::params![
                    format!("z{i}"),
                    format!("驾考宝典题库讨论{i} - 知乎"),
                    format!("https://www.zhihu.com/question/{i}"),
                ],
            )
            .unwrap();
        }

        let segmenter = Segmenter::new();
        let expanded = QueryExpander::new().expand(&conn, &segmenter, "驾考");

        assert!(
            !expanded.expanded_terms.iter().any(|t| t == "知乎"),
            "platform stopword 知乎 must not appear in expansion: {:?}",
            expanded.expanded_terms
        );

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }
}
