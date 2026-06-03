import { useEffect, useState } from 'react';
import type { BrowserInfo, ImportStats } from '../lib/types';
import { detectBrowsers, importBrowserData } from '../lib/commands';

type Step = 'detect' | 'select' | 'importing' | 'done';

interface Props {
  onComplete: () => void;
}

export function ImportWizard({ onComplete }: Props) {
  const [step, setStep]             = useState<Step>('detect');
  const [browsers, setBrowsers]     = useState<BrowserInfo[]>([]);
  const [selected, setSelected]     = useState<string | null>(null);
  const [dataType, setDataType]     = useState<'bookmarks' | 'history' | 'all'>('all');
  const [limitDays, setLimitDays]   = useState<number | null>(null);
  const [stats, setStats]           = useState<ImportStats | null>(null);
  const [error, setError]           = useState<string | null>(null);

  useEffect(() => {
    detectBrowsers().then(bs => {
      setBrowsers(bs);
      setStep('select');
    }).catch(e => setError(String(e)));
  }, []);

  const handleImport = async () => {
    if (!selected) return;
    setStep('importing');
    setError(null);
    try {
      const s = await importBrowserData(selected, dataType, limitDays ?? undefined);
      setStats(s);
      setStep('done');
    } catch (e) {
      setError(String(e));
      setStep('select');
    }
  };

  return (
    <div style={{
      flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center',
      padding: '40px',
    }}>
      <div style={{
        width: '100%', maxWidth: '480px',
        background: 'var(--bg-surface)',
        border: '1px solid var(--border)',
        borderRadius: 'var(--r-xl)',
        padding: '36px',
        animation: 'fadeIn 400ms var(--ease)',
      }}>
        {/* Logo & title */}
        <div style={{ textAlign: 'center', marginBottom: '28px' }}>
          <div style={{
            fontSize: '36px', marginBottom: '8px',
            filter: 'drop-shadow(0 0 20px var(--accent-glow))',
          }}>
            🧠
          </div>
          <h1 style={{ fontSize: '22px', fontWeight: 700, letterSpacing: '-0.02em' }}>
            Welcome to Recall
          </h1>
          <p style={{ color: 'var(--text-secondary)', fontSize: '13px', marginTop: '6px' }}>
            Import your browser data to start finding lost memories
          </p>
        </div>

        {step === 'detect' && (
          <div style={{ textAlign: 'center', color: 'var(--text-muted)' }}>
            <div className="spinner" style={{ margin: '0 auto 12px', width: '24px', height: '24px' }} />
            Detecting installed browsers...
          </div>
        )}

        {step === 'select' && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: '16px' }}>
            {/* Browser selection */}
            <div>
              <label style={{ fontSize: '12px', color: 'var(--text-muted)', fontWeight: 600,
                letterSpacing: '0.06em', textTransform: 'uppercase', display: 'block', marginBottom: '8px' }}>
                Browser
              </label>
              <div style={{ display: 'flex', gap: '8px' }}>
                {browsers.filter(b => b.available).map(b => (
                  <button
                    key={b.id}
                    className={`chip ${selected === b.id ? 'active' : ''}`}
                    style={{ flex: 1, justifyContent: 'center', padding: '10px' }}
                    onClick={() => setSelected(b.id)}
                  >
                    {b.name}
                  </button>
                ))}
                {browsers.filter(b => b.available).length === 0 && (
                  <p style={{ color: 'var(--text-muted)', fontSize: '13px' }}>
                    No supported browsers detected. Check if Edge or Chrome is installed.
                  </p>
                )}
              </div>
            </div>

            {/* Data type */}
            <div>
              <label style={{ fontSize: '12px', color: 'var(--text-muted)', fontWeight: 600,
                letterSpacing: '0.06em', textTransform: 'uppercase', display: 'block', marginBottom: '8px' }}>
                Import
              </label>
              <div style={{ display: 'flex', gap: '8px' }}>
                {(['all', 'bookmarks', 'history'] as const).map(t => (
                  <button
                    key={t}
                    className={`chip ${dataType === t ? 'active' : ''}`}
                    style={{ flex: 1, justifyContent: 'center', padding: '10px' }}
                    onClick={() => setDataType(t)}
                  >
                    {t === 'all' ? 'All' : t === 'bookmarks' ? '★ Bookmarks' : '🕐 History'}
                  </button>
                ))}
              </div>
            </div>

            {/* History limit */}
            {(dataType === 'history' || dataType === 'all') && (
              <div>
                <label style={{ fontSize: '12px', color: 'var(--text-muted)', fontWeight: 600,
                  letterSpacing: '0.06em', textTransform: 'uppercase', display: 'block', marginBottom: '8px' }}>
                  History range
                </label>
                <div style={{ display: 'flex', gap: '8px', flexWrap: 'wrap' }}>
                  {[null, 30, 90, 180, 365].map(d => (
                    <button
                      key={d ?? 'all'}
                      className={`chip ${limitDays === d ? 'active' : ''}`}
                      onClick={() => setLimitDays(d)}
                    >
                      {d === null ? 'All time' : `Last ${d}d`}
                    </button>
                  ))}
                </div>
              </div>
            )}

            {error && (
              <p style={{ fontSize: '12px', color: 'var(--error)', padding: '8px',
                background: 'hsla(355,80%,60%,0.1)', borderRadius: 'var(--r-sm)' }}>
                {error}
              </p>
            )}

            <div style={{ display: 'flex', gap: '8px', marginTop: '4px' }}>
              <button
                className="btn btn-primary"
                style={{ flex: 1 }}
                disabled={!selected}
                onClick={handleImport}
              >
                Import Data
              </button>
              <button
                className="btn btn-ghost"
                onClick={onComplete}
              >
                Skip
              </button>
            </div>
          </div>
        )}

        {step === 'importing' && (
          <div style={{ textAlign: 'center' }}>
            <div className="spinner" style={{ margin: '0 auto 16px', width: '28px', height: '28px',
              borderWidth: '3px' }} />
            <p style={{ color: 'var(--text-secondary)' }}>Importing browser data...</p>
            <p style={{ fontSize: '12px', color: 'var(--text-muted)', marginTop: '6px' }}>
              This may take a moment for large histories
            </p>
          </div>
        )}

        {step === 'done' && stats && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: '16px' }}>
            <div style={{
              background: 'var(--bg-raised)',
              borderRadius: 'var(--r-md)',
              padding: '16px',
              display: 'flex', flexDirection: 'column', gap: '8px',
            }}>
              <StatRow label="Bookmarks imported" value={stats.bookmarks_imported} accent />
              <StatRow label="History imported"   value={stats.history_imported}   accent />
              <StatRow label="Duplicates skipped" value={stats.duplicates_skipped} />
              {stats.errors.length > 0 && (
                <p style={{ fontSize: '11px', color: 'var(--error)' }}>
                  {stats.errors.length} error(s) — check console
                </p>
              )}
            </div>

            <p style={{ fontSize: '13px', color: 'var(--text-secondary)', textAlign: 'center' }}>
              ✓ Your memories are indexed. Start searching!
            </p>

            <button className="btn btn-primary" onClick={onComplete}>
              Start Searching →
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

function StatRow({ label, value, accent }: { label: string; value: number; accent?: boolean }) {
  return (
    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
      <span style={{ fontSize: '13px', color: 'var(--text-secondary)' }}>{label}</span>
      <span style={{
        fontSize: '18px', fontWeight: 700,
        color: accent ? 'var(--accent)' : 'var(--text-primary)',
      }}>
        {value.toLocaleString()}
      </span>
    </div>
  );
}
