import { useState, useEffect, useCallback } from 'react';
import type { QuestSummary, Quest } from '../lib/types';
import { listQuests, getQuest, generateQuests, renameQuest,
         mergeQuests, archiveQuest } from '../lib/commands';

interface QuestPanelProps {
  onBack: () => void;
}

export function QuestPanel({ onBack }: QuestPanelProps) {
  const [quests, setQuests] = useState<QuestSummary[]>([]);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [expandedQuest, setExpandedQuest] = useState<Quest | null>(null);
  const [isGenerating, setIsGenerating] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editName, setEditName] = useState('');
  const [selectionMode, setSelectionMode] = useState<string | null>(null);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());

  const fetchQuests = useCallback(async () => {
    try {
      const qs = await listQuests(50, 0);
      setQuests(qs);
    } catch (e) {
      console.error('Failed to list quests:', e);
    }
  }, []);

  useEffect(() => { fetchQuests(); }, [fetchQuests]);

  const handleGenerate = async () => {
    setIsGenerating(true);
    try {
      await generateQuests();
      await fetchQuests();
    } catch (e) {
      console.error('Failed to generate quests:', e);
    } finally {
      setIsGenerating(false);
    }
  };

  const handleToggle = async (id: string) => {
    if (expandedId === id) {
      setExpandedId(null);
      setExpandedQuest(null);
      return;
    }
    setExpandedId(id);
    try {
      const q = await getQuest(id);
      setExpandedQuest(q);
    } catch (e) {
      console.error('Failed to get quest:', e);
    }
  };

  const handleRename = async (id: string) => {
    const name = editName.trim();
    if (!name) {
      setEditingId(null);
      return;
    }
    try {
      await renameQuest(id, name);
      await fetchQuests();
    } catch (e) {
      console.error('Failed to rename:', e);
    }
    setEditingId(null);
  };

  const handleArchive = async (id: string) => {
    try {
      await archiveQuest(id);
      if (expandedId === id) {
        setExpandedId(null);
        setExpandedQuest(null);
      }
      await fetchQuests();
    } catch (e) {
      console.error('Failed to archive:', e);
    }
  };

  const handleStartMerge = (id: string) => {
    setSelectionMode(id);
    setSelectedIds(new Set([id]));
  };

  const handleToggleSelect = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const handleConfirmMerge = async () => {
    if (selectedIds.size < 2) {
      setSelectionMode(null);
      setSelectedIds(new Set());
      return;
    }
    try {
      await mergeQuests(Array.from(selectedIds));
      setSelectionMode(null);
      setSelectedIds(new Set());
      await fetchQuests();
    } catch (e) {
      console.error('Failed to merge:', e);
    }
  };

  const formatDate = (iso: string | null): string => {
    if (!iso) return '?';
    try {
      const d = new Date(iso);
      return d.toLocaleDateString('en-US', {
        month: 'short', day: 'numeric', year: 'numeric',
      });
    } catch {
      return iso.slice(0, 10);
    }
  };

  const formatTime = (iso: string | null): string => {
    if (!iso) return '';
    try {
      const d = new Date(iso);
      return d.toLocaleTimeString('en-US', {
        hour: '2-digit', minute: '2-digit',
      });
    } catch {
      return '';
    }
  };

  const statusLabel = (s: string) =>
    s === 'confirmed' ? '✓ Confirmed' : s === 'archived' ? 'Archived' : 'Auto';

  const statusClass = (s: string) =>
    s === 'confirmed' ? 'chip active' : 'chip';

  return (
    <div style={{
      height: '100%',
      display: 'flex',
      flexDirection: 'column',
      background: 'var(--bg-base)',
    }}>
      {/* Header */}
      <div style={{
        display: 'flex',
        alignItems: 'center',
        padding: '16px 20px 12px',
        gap: '12px',
        borderBottom: '1px solid var(--border)',
      }}>
        <button onClick={onBack} className="btn btn-ghost" style={{ fontSize: '12px', padding: '5px 12px' }}>
          ← Back
        </button>
        <span style={{ fontSize: '16px', fontWeight: 600, color: 'var(--text-primary)' }}>
          Quests
        </span>
        <span style={{ flex: 1 }} />
        {selectionMode && (
          <>
            <button onClick={() => { setSelectionMode(null); setSelectedIds(new Set()); }}
              className="btn btn-ghost" style={{ fontSize: '12px', padding: '5px 12px' }}>
              Cancel
            </button>
            <button onClick={handleConfirmMerge}
              className="btn btn-primary" style={{ fontSize: '12px', padding: '5px 14px' }}>
              Merge ({selectedIds.size})
            </button>
          </>
        )}
        {!selectionMode && (
          <button onClick={handleGenerate} disabled={isGenerating}
            className="btn btn-primary" style={{ fontSize: '12px', padding: '5px 14px' }}>
            {isGenerating ? 'Generating...' : 'Generate'}
          </button>
        )}
      </div>

      {/* Quest list */}
      <div style={{ flex: 1, overflowY: 'auto', padding: '16px 20px' }}>
        {quests.length === 0 && (
          <div style={{
            textAlign: 'center',
            padding: '60px 20px',
            color: 'var(--text-muted)',
          }}>
            <p style={{ fontSize: '15px', color: 'var(--text-secondary)', marginBottom: '8px' }}>
              No Quests yet
            </p>
            <p style={{ fontSize: '12px', opacity: 0.6 }}>
              Click "Generate" to auto-create Quests from your browsing history.
            </p>
          </div>
        )}

        {quests.map((q) => (
          <QuestCard
            key={q.id}
            quest={q}
            isExpanded={expandedId === q.id}
            expandedQuest={expandedId === q.id ? expandedQuest : null}
            isEditing={editingId === q.id}
            editName={editName}
            onEditName={setEditName}
            onStartEdit={(id) => setEditingId(id)}
            onCancelEdit={() => setEditingId(null)}
            onRename={handleRename}
            onToggle={handleToggle}
            onArchive={handleArchive}
            onStartMerge={handleStartMerge}
            selectionMode={selectionMode}
            isSelected={selectedIds.has(q.id)}
            onToggleSelect={handleToggleSelect}
            formatDate={formatDate}
            formatTime={formatTime}
            statusLabel={statusLabel}
            statusClass={statusClass}
          />
        ))}
      </div>
    </div>
  );
}

interface QuestCardProps {
  quest: QuestSummary;
  isExpanded: boolean;
  expandedQuest: Quest | null;
  isEditing: boolean;
  editName: string;
  onEditName: (v: string) => void;
  onStartEdit: (id: string) => void;
  onCancelEdit: () => void;
  onRename: (id: string) => void;
  onToggle: (id: string) => void;
  onArchive: (id: string) => void;
  onStartMerge: (id: string) => void;
  selectionMode: string | null;
  isSelected: boolean;
  onToggleSelect: (id: string) => void;
  formatDate: (iso: string | null) => string;
  formatTime: (iso: string | null) => string;
  statusLabel: (s: string) => string;
  statusClass: (s: string) => string;
}

function QuestCard({
  quest, isExpanded, expandedQuest, isEditing, editName,
  onEditName, onStartEdit, onCancelEdit, onRename, onToggle,
  onArchive, onStartMerge, selectionMode, isSelected, onToggleSelect,
  formatDate, formatTime, statusLabel, statusClass,
}: QuestCardProps) {
  const dateRange =
    quest.started_at && quest.ended_at
      ? `${formatDate(quest.started_at)} — ${formatDate(quest.ended_at)}`
      : quest.started_at
        ? formatDate(quest.started_at)
        : 'No date';

  const stats = `${quest.artifact_count} pages · ${quest.anchor_count} bookmarks`;

  return (
    <div className="card" style={{
      marginBottom: '10px',
      padding: '14px 16px',
      cursor: selectionMode ? 'default' : 'pointer',
      borderColor: isSelected ? 'var(--quest-accent)' : undefined,
      background: isSelected ? 'var(--quest-glow)' : undefined,
    }}>
      {/* Header row */}
      <div
        style={{ display: 'flex', alignItems: 'center', gap: '10px' }}
        onClick={() => {
          if (selectionMode) {
            onToggleSelect(quest.id);
          } else {
            onToggle(quest.id);
          }
        }}
      >
        {/* Checkbox in selection mode */}
        {selectionMode && (
          <input
            type="checkbox"
            checked={isSelected}
            onChange={() => onToggleSelect(quest.id)}
            style={{ cursor: 'pointer', accentColor: 'var(--quest-accent)' }}
            onClick={(e) => e.stopPropagation()}
          />
        )}

        {/* Name or edit input */}
        <div style={{ flex: 1, minWidth: 0 }}>
          {isEditing ? (
            <input
              className="input"
              value={editName}
              onChange={(e) => onEditName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') onRename(quest.id);
                if (e.key === 'Escape') onCancelEdit();
              }}
              onBlur={() => onRename(quest.id)}
              autoFocus
              style={{ fontSize: '14px', padding: '4px 10px' }}
              onClick={(e) => e.stopPropagation()}
            />
          ) : (
            <span
              style={{
                fontSize: '14px',
                fontWeight: 600,
                color: 'var(--text-primary)',
                display: 'block',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
              }}
              onDoubleClick={(e) => {
                e.stopPropagation();
                onEditName(quest.display_name);
                onStartEdit(quest.id);
              }}
              title="Double-click to rename"
            >
              {quest.display_name}
            </span>
          )}
        </div>

        {/* Status badge */}
        <span className={statusClass(quest.status)} style={{ fontSize: '11px', padding: '2px 8px' }}>
          {statusLabel(quest.status)}
        </span>

        {/* Expand indicator */}
        <span style={{ color: 'var(--text-muted)', fontSize: '12px' }}>
          {isExpanded ? '▾' : '▸'}
        </span>
      </div>

      {/* Subtitle */}
      <div
        style={{
          display: 'flex',
          gap: '16px',
          marginTop: '6px',
          fontSize: '12px',
          color: 'var(--text-muted)',
        }}
        onClick={() => {
          if (!selectionMode) onToggle(quest.id);
        }}
      >
        <span>{dateRange}</span>
        <span>{stats}</span>
      </div>

      {/* Expanded timeline */}
      {isExpanded && expandedQuest && (
        <div style={{ marginTop: '14px', borderTop: '1px solid var(--border)', paddingTop: '12px' }}>
          {/* Timeline items */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: '8px', marginBottom: '12px' }}>
            {expandedQuest.artifacts.map((a) => (
              <div key={a.id} style={{
                display: 'flex',
                alignItems: 'flex-start',
                gap: '10px',
                fontSize: '12px',
                padding: '6px 8px',
                borderRadius: 'var(--r-sm)',
                background: a.is_bookmarked ? 'var(--accent-glow)' : 'transparent',
              }}>
                {/* Time */}
                <span style={{
                  color: 'var(--text-muted)',
                  fontFamily: 'var(--font-mono)',
                  fontSize: '11px',
                  whiteSpace: 'nowrap',
                  minWidth: '50px',
                }}>
                  {formatTime(a.visited_at)}
                </span>

                {/* Dot */}
                <span style={{
                  width: '6px',
                  height: '6px',
                  borderRadius: '50%',
                  background: a.is_bookmarked ? 'var(--accent)' : 'var(--quest-accent)',
                  marginTop: '5px',
                  flexShrink: 0,
                }} />

                {/* Title + URL */}
                <div style={{ minWidth: 0 }}>
                  <div style={{
                    color: 'var(--text-primary)',
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}>
                    {a.is_bookmarked && <span style={{ color: 'var(--accent)', marginRight: '4px' }}>★</span>}
                    {a.title || a.url || 'Untitled'}
                  </div>
                  {a.domain && (
                    <div style={{ color: 'var(--text-muted)', fontSize: '11px' }}>
                      {a.domain}
                    </div>
                  )}
                </div>
              </div>
            ))}
          </div>

          {/* Actions */}
          <div style={{ display: 'flex', gap: '8px' }}>
            <button onClick={(e) => { e.stopPropagation(); onArchive(quest.id); }}
              className="btn btn-ghost" style={{ fontSize: '11px', padding: '4px 10px' }}>
              Archive
            </button>
            <button onClick={(e) => { e.stopPropagation(); onStartMerge(quest.id); }}
              className="btn btn-ghost" style={{ fontSize: '11px', padding: '4px 10px' }}>
              Merge with...
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
