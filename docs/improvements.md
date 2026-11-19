# improvements

Zain's wishlist (2026-11-19), spitballed while working; nothing here is built
yet. Pick by number ("build 1, 6"). Rule across all of them: conversions and
auto-anything only fire on keys he types, never on paste, `:stage`, Claude
cards or text read from RemNote, and never inside `$…$` or `` `…` ``.

## editing flow

1. **auto-indent**: `enter` on a bullet ending in `>>>` / `>>A)`, or starting
   with `#`/`##`/`###`, makes the new bullet its first child; `o` does the same
   even before it has children. `enter` on an empty indented bullet outdents it
   one level (repeatable), like `shift-tab`; at the page's top level it drops
   the empty bullet. Open: should `##` under `#` nest too?
2. **indent without insert**: `alt-h` / `alt-l` outdent / indent (pairs with
   `alt-j/k`), or `tab` / `shift-tab` in normal mode on the notes, leaving pane
   switching to `ctrl-h/l` / `ctrl-p`.
3. **indent guides**: thin dim lines from each parent down to its last child,
   one per level; the cursor's level in red (also in insert mode).
4. **`==` / `===` shortcuts**: typing the second `=` turns it into ` >> `, the
   third into ` >>> `, live in the buffer; `backspace` right after undoes it
   (literal `==`). Typed markers (`>>`, `>>>`, `::`, `;;`, `<>`, `<<`) get one
   space each side automatically; `===` waits for the third `=`.
5. **vim inside a line**: `esc` stops at a text cursor first (`w b e 0 $ f t x
   dw cw ciw D C p`), second `esc` saves; `v` charwise visual (`y d c`), `V`
   linewise visual over bullets (`d`, `>`/`<`, move).
6. **auto-pairs**: `( [ { " `` ` `` $` insert their closer, typing the closer
   steps over it, `backspace` after an opener deletes both. Never pair `'`;
   `$` only after a space or at line start. `[[` -> `[[]]` keeps autocomplete,
   `{{` -> `{{}}` for clozes.
7. **yank / paste bullets**: `yy` copies a subtree, `dd` cuts into a register,
   `p` / `P` paste below / above.
8. **`.` repeat** the last change.
9. **split / join**: `enter` mid-line splits into two bullets; `J` joins with
   the next.
10. **`/` commands** in insert mode (`/h2`, `/code`, `/cloze`, `/mcq`, `/ref`)
    with a filtering menu.
11. **format keys** in insert mode: `ctrl-b` / `ctrl-i` / `` ctrl-` `` wrap the
    selection or word in `**` / `*` / `` ` ``, `ctrl-m` in `$…$`.

## general

12. **staged changes survive** a crash or quit (check first whether they're
    already persisted).
13. **backlinks**: "referenced by" list for the current bullet or page.
14. **paste images**: `ctrl-v` with a screenshot adds an image bullet.
15. **due counts in the sidebar** per week.
16. **ask Claude about this bullet**: one key opens chat with the cursor
    bullet + children quoted.
17. **scroll speed**: wheel / touchpad scroll is slow because egui divides
    point deltas by the display scale (GNOME 1.5x) and m3m0's zoom; scale
    `MouseWheel` events back up in eframe's `raw_input_hook`, plus a setting.
18. **keep the bullet dot while editing**: show it in red instead of the
    pencil; the red left bar still marks the line.
19. **ordered `>>>` cards**: typing `1.` at the start of a `>>>` card's first
    child (or `front >>1.`, RemNote's list-answer syntax, to confirm) makes it
    ordered; children shown numbered and renumbered. Needs: how RemNote stores
    the flag (roundtrip must stay 0 differ) and whether the SDK can set it.
