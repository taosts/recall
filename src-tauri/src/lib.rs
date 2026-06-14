pub mod db;
pub mod expander;
pub mod import;
pub mod models;
pub mod normalizer;
pub mod quest;
pub mod search;
pub mod segmenter;
pub mod semantic;

use rusqlite::Connection;
use std::sync::Mutex;
use tauri::{Manager, State};
use uuid::Uuid;

use crate::models::{
    Artifact, BrowserInfo, DbStats, ImportStats, Quest, QuestSummary, SearchResult,
};

// ─────────────────────────────────────────────────────────────────────────────
// Shared application state
// ─────────────────────────────────────────────────────────────────────────────

pub struct AppDb(pub Mutex<Connection>);

pub struct AppEmbeddings(pub Mutex<semantic::EmbeddingRuntime>);

// ─────────────────────────────────────────────────────────────────────────────
// Tauri Commands
// ─────────────────────────────────────────────────────────────────────────────

/// Search artifacts using fuzzy/keyword queries with optional filters.
///
/// `query`       — user's natural-language or keyword query
/// `date_from`   — optional ISO 8601 lower bound
/// `date_to`     — optional ISO 8601 upper bound
/// `source`      — optional "edge" | "chrome"
/// `context_min` — context window in minutes (15 / 30 / 60 / 120); default 30
#[tauri::command]
fn search_artifacts(
    state: State<AppDb>,
    segmenter: State<segmenter::Segmenter>,
    embeddings: State<AppEmbeddings>,
    query: String,
    date_from: Option<String>,
    date_to: Option<String>,
    source: Option<String>,
    context_min: Option<i64>,
) -> Result<Vec<SearchResult>, String> {
    if query.trim().is_empty() {
        return Ok(vec![]);
    }
    let semantic_query = {
        let mut runtime = embeddings.0.lock().map_err(|e| e.to_string())?;
        runtime.embed_query_if_loaded(&query)?
    };
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    search::search(
        &conn,
        &segmenter,
        semantic_query.as_deref(),
        &query,
        date_from.as_deref(),
        date_to.as_deref(),
        source.as_deref(),
        context_min.unwrap_or(30),
    )
    .map_err(|e| e.to_string())
}

/// Detect which browsers are installed on this machine.
#[tauri::command]
fn detect_browsers() -> Vec<BrowserInfo> {
    import::detect_browsers()
}

/// Import bookmarks and/or history from a browser.
///
/// `browser`    — "edge" | "chrome"
/// `data_type`  — "bookmarks" | "history" | "all"
/// `limit_days` — optional, only import history from last N days
#[tauri::command]
fn import_browser_data(
    state: State<AppDb>,
    segmenter: State<segmenter::Segmenter>,
    browser: String,
    data_type: String,
    limit_days: Option<i64>,
) -> Result<ImportStats, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let batch = Uuid::new_v4().to_string();

    let mut combined = ImportStats {
        browser: browser.clone(),
        bookmarks_imported: 0,
        history_imported: 0,
        duplicates_skipped: 0,
        errors: vec![],
    };

    let do_bookmarks = data_type == "bookmarks" || data_type == "all";
    let do_history = data_type == "history" || data_type == "all";

    if do_bookmarks {
        match import::import_bookmarks(&browser, &conn, &batch) {
            Ok(s) => {
                combined.bookmarks_imported += s.bookmarks_imported;
                combined.duplicates_skipped += s.duplicates_skipped;
                combined.errors.extend(s.errors);
            }
            Err(e) => combined.errors.push(e),
        }
    }

    if do_history {
        match import::import_history(&browser, &conn, &batch, limit_days) {
            Ok(s) => {
                combined.history_imported += s.history_imported;
                combined.duplicates_skipped += s.duplicates_skipped;
                combined.errors.extend(s.errors);
            }
            Err(e) => combined.errors.push(e),
        }
    }

    // Recompute normalization metadata + FTS search_text for the newly imported
    // rows. Owned here (rather than inside import.rs) so the segmenter is in scope
    // and we run it once even when data_type == "all".
    if let Err(e) = normalizer::normalize_all(&conn, &segmenter) {
        combined.errors.push(format!("Normalize error: {}", e));
    }

    Ok(combined)
}

/// Get temporal context for a specific artifact.
///
/// Returns artifacts accessed within ±window_minutes of the given artifact.
/// The window is user-configurable (15 / 30 / 60 / 120 minutes).
#[tauri::command]
fn get_context(
    state: State<AppDb>,
    segmenter: State<segmenter::Segmenter>,
    artifact_id: String,
    window_minutes: Option<i64>,
) -> Result<Vec<Artifact>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    search::get_context(
        &conn,
        &segmenter,
        &artifact_id,
        window_minutes.unwrap_or(30),
    )
    .map_err(|e| e.to_string())
}

/// Add or update a user note on an artifact.
#[tauri::command]
fn add_note(state: State<AppDb>, artifact_id: String, note: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    search::set_user_note(&conn, &artifact_id, &note).map_err(|e| e.to_string())
}

/// Get overall database statistics for the status bar.
#[tauri::command]
fn get_stats(state: State<AppDb>) -> Result<DbStats, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    search::get_stats(&conn).map_err(|e| e.to_string())
}

/// Clear all imported artifacts and derived local recall data.
#[tauri::command]
fn clear_user_data(state: State<AppDb>) -> Result<(), String> {
    let mut conn = state.0.lock().map_err(|e| e.to_string())?;
    db::clear_user_data(&mut conn).map_err(|e| e.to_string())
}

/// Recompute Phase 3 normalization metadata for all artifacts.
#[tauri::command]
fn normalize_artifacts(
    state: State<AppDb>,
    segmenter: State<segmenter::Segmenter>,
) -> Result<normalizer::NormalizeStats, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    normalizer::normalize_all(&conn, &segmenter).map_err(|e| e.to_string())
}

/// Return semantic embedding queue and model-cache status without downloading the model.
#[tauri::command]
fn get_embedding_progress(
    state: State<AppDb>,
    embeddings: State<AppEmbeddings>,
) -> Result<semantic::EmbeddingProgress, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let runtime = embeddings.0.lock().map_err(|e| e.to_string())?;
    semantic::embedding_progress(&conn, &runtime).map_err(|e| e.to_string())
}

/// Explicit opt-in embedding preparation. This may download the local BGE model.
#[tauri::command]
fn prepare_embeddings(
    state: State<AppDb>,
    embeddings: State<AppEmbeddings>,
    batch_size: Option<usize>,
) -> Result<semantic::EmbeddingRunStats, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let mut runtime = embeddings.0.lock().map_err(|e| e.to_string())?;
    semantic::embed_pending_artifacts(&conn, &mut runtime, batch_size.unwrap_or(32))
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 2: Quest commands
// ─────────────────────────────────────────────────────────────────────────────

#[tauri::command]
fn generate_quests(
    state: State<AppDb>,
    segmenter: State<segmenter::Segmenter>,
) -> Result<usize, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    quest::generate_quests(&conn, &segmenter).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_quests(
    state: State<AppDb>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<QuestSummary>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    quest::list_quests(&conn, limit.unwrap_or(20), offset.unwrap_or(0)).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_quest(state: State<AppDb>, quest_id: String) -> Result<Quest, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    quest::get_quest(&conn, &quest_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_quest(state: State<AppDb>, quest_id: String, name: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    quest::rename_quest(&conn, &quest_id, &name).map_err(|e| e.to_string())
}

#[tauri::command]
fn merge_quests(state: State<AppDb>, quest_ids: Vec<String>) -> Result<String, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    quest::merge_quests(&conn, quest_ids).map_err(|e| e.to_string())
}

#[tauri::command]
fn archive_quest(state: State<AppDb>, quest_id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    quest::archive_quest(&conn, &quest_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_quest_for_artifact(
    state: State<AppDb>,
    artifact_id: String,
) -> Result<Vec<QuestSummary>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    quest::get_quest_for_artifact(&conn, &artifact_id).map_err(|e| e.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// App entry point
// ─────────────────────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir().unwrap_or_else(|_| {
                std::env::current_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from("."))
                    .join(".data")
            });
            std::fs::create_dir_all(&app_data_dir).expect("Failed to create app data directory");

            let db_path = app_data_dir.join("recall.db");
            let conn = db::init_db(&db_path).expect("Failed to initialize database");
            let model_cache_dir = app_data_dir.join("models");

            // Build the shared segmenter, then — on first run after upgrade or for
            // a legacy DB — backfill normalization metadata and the jieba-segmented
            // FTS `search_text` so Chinese literal search works before any query is
            // served. Runs only when some row still lacks `search_text`.
            let segmenter = segmenter::Segmenter::new();
            match normalizer::backfill_needed(&conn) {
                Ok(true) => {
                    if let Err(e) = normalizer::normalize_all(&conn, &segmenter) {
                        eprintln!("Startup normalization backfill failed: {}", e);
                    }
                }
                Ok(false) => {}
                Err(e) => eprintln!("Startup backfill check failed: {}", e),
            }

            app.manage(AppDb(Mutex::new(conn)));
            app.manage(AppEmbeddings(Mutex::new(semantic::EmbeddingRuntime::new(
                model_cache_dir,
            ))));
            app.manage(segmenter);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            search_artifacts,
            detect_browsers,
            import_browser_data,
            get_context,
            add_note,
            get_stats,
            clear_user_data,
            normalize_artifacts,
            get_embedding_progress,
            prepare_embeddings,
            generate_quests,
            list_quests,
            get_quest,
            rename_quest,
            merge_quests,
            archive_quest,
            get_quest_for_artifact,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
