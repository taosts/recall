// TypeScript types shared between frontend and Rust backend (via Tauri commands)

export type ArtifactType = 'bookmark' | 'history' | 'download' | 'note';
export type SourceType = 'edge' | 'chrome' | 'manual';
export type PageCategory = 'search_query' | 'content' | 'redirect' | 'login' | 'utility';

export interface Artifact {
  id: string;
  type: ArtifactType;
  title: string | null;
  url: string | null;
  domain: string | null;
  created_at: string;
  visited_at: string | null;
  is_bookmarked: boolean;
  visit_count: number;
  source: SourceType | null;
  content: string | null;
  user_note: string | null;
  folder_path: string | null;
  import_batch: string | null;
  page_category: PageCategory | null;
  noise_score: number;
  extracted_query: string | null;
  canonical_url: string | null;
  referrer_domain: string | null;
}

export interface SearchResult {
  artifact: Artifact;
  score: number;
  context: Artifact[];
  // Phase 2: Quest associations (populated when Quest system is active)
  quests?: QuestSummary[];
}

export interface ImportStats {
  browser: string;
  bookmarks_imported: number;
  history_imported: number;
  duplicates_skipped: number;
  errors: string[];
}

export interface BrowserInfo {
  id: string;
  name: string;
  bookmarks_path: string;
  history_path: string;
  available: boolean;
}

export interface DbStats {
  total_artifacts: number;
  total_bookmarks: number;
  total_history: number;
  oldest_record: string | null;
  newest_record: string | null;
  last_import: string | null;
}

export interface NormalizeStats {
  total_scanned: number;
  updated: number;
  search_queries: number;
  redirects: number;
  login_pages: number;
  utility_pages: number;
  high_noise: number;
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 2: Quest (探索任务) types
// ─────────────────────────────────────────────────────────────────────────────

export type QuestStatus = 'auto' | 'confirmed' | 'archived';

/** Full Quest object with its artifacts list. */
export interface Quest {
  id: string;
  name: string | null;
  auto_name: string | null;
  started_at: string | null;
  ended_at: string | null;
  status: QuestStatus;
  created_at: string;
  updated_at: string;
  artifacts: Artifact[];
}

/** Lightweight Quest summary for list views (no artifact details). */
export interface QuestSummary {
  id: string;
  /** Display name: prefers user-set `name`, falls back to `auto_name` */
  display_name: string;
  started_at: string | null;
  ended_at: string | null;
  status: QuestStatus;
  artifact_count: number;
  anchor_count: number;
}

/** Context time window options in minutes */
export type ContextWindow = 15 | 30 | 60 | 120;

export interface SearchFilters {
  dateFrom?: string;
  dateTo?: string;
  source?: SourceType;
  contextMin: ContextWindow;
}
