import { useEffect, useState } from 'react';
import { getStats } from '../lib/commands';
import type { DbStats } from '../lib/types';

function formatDate(iso: string | null): string {
  if (!iso) return '—';
  return new Date(iso).toLocaleDateString(undefined, {
    year: 'numeric', month: 'short', day: 'numeric',
  });
}

export function StatsBar() {
  const [stats, setStats] = useState<DbStats | null>(null);

  useEffect(() => {
    getStats().then(setStats).catch(console.error);
  }, []);

  if (!stats) return null;

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
      {stats.last_import && (
        <>
          <span style={{ marginLeft: 'auto' }}>
            Last import: {formatDate(stats.last_import)}
          </span>
        </>
      )}
    </div>
  );
}
