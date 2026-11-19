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

## unconfirmed (phase 0 tests these)

1. sandboxed plugin can reach `ws://127.0.0.1` in the desktop app
2. "Develop from localhost" survives a RemNote restart
3. every card kind and rich-text kind round-trips (bridge-test prints them)
4. `updateCardRepetitionStatus` schedules like a real review
5. the plugin's writes carry fresh `,u` stamps and reach the phone
6. RemNote keeps syncing while hidden/minimised; any way to start it hidden

## running the test

```
cd plugin && npm run build          # once
m3m0 bridge-test                    # serves the plugin, waits for it
# RemNote: Settings → Plugins → Build → Develop from localhost → http://localhost:47831
# check the phone for "m3m0 bridge test <stamp>", then:
m3m0 bridge-test --cleanup <root id>
```
