# Recall

> 帮助用户通过模糊记忆找回曾经接触过的信息，并逐步重建当时的探索过程。

**Recall** is a local-first personal memory trigger system. Instead of asking you to remember exact titles or URLs, it lets you search the way your brain actually works:

- _"去年冬天研究 NAS 时看过的 ZFS 缓存文章"_
- _"that GitHub Actions caching tutorial in English"_
- _"OpenWrt bypass router DNS setup I bookmarked"_

## Core Insight

> Traditional bookmarks save **information**. Recall saves **experience**.

The goal isn't to return a list of links — it's to trigger your memory by showing you the context of when and why you saw something.

## Features (v0.1 MVP)

- **Import** Edge/Chrome bookmarks and browsing history
- **Full-text search** via SQLite FTS5 with BM25 ranking
- **Temporal context** — see what else you were browsing at the same time (configurable: ±15/30/60/120 min)
- **Time filters** — narrow results by week, month, quarter, year
- **Source filters** — Edge or Chrome
- **User notes** — add a quick note to any result ("I used this", "This didn't work because...")
- **Local-first** — all data stays on your machine, no cloud required

## Privacy Principles

```
✓ Local-first: all data stored in ~/.local/share/com.recall.app/recall.db
✓ No cloud sync
✓ No account required
✓ AI is optional (Phase 2, not implemented in v1)
✓ You can delete everything by deleting the app data folder
```

## Tech Stack

| Layer | Technology |
|---|---|
| Frontend | React + TypeScript (Vite) |
| Desktop shell | Tauri v2 |
| Database | SQLite (bundled via rusqlite) with FTS5 |
| Search | BM25 ranking via SQLite FTS5 |
| Backend | Rust |

## Development Setup

**Prerequisites:**
- Rust (via rustup) — `rustc 1.70+`
- Node.js `18+`
- Microsoft Edge or Google Chrome installed

**Run in development:**
```bash
npm install
npm run tauri dev
```

**Build for release:**
```bash
npm run tauri build
```

**Run Rust tests:**
```bash
cd src-tauri && cargo test
```

## Project Structure

```
recall/
├── src/                        # React frontend
│   ├── components/
│   │   ├── SearchBar.tsx       # Search input + filter chips
│   │   ├── SearchResults.tsx   # Results list with expand/note
│   │   ├── ContextPanel.tsx    # "What were you also browsing?" panel
│   │   ├── ImportWizard.tsx    # First-run import flow
│   │   └── StatsBar.tsx        # Bottom status bar
│   ├── lib/
│   │   ├── commands.ts         # Tauri invoke() wrappers
│   │   └── types.ts            # Shared TypeScript types
│   └── styles/index.css        # Design system
├── src-tauri/src/
│   ├── lib.rs                  # Tauri commands + app setup
│   ├── db.rs                   # SQLite init + FTS5 + triggers
│   ├── models.rs               # Rust data structs
│   ├── import.rs               # Chrome/Edge bookmark + history import
│   └── search.rs               # FTS5 search + context queries
└── 回忆系统讨论.txt              # Original design discussion
```

## Future Directions (Phase 2+)

- **Quest** — group related artifacts into exploration tasks automatically
- **AI interpreter** — parse fuzzy natural language into structured queries (local model, opt-in)
- **Page snapshots** — capture page content at visit time (opt-in)
- **Thought map** — visualize topic clusters over time

---

*This is not a bookmark manager. It's not a note-taking app. It's a memory trigger.*
