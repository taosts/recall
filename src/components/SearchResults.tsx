import { useState } from 'react';
import type { SearchResult, ContextWindow } from '../lib/types';
import { addNote } from '../lib/commands';
import { ContextPanel } from './ContextPanel';
import { QuestBadge } from './QuestBadge';

interface Props {
  results: SearchResult[];
  contextMin: ContextWindow;
}

export function SearchResults({ results, contextMin }: Props) {
  if (results.length === 0) {
    return (
      <div style={{
        flex: 1, display: 'flex', flexDirection: 'column',
        alignItems: 'center', justifyContent: 'center',
        color: 'var(--text-muted)', gap: '12px',
      }}>
        <svg width="40" height="40" viewBox="0 0 24 24" fill="none"
          stroke="currentColor" strokeWidth="1.5" opacity="0.3">
          <circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/>
        </svg>
        <p style={{ fontSize: '14px' }}>No memories found</p>
        <p style={{ fontSize: '12px', opacity: 0.6 }}>
          Try different keywords or expand the time range
        </p>
      </div>
    );
  }

  return (
    <div style={{
      flex: 1,
      overflowY: 'auto',
      padding: '0 20px 20px',
      display: 'flex',
      flexDirection: 'column',
      gap: '8px',
    }}>
      <p style={{
        fontSize: '12px', color: 'var(--text-muted)',
        padding: '4px 0 8px',
      }}>
        {results.length} result{results.length !== 1 ? 's' : ''}
      </p>
      {results.map((result, i) => (
        <ResultCard
          key={result.artifact.id}
          result={result}
          contextMin={contextMin}
          style={{ animationDelay: `${i * 30}ms` }}
        />
      ))}
    </div>
  );
}

interface CardProps {
  result: SearchResult;
  contextMin: ContextWindow;
  style?: React.CSSProperties;
}

function ResultCard({ result, contextMin, style }: CardProps) {
  const { artifact } = result;
  const [expanded, setExpanded] = useState(false);
  const [editingNote, setEditingNote] = useState(false);
  const [note, setNote] = useState(artifact.user_note ?? '');
  const [saving, setSaving] = useState(false);

  const handleSaveNote = async () => {
    setSaving(true);
    try {
      await addNote(artifact.id, note);
      setEditingNote(false);
    } catch (e) {
      console.error(e);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      className="card animate-fade-in"
      style={{ padding: '14px 16px', cursor: 'default', ...style }}
    >
      {/* Top row: title + badges */}
      <div style={{ display: 'flex', alignItems: 'flex-start', gap: '10px' }}>
        {/* Favicon placeholder */}
        <div style={{
          width: '18px', height: '18px',
          borderRadius: '4px',
          background: 'var(--bg-hover)',
          flexShrink: 0,
          marginTop: '2px',
          display: 'flex', alignItems: 'center', justifyContent: 'center',
          fontSize: '10px',
        }}>
          {artifact.is_bookmarked ? '★' : '·'}
        </div>

        <div style={{ flex: 1, minWidth: 0 }}>
          {/* Title */}
          <a
            href={artifact.url ?? '#'}
            onClick={e => {
              e.preventDefault();
              if (artifact.url) window.open(artifact.url, '_blank');
            }}
            style={{
              fontSize: '14px',
              fontWeight: 500,
              color: 'var(--text-primary)',
              display: 'block',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
              textDecoration: 'none',
            }}
            title={artifact.title ?? artifact.url ?? ''}
          >
            {artifact.title || artifact.url || '(No title)'}
          </a>

          {/* URL */}
          <span style={{
            fontSize: '11px',
            color: 'var(--text-muted)',
            fontFamily: 'var(--font-mono)',
            display: 'block',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}>
            {artifact.url}
          </span>
        </div>

        {/* Badges */}
        <div style={{ display: 'flex', gap: '4px', flexShrink: 0, alignItems: 'center' }}>
          {artifact.source && (
            <span className={`chip badge-${artifact.source}`}
              style={{ fontSize: '11px', padding: '2px 8px', cursor: 'default' }}>
              {artifact.source}
            </span>
          )}
          {artifact.is_bookmarked && (
            <span className="chip badge-bookmark"
              style={{ fontSize: '11px', padding: '2px 8px', cursor: 'default' }}>
              ★
            </span>
          )}
          {artifact.extracted_query && (
            <span className="chip"
              style={{ fontSize: '11px', padding: '2px 8px', cursor: 'default' }}
              title="Search query extracted from browser history">
              Query: {artifact.extracted_query}
            </span>
          )}
          {artifact.noise_score >= 0.6 && (
            <span className="chip"
              style={{ fontSize: '11px', padding: '2px 8px', cursor: 'default', color: 'var(--text-muted)' }}
              title={`Noise score ${artifact.noise_score.toFixed(2)}`}>
              noisy
            </span>
          )}
          {result.quests && result.quests.length > 0 && (
            <QuestBadge quests={result.quests} />
          )}
        </div>
      </div>

      {/* Meta row */}
      <div style={{
        display: 'flex', alignItems: 'center', gap: '10px',
        marginTop: '8px', fontSize: '11px', color: 'var(--text-muted)',
      }}>
        {artifact.visited_at && (
          <span>{new Date(artifact.visited_at).toLocaleDateString(undefined, {
            year: 'numeric', month: 'short', day: 'numeric',
          })}</span>
        )}
        {artifact.visit_count > 0 && (
          <>
            <span>·</span>
            <span>{artifact.visit_count} visit{artifact.visit_count !== 1 ? 's' : ''}</span>
          </>
        )}
        {artifact.folder_path && (
          <>
            <span>·</span>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: '10px' }}>
              📁 {artifact.folder_path}
            </span>
          </>
        )}

        {/* Actions */}
        <div style={{ marginLeft: 'auto', display: 'flex', gap: '6px' }}>
          <button
            className="chip"
            style={{ fontSize: '11px', padding: '2px 8px' }}
            onClick={() => setEditingNote(!editingNote)}
            title="Add/edit note"
          >
            {note ? '✏️ Note' : '+ Note'}
          </button>
          <button
            className={`chip ${expanded ? 'active' : ''}`}
            style={{ fontSize: '11px', padding: '2px 8px' }}
            onClick={() => setExpanded(!expanded)}
            title="Show temporal context"
          >
            {expanded ? '▲ Context' : '▼ Context'}
            {result.context.length > 0 && (
              <span style={{
                background: 'var(--accent-glow)',
                color: 'var(--accent)',
                borderRadius: '999px',
                padding: '0 5px',
                fontSize: '10px',
                marginLeft: '2px',
              }}>
                {result.context.length}
              </span>
            )}
          </button>
        </div>
      </div>

      {/* Note editor */}
      {editingNote && (
        <div style={{ marginTop: '10px' }}>
          <textarea
            className="input"
            value={note}
            onChange={e => setNote(e.target.value)}
            placeholder="Why did you save this? Did it help? What did you decide?"
            style={{ fontSize: '13px', minHeight: '64px' }}
          />
          <div style={{ display: 'flex', gap: '6px', marginTop: '6px' }}>
            <button className="btn btn-primary" style={{ fontSize: '12px', padding: '6px 14px' }}
              onClick={handleSaveNote} disabled={saving}>
              {saving ? 'Saving...' : 'Save'}
            </button>
            <button className="btn btn-ghost" style={{ fontSize: '12px', padding: '6px 14px' }}
              onClick={() => { setEditingNote(false); setNote(artifact.user_note ?? ''); }}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {/* Existing note display */}
      {!editingNote && note && (
        <div style={{
          marginTop: '8px',
          padding: '8px 10px',
          background: 'var(--accent-glow)',
          borderRadius: 'var(--r-sm)',
          borderLeft: '2px solid var(--accent)',
          fontSize: '12px',
          color: 'var(--text-secondary)',
          fontStyle: 'italic',
        }}>
          {note}
        </div>
      )}

      {/* Context panel */}
      {expanded && (
        <ContextPanel
          artifactId={artifact.id}
          visitedAt={artifact.visited_at}
          initialWindow={contextMin}
        />
      )}
    </div>
  );
}
