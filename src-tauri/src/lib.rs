pub mod db;
pub mod models;
pub mod import;
pub mod search;
pub mod quest;

use std::sync::Mutex;
use tauri::{Manager, State};
use rusqlite::Connection;
use uuid::Uuid;

use crate::models::{SearchResult, Artifact, ImportStats, BrowserInfo, DbStats, Quest, QuestSummary};

// ─────────────────────────────────────────────────────────────────────────────
// Shared application state
// ─────────────────────────────────────────────────────────────────────────────

pub struct AppDb(pub Mutex<Connection>);

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
    query: String,
    date_from: Option<String>,
    date_to: Option<String>,
    source: Option<String>,
    context_min: Option<i64>,
) -> Result<Vec<SearchResult>, String> {
    if query.trim().is_empty() {
        return Ok(vec![]);
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    search::search(
        &conn,
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
    let do_history   = data_type == "history"   || data_type == "all";

    if do_bookmarks {
        match import::import_bookmarks(&browser, &conn, &batch) {
            Ok(s) => {
                combined.bookmarks_imported   += s.bookmarks_imported;
                combined.duplicates_skipped   += s.duplicates_skipped;
                combined.errors.extend(s.errors);
            }
            Err(e) => combined.errors.push(e),
        }
    }

    if do_history {
        match import::import_history(&browser, &conn, &batch, limit_days) {
            Ok(s) => {
                combined.history_imported     += s.history_imported;
                combined.duplicates_skipped   += s.duplicates_skipped;
                combined.errors.extend(s.errors);
            }
            Err(e) => combined.errors.push(e),
        }
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
    artifact_id: String,
    window_minutes: Option<i64>,
) -> Result<Vec<Artifact>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    search::get_context(&conn, &artifact_id, window_minutes.unwrap_or(30))
        .map_err(|e| e.to_string())
}

/// Add or update a user note on an artifact.
#[tauri::command]
fn add_note(
    state: State<AppDb>,
    artifact_id: String,
    note: String,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    search::set_user_note(&conn, &artifact_id, &note)
        .map_err(|e| e.to_string())
}

/// Get overall database statistics for the status bar.
#[tauri::command]
fn get_stats(state: State<AppDb>) -> Result<DbStats, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    search::get_stats(&conn).map_err(|e| e.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 2: Quest commands
// ─────────────────────────────────────────────────────────────────────────────

#[tauri::command]
fn generate_quests(state: State<AppDb>) -> Result<usize, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    quest::generate_quests(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_quests(state: State<AppDb>, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<QuestSummary>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    quest::list_quests(&conn, limit.unwrap_or(20), offset.unwrap_or(0))
        .map_err(|e| e.to_string())
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
fn get_quest_for_artifact(state: State<AppDb>, artifact_id: String) -> Result<Vec<QuestSummary>, String> {
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
            let resolve_db = || -> Result<rusqlite::Connection, Box<dyn std::error::Error>> {
                let app_data_dir = app.path().app_data_dir()?;
                std::fs::create_dir_all(&app_data_dir)?;
                let db_path = app_data_dir.join("recall.db");
                Ok(db::init_db(&db_path)?)
            };

            let conn = resolve_db().unwrap_or_else(|_| {
                let fallback = std::env::current_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from("."))
                    .join(".data");
                std::fs::create_dir_all(&fallback)
                    .expect("Failed to create fallback data directory");
                let db_path = fallback.join("recall.db");
                db::init_db(&db_path).expect("Failed to initialize database (fallback)")
            });

            app.manage(AppDb(Mutex::new(conn)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            search_artifacts,
            detect_browsers,
            import_browser_data,
            get_context,
            add_note,
            get_stats,
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
