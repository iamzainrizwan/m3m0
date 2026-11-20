# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Context for Claude Code sessions working on m3m0. Read this, then the doc in
`docs/` for whatever you're touching.

## What m3m0 is

A native RemNote client for Zain, replacing the RemNote desktop app's laggy
Electron UI. Rust + egui (eframe 0.31). RemNote itself keeps running hidden as
the sync engine (its cloud sync only exists inside its app); m3m0 is the UI.

Six features, all in scope (`docs/plan.md` has phases and status):
1. outliner editor (view + edit notes), vim-style, keyboard-first
2. graph view of notes (like the Obsidian graph he used via remnote_vault.py)
3. a week files pane: every file for the open week (README with followable
   links, quiz JSON with answers on `a`, markdown, PDFs, images, text)
4. Claude flashcards (`uni-git/<module>/revision/week-NN-flashcards.md`) next
   to his own cards, gaps highlighted, keybind to link a rem to a Claude card
5. Claude chat panel (`claude -p`, whole-module context; may edit only
   Claude's files under `<module>/revision/`, never his rems)
6. flashcard review with RemNote's default scheduler, written back to RemNote

Every decision and the reason for it: `docs/design.md`.

## Data flow (read vs write)

Reads: `kb::Kb::load` takes a SQLite backup-API snapshot of RemNote's local DB
(`~/remnote/*/remnote.db`, picked with `:db`), so it's safe while RemNote runs.
Writes never touch the DB: edits are staged as ops in `edit.rs` (undo/redo,
`:diff` to inspect), and `:w` hands them to `commit.rs`, which sends them over
the bridge. m3m0 is the server (`ws://127.0.0.1:47830`; it also serves
`plugin/dist` on `:47831` for RemNote's "develop from localhost"); the plugin
connects and applies ops through the RemNote SDK so RemNote owns sync stamps.
Tmp ids for created rems are mapped to real ids on reply. Protocol and op
table: `docs/bridge.md`. Review grades go the same way (`review` op).

## Layout

- `src/main.rs`: app state, focus model, normal-mode keys, panels (tree,
  notes, pdf, chat), overlays (help, find, db, diff, log, link picker),
  deferred actions, preview hooks, `roundtrip` / `commit-test` subcommands
- `src/keys.rs`: **every key** (drives the `?` reference, which-key popup and
  status hints). Add new keys here.
- `src/kb.rs`: read model of the RemNote DB (snapshot via SQLite backup API),
  `line_of`, `resolve`, card docs, DB discovery
- `src/rich.rs`: rich text <-> spans <-> editable deck syntax (lossless;
  `m3m0 roundtrip` must report 0 differ over the whole DB)
- `src/render.rs`: own line layout (per-row baselines), inline KaTeX, refs
- `src/math.rs`: RaTeX -> texture cache (KaTeX fonts, 1.21em, baseline-aligned)
- `src/edit.rs`: staged ops (Create/Update/Delete/Move), undo/redo (groups
  = one `u`), staged ops saved to `~/.local/state/m3m0/staged.json`
- `src/editor.rs`: insert mode, vim inside the line (esc), V over bullets,
  registers (yy/dd/p), `.`, split/join, structure ops, `[[` / `/` menus
- `src/typing.rs`: what typed keys do (auto-pairs, `==`, marker spacing,
  format keys, `/` commands, line motions); pure functions with tests
- `src/commit.rs`: ops -> bridge calls, tmp id mapping, commit log
- `src/bridge.rs`: plugin HTTP server + websocket worker, RemNote launch/stop
- `src/pdf.rs`: week files pane: file discovery (`week_files`), hayro PDF
  worker, link following (`f` hints, `b` back), non-PDF scrolling
- `src/files.rs`: renderers: quiz JSON (+ answers), markdown (+ links),
  JSON, images, text; `latex_text` converts LaTeX text mode (quiz strings)
- `src/graph.rs`: graph view (pages/terms/citations, m3m0 look)
- `src/deck.rs` + `src/cards.rs`: Claude decks, pairing, links, cards view
- `src/chat.rs`: `claude -p` chat panel
- `src/review.rs`: due queue + grading through the bridge
- `src/inhibit.rs`: GNOME logout inhibitor while anything is uncommitted
- `src/boot.rs`, `src/icon.rs`, `src/theme.rs`, `src/config.rs`
- `plugin/`: the RemNote plugin (TypeScript)
- `scripts/preview.sh`: frames on the secondary monitor (XWayland)
- `spikes/`: phase 0 experiments; `docs/`: design, plan, storage, bridge

## Build, run, preview

```
cargo build --release && ./target/release/m3m0
./target/release/m3m0 --install                # icon + .desktop entry
cd plugin && npm install && npm run build      # plugin/dist + PluginZip.zip
```

Checks (no lint config; plain `cargo clippy` if wanted):

```
cargo test                                     # unit tests (rich.rs, files.rs)
cargo test rich::tests::typed_markup           # one test
./target/release/m3m0 roundtrip                # must report 0 differ over the whole DB
./target/release/m3m0 commit-test --keep       # write path on a scratch tree
./target/release/m3m0 bridge-test --cleanup ID # delete that scratch tree
cd plugin && npm run check-types               # tsc
```

Dev env vars: `M3M0_DB` (db path), `M3M0_OPEN="FC2/Week 8"` (open a week),
`M3M0_KEYS` (replay keys after boot; `~b` = ctrl-b, newline = enter,
`\x1b` = esc, `\x08` = backspace, `|` = pause ~0.6 s so the UI can draw before the next keys),
`M3M0_SHOTS=DIR` (save frames at fixed times, then quit; real keys ignored),
`M3M0_DEBUG` (log keys), `M3M0_PLUGIN_DIST`.

**Show Zain the result, don't describe it.** Use `scripts/preview.sh OUT
[MODULE/Week N] [KEYS]`: a laptop-sized window on the secondary monitor,
never his main display; previews never read or write his config.
`M3M0_LAST=40` delays the last frame (chat answers), `M3M0_OPEN_ID` opens any
rem as the page. Previews go in `~/Documents/claude/m3m0/`.
**Never replay grade keys (1-4) in review or `:w` on real notes**; test
writes on a scratch tree (`m3m0 commit-test --keep`, then
`m3m0 bridge-test --cleanup ID`).

## Rules

- **Data safety first.** Reads only ever go through `kb::Kb::load` (snapshot).
  Never write the SQLite DB while RemNote runs; writes go through the plugin
  bridge. Bridge tests create a dated scratch tree and delete it after
  (`m3m0 bridge-test --cleanup ID`); never touch real notes in tests.
- **Laptop first.** Zain uses a 14" panel, 2560x1600 at GNOME scale 1.5
  (~1706x1066 points), most of the time. Text must be readable there without
  zooming (he complained it was tiny). Zoom is persisted (ctrl +/-).
- **Keyboard-first.** "vim" means the whole app is drivable from the home row:
  every new panel or overlay gets j/k/h/l, a focus model, and entries in the
  `?` help overlay. Mouse works too but is secondary.
- **Look**: rice. All black, BlexMono, red accents (`RED`, `RED_BRIGHT`,
  `RED_CURSOR`), no green except "done", square corners, no shadows. Our own
  titlebar (window decorations are off). Explicit alignment pass on any UI.
- **Git**: commit straight to main and push, at natural checkpoints, short
  verb-first messages. Public repo, but Claude commits freely. No Claude
  attribution anywhere (no Co-Authored-By, no "generated with").
- Uni content rule: no Scala material until his PEP C++ coursework is in (don't
  open `pep/**/scala*` even as test data).
- Every key lives in `src/keys.rs` (the `?` help is tabbed by section,
  opened on the current pane's section; which-key covers prefixes). Add any
  new key there, or it's undiscoverable.
- He launches from the app grid, where GNOME gives no shell PATH (no
  ~/.local/bin). Resolve external tools explicitly (see `chat::claude_bin`);
  test with `PATH=/usr/bin:/bin`.
- Ask before adding features he didn't list. Lowercase casual voice for any
  user-facing copy.
