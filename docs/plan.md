# plan

Build hours are Claude's; "you" = Zain's testing time. Commit + push after each.

| # | phase | status |
|---|---|---|
| 0 | spikes: plugin bridge, KaTeX look, PDF renderer | done: RaTeX, hayro, bridge 6/6 |
| 1 | skeleton + read model | done: boot anim, vim nav, titlebar, icon, :db |
| 2 | outliner editor | done: insert mode, o/O, dd, >>/<<, alt-j/k, undo, [[ autocomplete, katex; lossless round-trip over all 3700 rems |
| 3 | changesets + commit | done: :w diff, :w!, :log revert, tmp id mapping; verified against RemNote (commit-test + UI path) |
| 4 | week files pane | done: any week file (README links, quiz + answers, md, json, images, pdf), f hints / b back |
| 5 | graph | done, rebuilt as asked: pages/terms/citations, m3m0 look, module islands, tab walks links, / search |
| 6 | Claude deck comparison | done: cards view, gaps, gl link, s stage |
| 7 | chat panel | done: claude -p, page + module context, edits only revision/ |
| 8 | review | done: due queue, grading via RemNote's scheduler (bridge); not exercised on real cards |

Open / next:
- RemNote starts on demand (commit, review, `:remnote`), not at launch, and
  not hidden (no Wayland way to hide another app's window).
- Multiple-choice writes: need one sample MCQ card to learn the encoding.
- Turning a rem into a code block isn't possible through the plugin SDK.
- Images inside notes show as `[image]` (images as week files do render).
- rem -> uni-git mapping is by name only (no override file yet).
