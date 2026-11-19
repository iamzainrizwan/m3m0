# design

Decisions from the kickoff Q&A (2026-11-19) and why. If you change one, update
this file.

## why

RemNote's desktop app is a laggy webview. Zain wants a native client that does
everything he actually uses, plus things RemNote can't: a graph view, his
module's PDFs next to the notes, Claude's generated flashcards next to his own,
and a Claude chat about the module.

## architecture

```
            reads (snapshot, safe while RemNote runs)
  m3m0  ─────────────────────────────────────────▶  ~/remnote/<kb>/remnote.db
   │ ▲                                                      ▲
   │ │ ws://127.0.0.1:47830 (m3m0 is the server)            │ writes + sync stamps
   ▼ │                                                      │
  plugin (inside RemNote, loaded from http://localhost:47831) ──▶ RemNote cloud ──▶ phone
```

- **RemNote stays installed as the sync engine** and runs hidden. Its sync only
  exists inside its app; there's no public sync API. Zain: "does remnote have to
  be OPEN? ugh lame ok sure". m3m0 should start it hidden when needed and wait
  for sync to settle, so he never sees it.
- **Reads** go straight to SQLite through a snapshot (backup API), as
  `uni-git/_tools/remnote.py` already did. Never blocks or disturbs RemNote.
- **Writes** go through a RemNote plugin over a localhost websocket, so RemNote
  writes the per-field sync stamps itself. Direct SQLite writes (rules in
  `docs/remnote-storage.md`) are the fallback, only with RemNote closed.
- **The plugin is served by m3m0** on localhost and loaded through RemNote's
  "Develop from localhost". RemNote probably can't install an unpublished
  plugin from a zip, and the plugin only matters while m3m0 runs anyway.

## edits: git-like changesets

Zain: "i dont mind this using a git commit like system for management because i
know writing to the remnote database can be a biatch".

- Edits stage into a changeset (shown as a diff); a commit applies it in one
  go through the bridge.
- Both: one-key "stage, show diff, apply" and real history (log of commits,
  revert = apply the inverse changeset). Store before/after per op.
- Conflicts with phone edits: **commit wins**. (RemNote merges per field by the
  newer `,u` stamp, which a fresh commit always has.)
- Doesn't need to work with the RemNote UI open; m3m0 replaces it.

## editor

- Outliner (bullets, indent, collapse), vim keys, rendered when the cursor is
  elsewhere and raw only on the focused bullet (per-block live preview).
- Must support: LaTeX, images, cloze, `::` `;;` `>>` `<>` `>>>` cards,
  `[[references]]`, headings, inline code, and longer code blocks.
- LaTeX renders inline and should look like KaTeX, "so it looks the same here
  and on the phone". → RaTeX (`docs/spikes.md`).

## graph

Rebuilt from scratch on request (not remnote_vault.py's model). Nodes: pages
(weeks), terms (`::` concepts) and anything [[referenced]]. Edges: term ->
its page, and every [[ref]] from where it's used -> its target, weighted.
Look: dot-matrix squares (pages hollow white, terms grey sized by citations),
cross-module links red, module names dim over their clusters; gravity pulls
each module to its own spot so modules form islands. Keyboard: hjkl
directional select, tab walks links, / search, L local, f module filter.

## flashcards

- His cards next to that week's Claude deck
  (`uni-git/<module>/revision/week-NN-flashcards.md`), gaps highlighted both
  ways, "stage into my notes" for a Claude card.
- Keybind to link the rem under the cursor to a Claude card.
- Review: RemNote's **default** scheduler (his setting), rebuilt from card
  history, written back through the bridge (`Card.updateCardRepetitionStatus`).
  Less important; he mostly reviews on his phone.

## claude chat

- Shells out to `claude -p` (his plan, no API key), prompt on stdin,
  `--setting-sources ""` so his Discord/screensaver hooks don't fire.
- Context: the whole module (`uni-git/<module>`) plus the current note and
  week's PDFs.
- May answer questions and edit Claude's own material (`<module>/revision/`),
  never his rems.

## pdfs

KEATS downloads land in `uni-git/<module>/sources/` and get sorted into
`weeks/`. Built-in viewer (hayro, `docs/spikes.md`), side by side with notes.

## mapping rems to uni-git

A RemNote module rem (`FC2`) maps to the uni-git folder (`fc2`) by lowercased
name; `Week N - …` maps to `week-NN`. An override file covers mismatches.

## platform

- Rust + egui ("Rust would be interesting"). Not Tauri (that's a webview again).
- Rice look; own titlebar; app icon from the logo glyphs.
- Target screen: 14" laptop, 2560x1600 at 1.5x. Keyboard-first everywhere.

## which database

`~/remnote/remnote-<id>/remnote.db`; the id looks like the knowledge base id
(stable unless he makes a new KB). Default = largest db found; `:db` lists all
and saves the pick. If RemNote moves storage, read how its app JS opens the DB
(that's how remnote.py's notes were reverse-read) and add the path to
`kb::find_dbs`.
