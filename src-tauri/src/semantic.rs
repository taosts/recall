use crate::models::Artifact;
use crate::search::row_to_artifact;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use rusqlite::types::ToSql;
use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::path::PathBuf;

pub const CURRENT_EMBEDDING_VERSION: i64 = 1;
pub const EMBEDDING_MODEL_NAME: &str = "BAAI/bge-small-zh-v1.5";
pub const EMBEDDING_DIMS: i64 = 512;

const BGE_QUERY_PREFIX: &str = "为这个句子生成表示以用于检索相关文章：";
const DEFAULT_EMBEDDING_BATCH_SIZE: usize = 16;

#[derive(Debug, Serialize, Deserialize)]
pub struct EmbeddingProgress {
    pub done: i64,
    pub total: i64,
    pub pending: i64,
    pub embedded: i64,
    pub current_version: i64,
    pub model: String,
    pub dims: i64,
    pub model_loaded: bool,
    pub cache_dir: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmbeddingRunStats {
    pub scanned: usize,
    pub embedded: usize,
    pub skipped: usize,
    pub remaining: i64,
    pub errors: Vec<String>,
}

pub struct EmbeddingRuntime {
    cache_dir: PathBuf,
    model: Option<TextEmbedding>,
}

impl EmbeddingRuntime {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            model: None,
        }
    }

    pub fn cache_dir(&self) -> &PathBuf {
        &self.cache_dir
    }

    pub fn model_loaded(&self) -> bool {
        self.model.is_some()
    }

    pub fn ensure_model_loaded(&mut self) -> std::result::Result<(), String> {
        if self.model.is_some() {
            return Ok(());
        }

        std::fs::create_dir_all(&self.cache_dir).map_err(|e| e.to_string())?;
        let options = InitOptions::new(EmbeddingModel::BGESmallZHV15)
            .with_cache_dir(self.cache_dir.clone())
            .with_show_download_progress(false);
        let model = TextEmbedding::try_new(options).map_err(|e| e.to_string())?;
        self.model = Some(model);
        Ok(())
    }

    pub fn embed_documents(
        &mut self,
        texts: &[String],
    ) -> std::result::Result<Vec<Vec<f32>>, String> {
        self.ensure_model_loaded()?;
        let model = self
            .model
            .as_mut()
            .ok_or_else(|| "embedding model is not loaded".to_string())?;
        model
            .embed(texts, Some(DEFAULT_EMBEDDING_BATCH_SIZE))
            .map_err(|e| e.to_string())
    }

    pub fn embed_query_if_loaded(
        &mut self,
        query: &str,
    ) -> std::result::Result<Option<Vec<f32>>, String> {
        if self.model.is_none() {
            return Ok(None);
        }

        let model = self
            .model
            .as_mut()
            .ok_or_else(|| "embedding model is not loaded".to_string())?;
        let prefixed = prefix_query_for_retrieval(query);
        let mut vectors = model
            .embed(vec![prefixed], Some(1))
            .map_err(|e| e.to_string())?;
        Ok(vectors.pop())
    }
}

pub fn embedding_progress(
    conn: &Connection,
    runtime: &EmbeddingRuntime,
) -> Result<EmbeddingProgress> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))?;
    let done: i64 = conn.query_row(
        "SELECT COUNT(*) FROM artifacts WHERE embedding_version >= ?1",
        params![CURRENT_EMBEDDING_VERSION],
        |row| row.get(0),
    )?;
    let embedded: i64 = conn.query_row("SELECT COUNT(*) FROM artifact_embeddings", [], |row| {
        row.get(0)
    })?;

    Ok(EmbeddingProgress {
        done,
        total,
        pending: total.saturating_sub(done),
        embedded,
        current_version: CURRENT_EMBEDDING_VERSION,
        model: EMBEDDING_MODEL_NAME.to_string(),
        dims: EMBEDDING_DIMS,
        model_loaded: runtime.model_loaded(),
        cache_dir: runtime.cache_dir().to_string_lossy().to_string(),
    })
}

pub fn embed_pending_artifacts(
    conn: &Connection,
    runtime: &mut EmbeddingRuntime,
    batch_size: usize,
) -> std::result::Result<EmbeddingRunStats, String> {
    let limit = batch_size.max(1).min(128);
    let candidates = pending_artifacts(conn, limit).map_err(|e| e.to_string())?;
    let scanned = candidates.len();

    let mut skipped = 0;
    let mut indexable = Vec::new();
    let mut texts = Vec::new();

    for artifact in candidates {
        if should_index_artifact(&artifact) {
            let text = embedding_text(&artifact);
            if text.trim().is_empty() {
                mark_embedding_complete(conn, &artifact.id).map_err(|e| e.to_string())?;
                skipped += 1;
            } else {
                texts.push(text);
                indexable.push(artifact);
            }
        } else {
            delete_embedding(conn, &artifact.id).map_err(|e| e.to_string())?;
            mark_embedding_complete(conn, &artifact.id).map_err(|e| e.to_string())?;
            skipped += 1;
        }
    }

    let mut embedded = 0;
    let mut errors = Vec::new();

    if !texts.is_empty() {
        match runtime.embed_documents(&texts) {
            Ok(vectors) => {
                for (artifact, vector) in indexable.iter().zip(vectors.iter()) {
                    if vector.len() != EMBEDDING_DIMS as usize {
                        errors.push(format!(
                            "{} produced {} dims, expected {}",
                            artifact.id,
                            vector.len(),
                            EMBEDDING_DIMS
                        ));
                        continue;
                    }
                    upsert_embedding(conn, &artifact.id, vector).map_err(|e| e.to_string())?;
                    mark_embedding_complete(conn, &artifact.id).map_err(|e| e.to_string())?;
                    embedded += 1;
                }
            }
            Err(e) => errors.push(e),
        }
    }

    let remaining = count_pending(conn).map_err(|e| e.to_string())?;

    Ok(EmbeddingRunStats {
        scanned,
        embedded,
        skipped,
        remaining,
        errors,
    })
}

pub fn search_semantic(
    conn: &Connection,
    query_vector: &[f32],
    date_from: Option<&str>,
    date_to: Option<&str>,
    source: Option<&str>,
    limit: usize,
) -> Result<Vec<(Artifact, f64)>> {
    if query_vector.is_empty() {
        return Ok(Vec::new());
    }

    let mut sql = String::from(
        r#"SELECT a.id, a.type, a.title, a.url, a.domain, a.created_at,
                  a.visited_at, a.is_bookmarked, a.visit_count, a.source,
                  a.content, a.user_note, a.folder_path, a.import_batch,
                  a.page_category, a.noise_score, a.extracted_query,
                  a.canonical_url, a.referrer_domain,
                  e.embedding
           FROM artifact_embeddings e
           JOIN artifacts a ON a.id = e.artifact_id
           WHERE e.model = ?"#,
    );

    let mut owned_params: Vec<Box<dyn ToSql>> = vec![Box::new(EMBEDDING_MODEL_NAME.to_string())];
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

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn ToSql> = owned_params.iter().map(|p| p.as_ref()).collect();
    let mut rows = stmt.query(param_refs.as_slice())?;

    let mut scored = Vec::new();
    while let Some(row) = rows.next()? {
        let artifact = row_to_artifact(row)?;
        let blob: Vec<u8> = row.get(19)?;
        let vector = blob_to_f32_vec(&blob);
        if vector.len() != query_vector.len() {
            continue;
        }
        let score = cosine_similarity(query_vector, &vector) as f64;
        scored.push((artifact, score));
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
    scored.truncate(limit);
    Ok(scored)
}

fn pending_artifacts(conn: &Connection, limit: usize) -> Result<Vec<Artifact>> {
    let mut stmt = conn.prepare(
        r#"SELECT id, type, title, url, domain, created_at, visited_at,
                  is_bookmarked, visit_count, source, content, user_note,
                  folder_path, import_batch, page_category, noise_score,
                  extracted_query, canonical_url, referrer_domain
           FROM artifacts
           WHERE embedding_version < ?1
           ORDER BY COALESCE(visited_at, created_at) DESC
           LIMIT ?2"#,
    )?;

    let rows = stmt.query_map(params![CURRENT_EMBEDDING_VERSION, limit as i64], |row| {
        row_to_artifact(row)
    })?;
    let mut artifacts = Vec::new();
    for artifact in rows {
        artifacts.push(artifact?);
    }
    Ok(artifacts)
}

fn count_pending(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM artifacts WHERE embedding_version < ?1",
        params![CURRENT_EMBEDDING_VERSION],
        |row| row.get(0),
    )
}

fn should_index_artifact(artifact: &Artifact) -> bool {
    !matches!(
        artifact.page_category.as_deref(),
        Some("redirect") | Some("login") | Some("utility")
    ) && artifact.noise_score <= 0.85
}

fn embedding_text(artifact: &Artifact) -> String {
    [
        artifact.extracted_query.as_deref().unwrap_or(""),
        artifact.title.as_deref().unwrap_or(""),
        artifact.user_note.as_deref().unwrap_or(""),
        artifact.content.as_deref().unwrap_or(""),
        artifact.domain.as_deref().unwrap_or(""),
        artifact.url.as_deref().unwrap_or(""),
    ]
    .into_iter()
    .filter(|part| !part.trim().is_empty())
    .collect::<Vec<_>>()
    .join("\n")
}

fn prefix_query_for_retrieval(query: &str) -> String {
    format!("{}{}", BGE_QUERY_PREFIX, query.trim())
}

fn upsert_embedding(conn: &Connection, artifact_id: &str, vector: &[f32]) -> Result<()> {
    conn.execute(
        r#"INSERT INTO artifact_embeddings
               (artifact_id, model, dims, embedding, updated_at)
           VALUES (?1, ?2, ?3, ?4, ?5)
           ON CONFLICT(artifact_id) DO UPDATE SET
               model = excluded.model,
               dims = excluded.dims,
               embedding = excluded.embedding,
               updated_at = excluded.updated_at"#,
        params![
            artifact_id,
            EMBEDDING_MODEL_NAME,
            EMBEDDING_DIMS,
            f32_vec_to_blob(vector),
            chrono::Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn delete_embedding(conn: &Connection, artifact_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM artifact_embeddings WHERE artifact_id = ?1",
        params![artifact_id],
    )?;
    Ok(())
}

fn mark_embedding_complete(conn: &Connection, artifact_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE artifacts SET embedding_version = ?1 WHERE id = ?2",
        params![CURRENT_EMBEDDING_VERSION, artifact_id],
    )?;
    Ok(())
}

fn f32_vec_to_blob(vector: &[f32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(vector.len() * std::mem::size_of::<f32>());
    for value in vector {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    blob
}

fn blob_to_f32_vec(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(std::mem::size_of::<f32>())
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    let mut dot = 0.0_f32;
    let mut norm_a = 0.0_f32;
    let mut norm_b = 0.0_f32;

    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a.sqrt() * norm_b.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact_with_quality(page_category: &str, noise_score: f64) -> Artifact {
        Artifact {
            id: "a1".to_string(),
            r#type: "history".to_string(),
            title: Some("驾考宝典题库来源".to_string()),
            url: Some("https://example.com".to_string()),
            domain: Some("example.com".to_string()),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            visited_at: None,
            is_bookmarked: false,
            visit_count: 1,
            source: Some("edge".to_string()),
            content: None,
            user_note: None,
            folder_path: None,
            import_batch: None,
            page_category: Some(page_category.to_string()),
            noise_score,
            extracted_query: Some("考驾照".to_string()),
            canonical_url: None,
            referrer_domain: None,
        }
    }

    fn test_conn() -> Connection {
        let db_path =
            std::env::temp_dir().join(format!("recall-semantic-{}.db", uuid::Uuid::new_v4()));
        crate::db::init_db(&db_path).unwrap()
    }

    fn insert_minimal_artifact(conn: &Connection, id: &str, title: &str) {
        conn.execute(
            r#"INSERT INTO artifacts
                   (id, type, title, url, domain, created_at, visited_at, source, embedding_version)
               VALUES (?1, 'history', ?2, ?3, 'example.com', '2026-01-01T00:00:00Z',
                       '2026-01-01T00:00:00Z', 'edge', ?4)"#,
            params![
                id,
                title,
                format!("https://example.com/{}", id),
                CURRENT_EMBEDDING_VERSION,
            ],
        )
        .unwrap();
    }

    fn insert_pending_artifact_with_quality(
        conn: &Connection,
        id: &str,
        page_category: &str,
        noise_score: f64,
    ) {
        conn.execute(
            r#"INSERT INTO artifacts
                   (id, type, title, created_at, page_category, noise_score, embedding_version)
               VALUES (?1, 'history', ?2, '2026-01-01T00:00:00Z', ?3, ?4, 0)"#,
            params![id, format!("Pending {}", id), page_category, noise_score],
        )
        .unwrap();
    }

    #[test]
    fn test_blob_roundtrip_preserves_vector() {
        let vector = vec![1.0_f32, -2.5, 0.125, 9.0];
        let blob = f32_vec_to_blob(&vector);
        assert_eq!(blob.len(), vector.len() * 4);
        assert_eq!(blob_to_f32_vec(&blob), vector);
    }

    #[test]
    fn test_cosine_similarity_orders_related_vectors() {
        let query = [1.0_f32, 0.0, 0.0];
        let close = [0.9_f32, 0.1, 0.0];
        let far = [0.0_f32, 1.0, 0.0];

        assert!(cosine_similarity(&query, &close) > cosine_similarity(&query, &far));
    }

    #[test]
    fn test_bge_query_prefix_is_centralized() {
        assert_eq!(
            prefix_query_for_retrieval("考驾照"),
            "为这个句子生成表示以用于检索相关文章：考驾照"
        );
    }

    #[test]
    fn test_noise_and_utility_pages_are_not_indexed() {
        assert!(should_index_artifact(&artifact_with_quality(
            "content", 0.1
        )));
        assert!(!should_index_artifact(&artifact_with_quality(
            "utility", 0.1
        )));
        assert!(!should_index_artifact(&artifact_with_quality(
            "content", 0.9
        )));
    }

    #[test]
    fn test_embedding_progress_counts_pending_without_loading_model() {
        let conn = test_conn();
        conn.execute(
            r#"INSERT INTO artifacts (id, type, title, created_at, embedding_version)
               VALUES ('pending', 'history', 'Pending page', '2026-01-01T00:00:00Z', 0)"#,
            [],
        )
        .unwrap();

        let runtime = EmbeddingRuntime::new(std::env::temp_dir().join("recall-models-test"));
        let progress = embedding_progress(&conn, &runtime).unwrap();

        assert_eq!(progress.total, 1);
        assert_eq!(progress.done, 0);
        assert_eq!(progress.pending, 1);
        assert!(!progress.model_loaded);
    }

    #[test]
    fn test_semantic_search_returns_nearest_embedding() {
        let conn = test_conn();
        insert_minimal_artifact(&conn, "close", "Close result");
        insert_minimal_artifact(&conn, "far", "Far result");

        let mut close = vec![0.0_f32; EMBEDDING_DIMS as usize];
        close[0] = 1.0;
        let mut far = vec![0.0_f32; EMBEDDING_DIMS as usize];
        far[1] = 1.0;

        upsert_embedding(&conn, "close", &close).unwrap();
        upsert_embedding(&conn, "far", &far).unwrap();

        let results = search_semantic(&conn, &close, None, None, None, 10).unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0.id, "close");
        assert!(results[0].1 > results[1].1);
    }

    #[test]
    fn test_embed_pending_skip_only_batch_does_not_load_model_or_stall_queue() {
        let conn = test_conn();
        insert_pending_artifact_with_quality(&conn, "utility", "utility", 0.1);
        insert_pending_artifact_with_quality(&conn, "noise", "content", 0.9);

        let mut runtime = EmbeddingRuntime::new(std::env::temp_dir().join("recall-models-test"));
        let stats = embed_pending_artifacts(&conn, &mut runtime, 32).unwrap();
        let progress = embedding_progress(&conn, &runtime).unwrap();

        assert_eq!(stats.scanned, 2);
        assert_eq!(stats.embedded, 0);
        assert_eq!(stats.skipped, 2);
        assert_eq!(stats.remaining, 0);
        assert!(stats.errors.is_empty());
        assert!(!runtime.model_loaded());
        assert_eq!(progress.pending, 0);
        assert_eq!(progress.embedded, 0);
    }
}
