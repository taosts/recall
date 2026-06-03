# 🤖 AI Handover Guide for Trae (Project Recall)

> **To the AI Assistant (Trae):** 
> Please read this document carefully before modifying the codebase. This project was scaffolded by an advanced reasoning model. Because you may have different compute constraints, this guide provides explicit instructions, architectural boundaries, and Standard Operating Procedures (SOPs) to help you modify, test, and maintain this project safely and effectively.

---

## 1. Project Context & Philosophy
**Recall** is a "Memory Trigger System," not a standard bookmark manager. 
- **Core Goal:** Help users find information through fuzzy memory (e.g., "that blue network topology map I saw last winter") rather than precise keyword matching.
- **Core Mechanism:** Temporal Context. The app shows what *other* pages the user was browsing at the exact same time they saw the target page.
- **Privacy:** 100% Local-first. SQLite DB. No cloud, no tracking.

## 2. Current Framework & Reading Guide
This is a **Tauri v2 + React (TypeScript) + Rust** application. The core MVP framework is already built: the app successfully extracts data from browsers, indexes it in SQLite, and provides a sleek frontend for fuzzy-memory search.

### 📚 Where to Start (MUST READ)
To quickly understand the project boundaries and data flow, you **must** use your `view_file` tool to read these files first:

1. **`src-tauri/src/models.rs` & `src/lib/types.ts`**
   - **Why:** The single source of truth for our data. They define `Artifact`, `SearchResult`, and how Rust bridges data to React.
2. **`src-tauri/src/db.rs`**
   - **Why:** Look specifically at `init_db`. You need to understand how the `artifacts` table is linked to the `artifacts_fts` (Full Text Search) virtual table via SQLite triggers.
3. **`src/App.tsx`**
   - **Why:** The frontend orchestrator. It shows the routing state (Import Wizard vs. Search view) and how search requests are dispatched to the Rust backend.

### 🔍 Read Later (When you need to modify specific features)
Do **not** waste your context window reading these unless you are tasked to modify them:

- **`src-tauri/src/search.rs`**: Read this if you are tweaking the BM25 search ranking algorithm or modifying how the "Temporal Context" window is calculated.
- **`src-tauri/src/import.rs`**: Read this if you need to support a new browser (like Firefox/Brave) or fix bookmark parsing issues.
- **`src/components/*.tsx`**: The React UI components. Only read the specific component you need to update.
- **`src/styles/index.css`**: Read this when you need to understand our CSS variable design system (no Tailwind!).

---

## 3. Critical Technical Constraints (DO NOT BREAK)

When writing code, you must adhere to these established solutions:

### A. SQLite FTS5 & Triggers
We use SQLite FTS5 for search (`artifacts_fts` table). We do **not** manually insert into the FTS5 table. 
- **Rule:** Insert/Update/Delete operations happen *only* on the `artifacts` table. SQLite `AFTER INSERT`, `AFTER UPDATE`, and `AFTER DELETE` triggers in `db.rs` automatically sync the FTS5 table.

### B. Browser History Access
Chrome and Edge lock their `History` SQLite databases when running.
- **Rule:** You **cannot** query the browser's history file directly. You must use `std::fs::copy` to copy the history file to a temporary directory first, and then open the copy with `rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY`. (See `import.rs`).

### C. Windows Timestamps (FILETIME)
Browser history stores timestamps in Windows `FILETIME` format (microseconds since January 1, 1601).
- **Rule:** To convert this to Unix timestamp, subtract the offset `11,644,473,600` seconds. This logic is already implemented in `import.rs`. Do not reinvent it.

### D. Styling & UI
- **Rule:** Do not install Tailwind CSS unless the user explicitly demands it. Use the existing CSS variables (`var(--bg-surface)`, `var(--accent)`, etc.) in `src/styles/index.css`. Keep the UI dark, sleek, and animation-rich.

---

## 4. Standard Operating Procedure (SOP) for Modification

When the user asks you to implement a new feature or fix a bug, follow this exact workflow:

### Step 1: Types First
If data structures change, update `src-tauri/src/models.rs` (Rust) AND `src/lib/types.ts` (TypeScript) synchronously. They must perfectly match.

### Step 2: Rust Backend implementation
If adding a new Tauri command:
1. Write the function in the relevant `.rs` module.
2. Annotate it with `#[tauri::command]`.
3. Register it in `src-tauri/src/lib.rs` inside `tauri::Builder::default().invoke_handler(tauri::generate_handler![...])`.

### Step 3: Frontend implementation
1. Add the TypeScript wrapper in `src/lib/commands.ts`.
2. Implement the React UI in `src/components/`.

---

## 5. How to Test and Report (Act like a Senior Dev)

Because your compute might be constrained, you must rely on hard compiler checks rather than assuming your code is correct. **Never tell the user a task is done without running these checks.**

### A. The Verification Commands
Before you finish your turn, run these commands using terminal tools to verify your work:

1. **Verify Rust (Backend):**
   ```powershell
   cd src-tauri; cargo check 2>&1
   ```
   *Fix any compiler errors or borrow-checker issues before proceeding.*

2. **Verify TypeScript (Frontend):**
   ```powershell
   npx tsc --noEmit 2>&1
   ```
   *Fix any missing imports or type mismatches before proceeding.*

### B. How to Report to the User
When you complete a task, provide a clean, structured report. Use Markdown checklists to show what was done, and explicitly state the results of your compilation tests.

**Example Report Format:**
```markdown
### Task Completed: [Feature Name]

I have implemented the requested feature. Here is the breakdown:

- [x] **Backend:** Added `new_command` in `search.rs` and registered it in `lib.rs`.
- [x] **Frontend:** Updated `types.ts` and wired the UI in `SearchResults.tsx`.

**Verification:**
- `cargo check` passed (0 errors)
- `tsc --noEmit` passed (0 errors)

To test this locally, please run:
`npm run tauri dev`
```

---

## 6. Common Maintenance Scenarios

### Adding a new Browser to Import
1. Add the browser logic to `import.rs`. Find its default profile path on Windows (e.g., `AppData/Local/BraveSoftware/...`).
2. Add the browser to the `SourceType` enum in both `models.rs` and `types.ts`.
3. Add a CSS badge class in `index.css` (e.g., `.badge-brave { ... }`).

### Modifying the Database Schema
If you add a column to `artifacts`:
1. Update `CREATE TABLE artifacts` in `db.rs`.
2. **Crucial:** Update the FTS5 table creation AND the `AFTER INSERT`/`UPDATE` triggers to include the new column, otherwise search will break.
3. Update `models.rs` and `types.ts`.

### Updating the Context Window Logic
The context window query in `search.rs` uses a self-join or subquery on time bounds. If the user wants to change how "context" is defined (e.g., grouping by domain instead of exact time), look at `get_context` in `search.rs`.

---
*Good luck, Trae! Rely on `cargo check` and `tsc`, follow the data flow from DB -> Rust -> TS -> React, and you will do great.*

---

## 7. Phase 2: Quest System — Your Current Task

The Quest system scaffolding is already in place. Your job is to implement the actual logic inside the scaffolded files.

### What's Already Done For You

| File | Status | What's inside |
|---|---|---|
| `src-tauri/src/quest.rs` | **Scaffolded** | Detailed pseudocode for every function you need to write |
| `src-tauri/src/models.rs` | **Done** | `Quest` and `QuestSummary` structs already added |
| `src-tauri/src/db.rs` | **Done** | `quests` and `quest_artifacts` tables already in schema |
| `src/lib/types.ts` | **Done** | `Quest`, `QuestSummary`, `QuestStatus` types added |
| `src/lib/commands.ts` | **Done** | All Quest command wrappers already written |
| `src/components/QuestPanel.tsx` | **Scaffolded** | Layout spec, interaction patterns, styling guidelines |
| `src/components/QuestBadge.tsx` | **Scaffolded** | Minimal working stub + design spec |

### Execution Order (Follow This Exactly)

**Step 1 — Read `quest.rs` first.** It contains the full algorithm spec.

**Step 2 — Implement Rust functions in `quest.rs`.**
Start with `generate_quests()` (the hardest part), then `list_quests()`, then `get_quest()`. The simpler CRUD functions (`rename_quest`, `archive_quest`, `merge_quests`, `get_quest_for_artifact`) can be done last.

**Step 3 — Wire up `lib.rs`.**
Add `mod quest;` at the top and register all new commands in `generate_handler![]`.

**Step 4 — Run `cargo check`.** Fix all errors before touching the frontend.

**Step 5 — Implement `QuestPanel.tsx`.**
Follow the layout spec in the file comments. Use existing CSS classes (`.card`, `.chip`, `.btn`).

**Step 6 — Update `App.tsx`.**
Add a `'quests'` case to the View type and add a navigation button.

**Step 7 — Run `tsc --noEmit`.** Fix all errors.

**Step 8 — Report to the user.**

### Key Constraint for Quest

> The Quest `quests` and `quest_artifacts` tables do NOT use FTS5.
> Quest search is not needed — users find Quests through artifact search results.
> Do not create FTS5 tables or triggers for Quest data.
