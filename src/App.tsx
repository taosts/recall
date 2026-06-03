import { useState, useCallback, useEffect } from 'react';
import './styles/index.css';
import { SearchBar }     from './components/SearchBar';
import { SearchResults } from './components/SearchResults';
import { ImportWizard }  from './components/ImportWizard';
import { StatsBar }      from './components/StatsBar';
import { QuestPanel }    from './components/QuestPanel';
import { searchArtifacts, getStats } from './lib/commands';
import type { SearchResult, SearchFilters, ContextWindow } from './lib/types';

type View = 'wizard' | 'search' | 'quests';

function App() {
  const [view, setView]           = useState<View>('search');
  const [results, setResults]     = useState<SearchResult[]>([]);
  const [hasSearched, setHasSearched] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [contextMin, setContextMin] = useState<ContextWindow>(30);
  const [statsKey, setStatsKey]   = useState(0); // bump to refresh StatsBar

  // On first load, check if DB is empty → show wizard
  useEffect(() => {
    getStats().then(s => {
      if (s.total_artifacts === 0) {
        setView('wizard');
      }
    }).catch(() => {
      // DB not ready yet — stay on search view with empty state
    });
  }, []);

  const handleSearch = useCallback(async (query: string, filters: SearchFilters) => {
    setIsLoading(true);
    setHasSearched(true);
    setContextMin(filters.contextMin);
    try {
      const r = await searchArtifacts(query, {
        dateFrom:   filters.dateFrom,
        dateTo:     filters.dateTo,
        source:     filters.source,
        contextMin: filters.contextMin,
      });
      setResults(r);
    } catch (e) {
      console.error('Search error:', e);
      setResults([]);
    } finally {
      setIsLoading(false);
    }
  }, []);

  const handleImportComplete = () => {
    setView('search');
    setStatsKey(k => k + 1); // refresh stats bar
  };

  if (view === 'wizard') {
    return (
      <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
        <ImportWizard onComplete={handleImportComplete} />
      </div>
    );
  }

  if (view === 'quests') {
    return (
      <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
        <QuestPanel onBack={() => setView('search')} />
      </div>
    );
  }

  return (
    <div style={{
      height: '100%',
      display: 'flex',
      flexDirection: 'column',
      background: 'var(--bg-base)',
    }}>
      {/* Top bar with logo + import button */}
      <div style={{
        display: 'flex',
        alignItems: 'center',
        padding: '12px 20px 0',
        gap: '12px',
      }}>
        <span style={{ fontSize: '15px', fontWeight: 700, letterSpacing: '-0.02em' }}>
          <span style={{ color: 'var(--accent)' }}>R</span>ecall
        </span>
        <span style={{ flex: 1 }} />
        <button
          className="btn btn-ghost"
          style={{ fontSize: '12px', padding: '5px 12px' }}
          onClick={() => setView('quests')}
        >
          Quests
        </button>
        <button
          className="btn btn-ghost"
          style={{ fontSize: '12px', padding: '5px 12px' }}
          onClick={() => setView('wizard')}
        >
          ↑ Import
        </button>
      </div>

      {/* Search bar */}
      <SearchBar onSearch={handleSearch} isLoading={isLoading} />

      {/* Results area */}
      <div style={{ flex: 1, overflowY: 'auto', display: 'flex', flexDirection: 'column' }}>
        {!hasSearched ? (
          <EmptyState />
        ) : (
          <SearchResults results={results} contextMin={contextMin} />
        )}
      </div>

      {/* Stats bar */}
      <StatsBar key={statsKey} />
    </div>
  );
}

function EmptyState() {
  return (
    <div style={{
      flex: 1,
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      justifyContent: 'center',
      gap: '16px',
      padding: '40px',
      color: 'var(--text-muted)',
      animation: 'fadeIn 600ms var(--ease)',
    }}>
      <div style={{ fontSize: '48px', opacity: 0.6 }}>🧠</div>
      <p style={{ fontSize: '15px', color: 'var(--text-secondary)' }}>
        What are you trying to remember?
      </p>
      <div style={{
        display: 'flex',
        flexDirection: 'column',
        gap: '6px',
        textAlign: 'center',
        fontSize: '12px',
        opacity: 0.5,
      }}>
        <p>Try: "去年研究 NAS 时看过的 ZFS 文章"</p>
        <p>Or: "that GitHub Actions caching tutorial"</p>
        <p>Or: "OpenWrt bypass router DNS setup"</p>
      </div>
    </div>
  );
}

export default App;
