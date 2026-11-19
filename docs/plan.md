# plan

Build hours are Claude's; "you" = Zain's testing time. Commit + push after each.

| # | phase | status |
|---|---|---|
| 0 | spikes: plugin bridge, KaTeX look, PDF renderer (~3h, you 0.5h) | math ✓ RaTeX, pdf ✓ hayro, bridge ✓ (5/6, see bridge.md); left: `>>>` check, hidden sync |
| 1 | skeleton + read model: rice theme, KB port, module/week tree (~3h) | ✓ plus boot anim, vim nav, titlebar, icon, :db |
| 2 | outliner editor: vim modes, indent, collapse, all syntax, `[[ref]]` autocomplete, KaTeX inline (~8h, you 1h) | |
| 3 | changesets: staged diff, commit via bridge, log, revert, backup, commit wins (~5h) | |
| 4 | PDF panel: week PDFs from `weeks/` + `sources/`, side by side (~3h) | |
| 5 | graph: remnote_vault.py nodes/colours, force layout, filters, local graph (~4h) | |
| 6 | Claude deck comparison: gaps, link keybind, stage into notes (~4h) | |
| 7 | chat panel: `claude -p`, module context, edits only `revision/` (~3h) | |
| 8 | review: RemNote default scheduler from history, write back (~5h, you 1h) | |

Also pending: start RemNote hidden on launch/commit and wait for sync; rem →
uni-git mapping override file.
