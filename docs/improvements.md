# improvements

Zain's wishlist (2026-11-19), spitballed while working. All built on
2026-11-20 except 14; what was decided is under each (→). Pick by number. Rule across all of them: conversions and
auto-anything only fire on keys he types, never on paste, `:stage`, Claude
cards or text read from RemNote, and never inside `$…$` or `` `…` ``.

## editing flow

1. **auto-indent**: `enter` on a bullet ending in `>>>` / `>>A)`, or starting
   with `#`/`##`/`###`, makes the new bullet its first child; `o` does the same
   even before it has children. `enter` on an empty indented bullet outdents it
   one level (repeatable), like `shift-tab`; at the page's top level it drops
   the empty bullet. Open: should `##` under `#` nest too?
   → built. `##` under `#` nests too: enter on any heading makes its first child.
2. **indent without insert**: `alt-h` / `alt-l` outdent / indent (pairs with
   `alt-j/k`), or `tab` / `shift-tab` in normal mode on the notes, leaving pane
   switching to `ctrl-h/l` / `ctrl-p`.
   → built as `alt-h` / `alt-l`; `tab` in normal mode still cycles panes (open: switch it?).
3. **indent guides**: thin dim lines from each parent down to its last child,
   one per level; the cursor's level in red (also in insert mode).
   → built.
4. **`==` / `===` shortcuts**: typing the second `=` turns it into ` >> `, the
   third into ` >>> `, live in the buffer; `backspace` right after undoes it
   (literal `==`). Typed markers (`>>`, `>>>`, `::`, `;;`, `<>`, `<<`) get one
   space each side automatically; `===` waits for the third `=`.
   → built (typing.rs). `>> A)` closes up to `>>A)`; a space typed right after an auto-space is swallowed; backspace right after any marker puts back what was typed.
5. **vim inside a line**: `esc` stops at a text cursor first (`w b e 0 $ f t x
   dw cw ciw D C p`), second `esc` saves; `v` charwise visual (`y d c`), `V`
   linewise visual over bullets (`d`, `>`/`<`, move).
   → built, plus `o`, `j`/`k` (save and move), `^`, `diw` `de` `d$` `dt)`. No counts inside the line yet. `V`: j/k extend, `d y > <` act.
6. **auto-pairs**: `( [ { " `` ` `` $` insert their closer, typing the closer
   steps over it, `backspace` after an opener deletes both. Never pair `'`;
   `$` only after a space or at line start. `[[` -> `[[]]` keeps autocomplete,
   `{{` -> `{{}}` for clozes.
   → built.
7. **yank / paste bullets**: `yy` copies a subtree, `dd` cuts into a register,
   `p` / `P` paste below / above.
   → built; the register holds the lines, so pasted bullets are new rems.
8. **`.` repeat** the last change.
   → built for dd, >>, <<, J, p/P, alt-hjkl, and o/O + the typed text.
9. **split / join**: `enter` mid-line splits into two bullets; `J` joins with
   the next.
   → built.
10. **`/` commands** in insert mode (`/h2`, `/code`, `/cloze`, `/mcq`, `/ref`)
    with a filtering menu.
    → built: h1-h3 ref cloze code math basic concept descriptor both multi mcq bold italic. `/code` is inline code (new code blocks are impossible through the plugin API).
11. **format keys** in insert mode: `ctrl-b` / `ctrl-i` / `` ctrl-` `` wrap the
    selection or word in `**` / `*` / `` ` ``, `ctrl-m` in `$…$`.
    → built; on already-wrapped text it unwraps.

## general

12. **staged changes survive** a crash or quit (check first whether they're
    already persisted).
    → they weren't persisted; now saved to `~/.local/state/m3m0/staged.json` on every change and restored on start (same db).
13. **backlinks**: "referenced by" list for the current bullet or page.
    → built as `gb`: this bullet, then this page.
14. **paste images**: `ctrl-v` with a screenshot adds an image bullet.
    → NOT built. RemNote's pastes are `%LOCAL_FILE%<id>.png` uploads (some aren't even on disk here) and the SDK has no upload call, so a file dropped into `files/` probably never reaches the phone. Options: data: URL inline (syncs, bloats the db), local file (desktop only?), an upload host.
15. **due counts in the sidebar** per week.
    → built; recounted every 30 s, cards graded in m3m0 drop out at once.
16. **ask Claude about this bullet**: one key opens chat with the cursor
    bullet + children quoted.
    → built as `ga`.
17. **scroll speed**: wheel / touchpad scroll is slow because egui divides
    point deltas by the display scale (GNOME 1.5x) and m3m0's zoom; scale
    `MouseWheel` events back up in eframe's `raw_input_hook`, plus a setting.
    → built, plus `:scroll N` (saved).
18. **keep the bullet dot while editing**: show it in red instead of the
    pencil; the red left bar still marks the line.
    → built.
19. **ordered `>>>` cards**: typing `1.` at the start of a `>>>` card's first
    child (or `front >>1.`, RemNote's list-answer syntax, to confirm) makes it
    ordered; children shown numbered and renumbered. Needs: how RemNote stores
    the flag (roundtrip must stay 0 differ) and whether the SDK can set it.
    → built: `1. ` at the start of an item numbers the card. RemNote stores the List Item power-up per item (`tp` tag natively, `apu.i` via the SDK); `setIsListItem` writes it, checked on a scratch tree. `>>1.` syntax not added; roundtrip still 0 differ.
