import { useEffect, useState } from 'react';
import { getEmbeddingProgress, getStats, prepareEmbeddings } from '../lib/commands';
import type { DbStats, EmbeddingProgress } from '../lib/types';

function formatDate(iso: string | null): string {
  if (!iso) return '—';
  return new Date(iso).toLocaleDateString(undefined, {
    year: 'numeric', month: 'short', day: 'numeric',
  });
}

export function StatsBar() {
  const [stats, setStats] = useState<DbStats | null>(null);
  const [embedding, setEmbedding] = useState<EmbeddingProgress | null>(null);
  const [isPreparing, setIsPreparing] = useState(false);

  useEffect(() => {
    getStats().then(setStats).catch(console.error);
    getEmbeddingProgress().then(setEmbedding).catch(console.error);
  }, []);

  if (!stats) return null;

  const refreshEmbedding = async () => {
    const progress = await getEmbeddingProgress();
    setEmbedding(progress);
  };

  const handlePrepareEmbeddings = async () => {
    setIsPreparing(true);
    try {
      await prepareEmbeddings(32);
      await refreshEmbedding();
    } catch (e) {
      console.error('Embedding preparation error:', e);
    } finally {
      setIsPreparing(false);
    }
  };

  const semanticStatus = embedding
    ? embedding.total === 0
      ? 'Semantic 0'
      : `${embedding.done.toLocaleString()}/${embedding.total.toLocaleString()} semantic`
    : null;
  const canPrepare = Boolean(embedding && embedding.pending > 0 && !isPreparing);

  return (
    <div style={{
      display: 'flex',
      alignItems: 'center',
      gap: '20px',
      padding: '8px 20px',
      borderTop: '1px solid var(--border)',
      background: 'var(--bg-surface)',
      fontSize: '12px',
      color: 'var(--text-muted)',
      flexShrink: 0,
    }}>
      <span>
        <span style={{ color: 'var(--accent)', fontWeight: 600 }}>
          {stats.total_artifacts.toLocaleString()}
        </span>{' '}
        records
      </span>
      <span>·</span>
      <span>
        <span style={{ color: 'var(--text-secondary)' }}>
          {stats.total_bookmarks.toLocaleString()}
        </span>{' '}
        bookmarks
      </span>
      <span>·</span>
      <span>
        <span style={{ color: 'var(--text-secondary)' }}>
          {stats.total_history.toLocaleString()}
        </span>{' '}
        history
      </span>
      {stats.oldest_record && (
        <>
          <span>·</span>
          <span>
            {formatDate(stats.oldest_record)} — {formatDate(stats.newest_record)}
          </span>
        </>
      )}
      <span style={{ flex: 1 }} />
      {stats.last_import && (
        <span>
          Last import: {formatDate(stats.last_import)}
        </span>
      )}
      {embedding && (
        <>
          <span>·</span>
          <span>
            <span style={{ color: embedding.pending === 0 ? 'var(--success)' : 'var(--text-secondary)' }}>
              {semanticStatus}
            </span>
          </span>
          {embedding.pending > 0 && (
            <button
              className="btn btn-ghost"
              style={{
                fontSize: '11px',
                padding: '3px 9px',
                opacity: canPrepare ? 1 : 0.6,
              }}
              disabled={!canPrepare}
              title={`Model: ${embedding.model}`}
              onClick={handlePrepareEmbeddings}
            >
              {isPreparing ? 'Indexing...' : embedding.model_loaded ? 'Index' : 'Enable'}
            </button>
          )}
        </>
      )}
    </div>
  );
}
