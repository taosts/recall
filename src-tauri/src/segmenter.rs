use jieba_rs::Jieba;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone)]
pub struct Segmenter {
    jieba: Arc<Jieba>,
}

impl Segmenter {
    pub fn new() -> Self {
        Self {
            jieba: Arc::new(Jieba::new()),
        }
    }

    /// Search-mode segmentation. Includes the original string and whitespace
    /// tokens so mixed Chinese/English queries keep exact recall.
    pub fn cut_for_search(&self, text: &str) -> Vec<String> {
        let mut tokens = Vec::new();
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            tokens.push(trimmed.to_string());
        }

        for token in trimmed.split_whitespace() {
            push_token(&mut tokens, token);
        }

        for token in self.jieba.cut_for_search(trimmed, true) {
            push_token(&mut tokens, token.word);
        }

        dedupe(tokens)
    }

    /// Precise segmentation for indexing or title analysis.
    pub fn cut(&self, text: &str) -> Vec<String> {
        dedupe(
            self.jieba
                .cut(text.trim(), true)
                .into_iter()
                .filter_map(|token| normalize_token(token.word))
                .collect(),
        )
    }

    /// Lightweight keyword extraction based on search-mode token frequency.
    pub fn extract_keywords(&self, text: &str, top_k: usize) -> Vec<String> {
        let mut counts: HashMap<String, usize> = HashMap::new();

        for token in text.trim().split_whitespace() {
            if let Some(token) = normalize_token(token) {
                if !is_noise_token(&token) {
                    *counts.entry(token).or_insert(0) += 1;
                }
            }
        }

        for token in self.jieba.cut_for_search(text.trim(), true) {
            let Some(token) = normalize_token(token.word) else {
                continue;
            };
            if is_noise_token(&token) {
                continue;
            }
            *counts.entry(token).or_insert(0) += 1;
        }

        let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.len().cmp(&a.0.len())));
        ranked
            .into_iter()
            .take(top_k)
            .map(|(token, _)| token)
            .collect()
    }
}

impl Default for Segmenter {
    fn default() -> Self {
        Self::new()
    }
}

fn push_token(tokens: &mut Vec<String>, token: &str) {
    if let Some(normalized) = normalize_token(token) {
        tokens.push(normalized);
    }
}

fn normalize_token(token: &str) -> Option<String> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    let token = token.trim_matches(|c: char| c.is_ascii_punctuation());
    if token.is_empty() {
        return None;
    }
    Some(token.to_string())
}

fn dedupe(tokens: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for token in tokens {
        let key = token.to_lowercase();
        if seen.insert(key) {
            unique.push(token);
        }
    }
    unique
}

fn is_noise_token(token: &str) -> bool {
    if token.chars().count() <= 1 {
        return true;
    }
    matches!(
        token.to_lowercase().as_str(),
        "the" | "and" | "for" | "with" | "http" | "https" | "www"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cut_for_search_includes_original_and_subtokens() {
        let segmenter = Segmenter::new();
        let tokens = segmenter.cut_for_search("驾考宝典");
        assert!(tokens.iter().any(|t| t == "驾考宝典"));
        assert!(tokens.iter().any(|t| t == "驾考") || tokens.iter().any(|t| t == "宝典"));
    }

    #[test]
    fn test_cut_for_search_keeps_english_terms() {
        let segmenter = Segmenter::new();
        let tokens = segmenter.cut_for_search("OpenWrt DNS");
        assert!(tokens.iter().any(|t| t.eq_ignore_ascii_case("OpenWrt")));
        assert!(tokens.iter().any(|t| t.eq_ignore_ascii_case("DNS")));
    }

    #[test]
    fn test_extract_keywords_limits_results() {
        let segmenter = Segmenter::new();
        let keywords = segmenter.extract_keywords("OpenWrt DNS DNS setup", 2);
        assert!(keywords.len() <= 2);
        assert!(keywords.iter().any(|t| t.eq_ignore_ascii_case("DNS")));
    }
}
