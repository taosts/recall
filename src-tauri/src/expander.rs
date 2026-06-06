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
            .filter(|(_, count)| *count >= 2)
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

        segmenter.extract_keywords(&text, 8)
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
}
