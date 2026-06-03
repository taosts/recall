import { useState, useRef, useEffect } from 'react';
import type { ContextWindow, SearchFilters, SourceType } from '../lib/types';

const PLACEHOLDERS = [
  '去年研究 NAS 时看过的 ZFS 缓存文章...',
  '当时为了解决磁盘 100% 找到过的方案...',
  '收藏过的 OpenWrt 旁路由配置教程...',
  '那篇讲 GitHub Actions 缓存优化的英文长文...',
  '我记得有一张蓝色的网络拓扑图...',
];

const TIME_CHIPS: { label: string; days: number | null }[] = [
  { label: 'All',   days: null },
  { label: '1Y',    days: 365 },
  { label: '3M',    days: 90 },
  { label: '1M',    days: 30 },
  { label: '1W',    days: 7 },
];

const SOURCE_CHIPS: { label: string; value: SourceType | undefined }[] = [
  { label: 'All',    value: undefined },
  { label: 'Edge',   value: 'edge' },
  { label: 'Chrome', value: 'chrome' },
];

const CONTEXT_WINDOWS: ContextWindow[] = [15, 30, 60, 120];

interface Props {
  onSearch: (query: string, filters: SearchFilters) => void;
  isLoading: boolean;
}

function daysBefore(days: number): string {
  const d = new Date();
  d.setDate(d.getDate() - days);
  return d.toISOString();
}

export function SearchBar({ onSearch, isLoading }: Props) {
  const [query, setQuery] = useState('');
  const [placeholderIdx, setPlaceholderIdx] = useState(0);
  const [timeDays, setTimeDays] = useState<number | null>(null);
  const [source, setSource] = useState<SourceType | undefined>(undefined);
  const [contextMin, setContextMin] = useState<ContextWindow>(30);
  const inputRef = useRef<HTMLInputElement>(null);

  // Cycle through fuzzy-memory placeholder examples
  useEffect(() => {
    const id = setInterval(() => {
      setPlaceholderIdx(i => (i + 1) % PLACEHOLDERS.length);
    }, 3500);
    return () => clearInterval(id);
  }, []);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const handleSubmit = (e?: React.FormEvent) => {
    e?.preventDefault();
    if (!query.trim()) return;
    onSearch(query, {
      dateFrom: timeDays ? daysBefore(timeDays) : undefined,
      source,
      contextMin,
    });
  };

  return (
    <div style={{
      padding: '28px 28px 16px',
      flexShrink: 0,
    }}>
      {/* Main search input */}
      <form onSubmit={handleSubmit} style={{ position: 'relative' }}>
        <div style={{
          position: 'relative',
          display: 'flex',
          alignItems: 'center',
        }}>
          {/* Search icon */}
          <svg style={{
            position: 'absolute', left: '16px',
            color: 'var(--text-muted)', pointerEvents: 'none',
            flexShrink: 0,
          }} width="18" height="18" viewBox="0 0 24 24" fill="none"
            stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/>
          </svg>

          <input
            ref={inputRef}
            id="recall-search-input"
            type="text"
            value={query}
            onChange={e => setQuery(e.target.value)}
            placeholder={PLACEHOLDERS[placeholderIdx]}
            autoComplete="off"
            spellCheck={false}
            style={{
              width: '100%',
              background: 'var(--bg-raised)',
              border: '1px solid var(--border)',
              borderRadius: 'var(--r-lg)',
              color: 'var(--text-primary)',
              fontFamily: 'var(--font-sans)',
              fontSize: '15px',
              padding: '13px 52px 13px 46px',
              outline: 'none',
              transition: 'border-color 150ms, box-shadow 150ms',
            }}
            onFocus={e => {
              e.currentTarget.style.borderColor = 'var(--accent-border)';
              e.currentTarget.style.boxShadow = '0 0 0 3px var(--accent-glow)';
            }}
            onBlur={e => {
              e.currentTarget.style.borderColor = 'var(--border)';
              e.currentTarget.style.boxShadow = 'none';
            }}
          />

          {/* Spinner or search button */}
          <div style={{ position: 'absolute', right: '14px' }}>
            {isLoading ? (
              <div className="spinner" />
            ) : (
              <button
                type="submit"
                style={{
                  background: query.trim() ? 'var(--accent)' : 'transparent',
                  border: 'none',
                  borderRadius: 'var(--r-sm)',
                  color: query.trim() ? '#0a0a0f' : 'var(--text-muted)',
                  cursor: query.trim() ? 'pointer' : 'default',
                  padding: '4px 8px',
                  fontSize: '12px',
                  fontWeight: 600,
                  fontFamily: 'var(--font-sans)',
                  transition: 'all 150ms',
                }}
              >
                ↵
              </button>
            )}
          </div>
        </div>
      </form>

      {/* Filter chips row */}
      <div style={{
        display: 'flex',
        alignItems: 'center',
        gap: '8px',
        marginTop: '12px',
        flexWrap: 'wrap',
      }}>
        {/* Time filter */}
        <span style={{ fontSize: '12px', color: 'var(--text-muted)', marginRight: '2px' }}>
          Period:
        </span>
        {TIME_CHIPS.map(chip => (
          <button
            key={chip.label}
            className={`chip ${timeDays === chip.days ? 'active' : ''}`}
            onClick={() => setTimeDays(chip.days)}
          >
            {chip.label}
          </button>
        ))}

        <div style={{ width: '1px', height: '16px', background: 'var(--border)', margin: '0 4px' }} />

        {/* Source filter */}
        <span style={{ fontSize: '12px', color: 'var(--text-muted)', marginRight: '2px' }}>
          Source:
        </span>
        {SOURCE_CHIPS.map(chip => (
          <button
            key={chip.label}
            className={`chip ${source === chip.value ? 'active' : ''}`}
            onClick={() => setSource(chip.value)}
          >
            {chip.label}
          </button>
        ))}

        <div style={{ width: '1px', height: '16px', background: 'var(--border)', margin: '0 4px' }} />

        {/* Context window */}
        <span style={{ fontSize: '12px', color: 'var(--text-muted)', marginRight: '2px' }}>
          Context:
        </span>
        {CONTEXT_WINDOWS.map(w => (
          <button
            key={w}
            className={`chip ${contextMin === w ? 'active' : ''}`}
            onClick={() => setContextMin(w)}
            title={`Show pages visited within ±${w} minutes`}
          >
            {w}m
          </button>
        ))}
      </div>
    </div>
  );
}
