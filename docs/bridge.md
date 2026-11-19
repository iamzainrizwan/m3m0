# bridge

How m3m0 writes to RemNote: a RemNote plugin (`plugin/`) applies ops sent by
m3m0 through the plugin SDK, so RemNote handles sync stamps and cloud sync.

## transport

- A plugin can't listen on a port, so **m3m0 is the server**:
  `ws://127.0.0.1:47830`. The plugin connects on activate and retries every 2 s
  while m3m0 isn't running.
- m3m0 also serves `plugin/dist` on `http://localhost:47831` (CORS open).
  RemNote loads it via Settings → Plugins → Build → **Develop from localhost**.
- Sandboxed plugin (`requestNative: false`). Evidence a sandboxed plugin can
  open a localhost websocket: the community remnote-mcp-bridge plugin does.
  Switch to `requestNative: true` if the connection is blocked.
- Scope: `All` / `ReadCreateModifyDelete`.

## protocol (v1)

Plugin → m3m0 on connect: `{"hello": "m3m0-bridge", "version": 1}`.
m3m0 → plugin: `{"id": N, "op": "...", ...}`; reply
`{"id": N, "ok": true, "result": ...}` or `{"id": N, "ok": false, "error": "..."}`.

| op | args | does |
|---|---|---|
| `ping` | | `{version}` |
| `create` | set-fields | `createRem()` then set-fields; `{id}` |
| `update` | `rem`, set-fields | set-fields on an existing rem |
| `delete` | `rem` | `rem.remove()` |
| `get` | `rem` | `{text, back, parent, type, cards[{id,type,next,history}]}` |
| `alias` | `rem`, `text` | `rem.getOrCreateAliasWithText(text)`; `{id}` (for `[[name\|alias]]` refs, sent as `aliasId`) |
| `review` | `card`, `score` | `card.updateCardRepetitionStatus(score)`; `{next, history}` |

set-fields (all optional): `parent` (+`position`) → `setParent`; `text` →
`setText`; `back` (null clears) → `setBackText`; `remType` (1 concept, 2
descriptor) → `setType`; `heading` ("H1".."H3") → `setFontSize`; `practice`
("forward"/"backward"/"both"/"none") → `setPracticeDirection`; `cardItem` →
`setIsCardItem`.

Scores (`QueueInteractionScore`): AGAIN 0, HARD 0.5, GOOD 1, EASY 1.5,
TOO_EARLY 0.01, RESET 3.

## SDK facts (from @remnote/plugin-sdk 0.0.46 typings)

- `createRem()` takes no args; parent/text come after.
- The `richText` builder can't make clozes; pass raw JSON. Rich text shapes
  match the DB (`docs/remnote-storage.md`); the cloze key name in the SDK
  (`RICH_TEXT_FORMATTING.CLOZE`) wasn't resolved, bridge-test checks whether
  `cId` round-trips.
- Code blocks: `richText.code(text, lang)`; there's no `setIsCode`.
- Multi-line `>>>`: `setIsCardItem` on children (and maybe the `w` MultiLineCard
  power-up); bridge-test checks what RemNote generates.

## phase 0 results (2026-11-19, RemNote 1.28.x desktop)

1. ✓ sandboxed plugin reaches `ws://127.0.0.1` (no `requestNative` needed)
2. ✓ "Develop from localhost" survives a RemNote restart
3. ✓ round-trip, checked in the DB: heading (`ps.r_s.v.s`), `::` (`type 1`),
   `;;` (`type 2`), `>>` (`enableBackSR false`), `<>` (`true`, cards f+b),
   latex / code / bold / `[[ref]]` stored byte-identical to our JSON, cloze via
   `cId` generates a cloze card
   - `>>>`: `setIsCardItem` tags children with `apu.w` (MultiLineCard), not
     the `tp` Card Item tag hand-typed notes use; parent gets a forward card.
     Pending: confirm it reviews as a multi-line card on the phone.
4. ✓ `updateCardRepetitionStatus(GOOD)` adds a history entry
5. ✓ every written field has fresh `,u` stamps; the tree reached the phone
6. ✓ keeps syncing with RemNote minimised/on another workspace (the write
   reached the phone without touching RemNote)

Gotcha: RemNote fetches the plugin code from m3m0's HTTP server **at its own
startup**. If RemNote starts while m3m0 isn't serving, the plugin is listed but
never runs. So m3m0 must start its server first, then launch RemNote (hidden),
or restart an already-running RemNote. Background windows also throttle
timers to ~1/min, so a reconnect can take up to a minute.

Side finding: RemNote's daily backup job zips and unlinks every entry in
`<kb>/backups/` and errors on directories, so never put anything there.
remnote.py's pre-write copies now go to `~/.local/share/m3m0/remnote-backups/`.

## running the test

```
cd plugin && npm run build          # once
m3m0 bridge-test                    # serves the plugin, waits for it
# RemNote: Settings → Plugins → Build → Develop from localhost → http://localhost:47831
# check the phone for "m3m0 bridge test <stamp>", then:
m3m0 bridge-test --cleanup <root id>
```
