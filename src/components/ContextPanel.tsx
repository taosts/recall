import { useState } from 'react';
import type { Artifact, ContextWindow } from '../lib/types';
import { getContext } from '../lib/commands';

const CONTEXT_WINDOWS: ContextWindow[] = [15, 30, 60, 120];

function formatRelativeTime(iso: string | null): string {
  if (!iso) return '';
  const diff = Date.now() - new Date(iso).getTime();
  const mins  = Math.floor(diff / 60000);
  const hours = Math.floor(mins / 60);
  const days  = Math.floor(hours / 24);
  const months= Math.floor(days / 30);
  const years = Math.floor(days / 365);

  if (years  > 0) return `${years}y ago`;
  if (months > 0) return `${months}mo ago`;
  if (days   > 0) return `${days}d ago`;
  if (hours  > 0) return `${hours}h ago`;
  if (mins   > 0) return `${mins}m ago`;
  return 'just now';
}

interface Props {
  artifactId: string;
  visitedAt: string | null;
  initialWindow: ContextWindow;
}

export function ContextPanel({ artifactId, visitedAt, initialWindow }: Props) {
  const [windowMin, setWindowMin] = useState<ContextWindow>(initialWindow);
  const [items, setItems]         = useState<Artifact[] | null>(null);
  const [loading, setLoading]     = useState(false);

  const load = async (w: ContextWindow) => {
    setWindowMin(w);
    setLoading(true);
    try {
      const ctx = await getContext(artifactId, w);
      setItems(ctx);
    } catch (e) {
      console.error(e);
      setItems([]);
    } finally {
      setLoading(false);
    }
  };

  // Load on first render
  if (items === null && !loading) {
    load(windowMin);
  }

  return (
    <div style={{
      marginTop: '12px',
      padding: '14px',
      background: 'var(--bg-base)',
      borderRadius: 'var(--r-md)',
      border: '1px solid var(--border)',
      animation: 'fadeIn 200ms var(--ease)',
    }}>
      {/* Header */}
      <div style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        marginBottom: '10px',
        gap: '8px',
        flexWrap: 'wrap',
      }}>
        <span style={{
          fontSize: '11px',
          fontWeight: 600,
          letterSpacing: '0.08em',
          textTransform: 'uppercase',
          color: 'var(--accent)',
        }}>
          ⏱ When you visited this, you also saw:
        </span>

        {/* Context window selector */}
        <div style={{ display: 'flex', gap: '4px' }}>
          {CONTEXT_WINDOWS.map(w => (
            <button
              key={w}
              className={`chip ${windowMin === w ? 'active' : ''}`}
              style={{ fontSize: '11px', padding: '2px 8px' }}
              onClick={() => load(w)}
              title={`±${w} minute window`}
            >
              ±{w}m
            </button>
          ))}
        </div>
      </div>

      {visitedAt && (
        <p style={{ fontSize: '11px', color: 'var(--text-muted)', marginBottom: '10px' }}>
          You visited this {formatRelativeTime(visitedAt)}{' '}
          ({new Date(visitedAt).toLocaleString()})
        </p>
      )}

      {/* Content */}
      {loading ? (
        <div style={{ display: 'flex', gap: '8px', alignItems: 'center', padding: '8px 0' }}>
          <div className="spinner" />
          <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>
            Loading context...
          </span>
        </div>
      ) : items && items.length === 0 ? (
        <p style={{ fontSize: '12px', color: 'var(--text-muted)', fontStyle: 'italic' }}>
          No other pages found in this time window. Try ±60m or ±120m.
        </p>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
          {items?.map(item => (
            <ContextItem key={item.id} artifact={item} />
          ))}
        </div>
      )}
    </div>
  );
}

function ContextItem({ artifact }: { artifact: Artifact }) {
  const isBookmark = artifact.is_bookmarked;

  return (
    <div style={{
      display: 'flex',
      alignItems: 'center',
      gap: '10px',
      padding: '6px 8px',
      borderRadius: 'var(--r-sm)',
      transition: 'background 120ms',
    }}
    onMouseEnter={e => (e.currentTarget.style.background = 'var(--bg-hover)')}
    onMouseLeave={e => (e.currentTarget.style.background = 'transparent')}
    >
      {/* Dot */}
      <div style={{
        width: '6px', height: '6px',
        borderRadius: '50%',
        flexShrink: 0,
        background: isBookmark ? 'var(--accent)' : 'var(--text-muted)',
      }} />

      {/* Time */}
      <span style={{
        fontSize: '11px',
        color: 'var(--text-muted)',
        fontFamily: 'var(--font-mono)',
        flexShrink: 0,
        minWidth: '60px',
      }}>
        {artifact.visited_at
          ? new Date(artifact.visited_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
          : '??:??'}
      </span>

      {/* Title / URL */}
      <a
        href={artifact.url ?? '#'}
        onClick={e => {
          e.preventDefault();
          if (artifact.url) window.open(artifact.url, '_blank');
        }}
        style={{
          fontSize: '12px',
          color: 'var(--text-secondary)',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
          flex: 1,
        }}
        title={artifact.url ?? ''}
      >
        {artifact.title || artifact.domain || artifact.url || 'Unknown'}
      </a>

      {/* Bookmark icon */}
      {isBookmark && (
        <span style={{ color: 'var(--accent)', fontSize: '11px', flexShrink: 0 }}>★</span>
      )}
    </div>
  );
}
