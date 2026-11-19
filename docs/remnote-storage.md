# remnote storage

Reverse-read from RemNote 1.28.50's app JS and DB on 2026-10-22 (originally in
`uni-git/_tools/remnote.py`). Re-check after RemNote updates.

## files

- `~/remnote/remnote-<kb id>/remnote.db` (SQLite, WAL), plus `files/` for
  images (`%LOCAL_FILE%<name>` in rich text).
- Other folders under `~/remnote/` (`browser/`, `remnote-browser/`, `lnotes/`)
  are other copies/KBs; `:db` lists them all.
- The desktop app's Electron profile is `~/.config/RemNote` (IndexedDB etc.),
  not the notes.

## tables

- `quanta(_id, doc JSON, x)`: one row per rem.
  - `doc.key` / `doc.value`: rich text (front / back).
  - `doc.parent` + `doc.f` (sort key string): the tree.
  - `doc.type`: 1 concept (`::`), 2 descriptor (`;;`).
  - `doc.enableBackSR`: value + this = `<>`; value alone = `>>`.
  - no value + children tagged with the Card Item power-up
    (`SrPWzY0xLzu625Ted` in `doc.tp`) = `>>>`.
  - `doc.ps.r_s.v.s`: heading level `"H0"`..`"H4"`.
  - every field has `"<field>,u"` / `"<field>,o"` sync stamps.
- `cards(_id, doc, x, rId)`: one row per card direction; `doc.k` ends with the
  direction; `doc.h` is review history.

## rich text

A list of strings and objects:

| `i` | meaning | fields |
|---|---|---|
| `x` | LaTeX | `text`, `block?`, `cId?` |
| `m` | styled text | `text`, `b` bold, `l` italic, `u` underline, `q` code, `cId` cloze, `sup`/`sub` |
| `q` | reference | `_id` |
| `i` | image | `url`, `width?`, `height?` |

The plugin SDK's `RichTextInterface` uses the same shapes (`docs/bridge.md`), so
DB rich text can go through the bridge nearly unchanged.

## sync rules (for the direct-write fallback only)

- On start the app uploads docs with `doc.p > sync2["quanta"].lastSync`, and
  within a doc only fields whose `"<field>,u"` is newer than lastSync.
- Merging with the server, the side with the higher `"<field>,u"` wins.
- The `x` column is the rem's numeric id (`doc.x` is overwritten on load); the
  in-memory graph reloads rows with `doc.y >= tiny_graph_metadata.lastFlushedTime`.
- Search tables are rebuilt by quanta triggers; cards rows by the app.
- So a direct write sets `p = u = y = now` and `"<field>,u" = now` on each
  field it touches, with now later than lastSync and lastFlushedTime, **only
  with the app closed**, after copying the DB to `backups/`.

`remnote.py add/link` implements this and is the reference.
