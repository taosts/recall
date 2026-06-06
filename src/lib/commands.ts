import { invoke } from '@tauri-apps/api/core';
import type {
  SearchResult,
  ImportStats,
  BrowserInfo,
  Artifact,
  DbStats,
  NormalizeStats,
  SourceType,
  ContextWindow,
  Quest,
  QuestSummary,
  EmbeddingProgress,
  EmbeddingRunStats,
} from './types';

/** Search artifacts with optional filters. */
export async function searchArtifacts(
  query: string,
  options?: {
    dateFrom?: string;
    dateTo?: string;
    source?: SourceType;
    contextMin?: ContextWindow;
  }
): Promise<SearchResult[]> {
  return invoke<SearchResult[]>('search_artifacts', {
    query,
    dateFrom: options?.dateFrom ?? null,
    dateTo: options?.dateTo ?? null,
    source: options?.source ?? null,
    contextMin: options?.contextMin ?? 30,
  });
}

/** Detect installed browsers. */
export async function detectBrowsers(): Promise<BrowserInfo[]> {
  return invoke<BrowserInfo[]>('detect_browsers');
}

/** Import bookmarks/history from a browser. */
export async function importBrowserData(
  browser: string,
  dataType: 'bookmarks' | 'history' | 'all',
  limitDays?: number
): Promise<ImportStats> {
  return invoke<ImportStats>('import_browser_data', {
    browser,
    dataType,
    limitDays: limitDays ?? null,
  });
}

/** Get temporal context for an artifact (user-configurable window). */
export async function getContext(
  artifactId: string,
  windowMinutes: ContextWindow = 30
): Promise<Artifact[]> {
  return invoke<Artifact[]>('get_context', {
    artifactId,
    windowMinutes,
  });
}

/** Save a user note on an artifact. */
export async function addNote(artifactId: string, note: string): Promise<void> {
  return invoke<void>('add_note', { artifactId, note });
}

/** Fetch database statistics. */
export async function getStats(): Promise<DbStats> {
  return invoke<DbStats>('get_stats');
}

/** Recompute Phase 3 normalization metadata for all artifacts. */
export async function normalizeArtifacts(): Promise<NormalizeStats> {
  return invoke<NormalizeStats>('normalize_artifacts');
}

/** Get semantic embedding progress without loading/downloading the model. */
export async function getEmbeddingProgress(): Promise<EmbeddingProgress> {
  return invoke<EmbeddingProgress>('get_embedding_progress');
}

/** Explicitly prepare a batch of local embeddings. May download the model first. */
export async function prepareEmbeddings(batchSize: number = 32): Promise<EmbeddingRunStats> {
  return invoke<EmbeddingRunStats>('prepare_embeddings', { batchSize });
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 2: Quest commands
// These are the frontend wrappers for the Quest Tauri commands.
// They will fail at runtime until quest.rs is fully implemented and
// the commands are registered in lib.rs.
// ─────────────────────────────────────────────────────────────────────────────

/** Run the auto-clustering algorithm. Returns number of Quests created. */
export async function generateQuests(): Promise<number> {
  return invoke<number>('generate_quests');
}

/** List Quests with pagination, ordered by most recent first. */
export async function listQuests(
  limit: number = 20,
  offset: number = 0
): Promise<QuestSummary[]> {
  return invoke<QuestSummary[]>('list_quests', { limit, offset });
}

/** Get a full Quest including its artifact list. */
export async function getQuest(questId: string): Promise<Quest> {
  return invoke<Quest>('get_quest', { questId });
}

/** User renames a Quest. Also sets status to 'confirmed'. */
export async function renameQuest(questId: string, name: string): Promise<void> {
  return invoke<void>('rename_quest', { questId, name });
}

/** Merge multiple Quests into one. Returns the surviving Quest's ID. */
export async function mergeQuests(questIds: string[]): Promise<string> {
  return invoke<string>('merge_quests', { questIds });
}

/** Archive a Quest (soft-delete). */
export async function archiveQuest(questId: string): Promise<void> {
  return invoke<void>('archive_quest', { questId });
}
