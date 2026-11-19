# CLAUDE.md

Context for Claude Code sessions working on m3m0. Read this, then the doc in
`docs/` for whatever you're touching.

## What m3m0 is

A native RemNote client for Zain, replacing the RemNote desktop app's laggy
Electron UI. Rust + egui (eframe 0.31). RemNote itself keeps running hidden as
the sync engine (its cloud sync only exists inside its app); m3m0 is the UI.

Six features, all in scope (`docs/plan.md` has phases and status):
1. outliner editor (view + edit notes), vim-style, keyboard-first
2. graph view of notes (like the Obsidian graph he used via remnote_vault.py)
3. weekly PDFs for a module (built-in viewer, side by side with notes)
4. Claude flashcards (`uni-git/<module>/revision/week-NN-flashcards.md`) next
   to his own cards, gaps highlighted, keybind to link a rem to a Claude card
5. Claude chat panel (`claude -p`, whole-module context; may edit only
   Claude's files under `<module>/revision/`, never his rems)
6. flashcard review with RemNote's default scheduler, written back to RemNote

Every decision and the reason for it: `docs/design.md`.

## Layout

- `src/main.rs`: app state, keys (vim nav), panels, overlays, preview hooks
- `src/kb.rs`: read model of the RemNote DB (snapshot via SQLite backup API),
  DB discovery (`find_dbs`, `resolve_db`)
- `src/boot.rs`: boot animation + the dot-matrix logo (`GLYPHS`)
- `src/icon.rs`: app icon drawn from `GLYPHS`; `m3m0 --install` writes the icon
  and `~/.local/share/applications/m3m0.desktop`
- `src/theme.rs`: rice palette (`~/rice/palette.md`) and BlexMono fonts
- `src/config.rs`: `~/.config/m3m0/config.json` (zoom, sidebar, db)
- `src/bridge.rs`: websocket server + plugin file server for the RemNote
  plugin; `m3m0 bridge-test` spike
- `plugin/`: the RemNote plugin (TypeScript, from remnote's react template)
- `spikes/`: phase 0 experiments (math rendering, PDF rendering); standalone
  crates, not workspace members
- `docs/`: design, plan, RemNote storage notes, bridge protocol, spike results

## Build, run, preview

```
cargo build --release && ./target/release/m3m0
cd plugin && npm install && npm run build      # plugin/dist + PluginZip.zip
```

Dev env vars: `M3M0_DB` (db path), `M3M0_OPEN="FC2/Week 8"` (open a week),
`M3M0_KEYS` (replay keys after boot; `~b` = ctrl-b, newline = enter),
`M3M0_SHOTS=DIR` (save frames at fixed times, then quit; real keys ignored),
`M3M0_DEBUG` (log keys), `M3M0_PLUGIN_DIST`.

**Show Zain the result, don't describe it.** Capture with `M3M0_SHOTS` and put
previews in `~/Documents/claude/m3m0/` (montage for contact sheets). The
window opens maximised on whichever monitor GNOME picks, so note the size.

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
- Ask before adding features he didn't list. Lowercase casual voice for any
  user-facing copy.
