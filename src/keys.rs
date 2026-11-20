//! Every key, in one table: drives the `?` reference, the which-key popup
//! (shown while a prefix like g / z / d is pending) and the per-pane hint line.
//! If you add a key anywhere, add it here.

pub struct Group {
    pub title: &'static str,
    pub keys: &'static [(&'static str, &'static str)],
}

pub const HELP: &[Group] = &[
    Group {
        title: "anywhere",
        keys: &[
            ("?", "this reference (j/k scroll, esc close)"),
            (":", "command line (see commands below)"),
            ("/", "search: notes page or graph · n / N next / previous"),
            ("ctrl-t", "find any week or rem by name and jump to it"),
            ("tab / s-tab", "cycle focus: tree → notes → pdf"),
            ("ctrl-h / ctrl-l", "focus tree / notes"),
            ("ctrl-b", "show / hide the sidebar"),
            ("ctrl-p", "show / hide the week's files pane (pdfs, quiz, notes...)"),
            ("ctrl-g", "graph view on / off"),
            ("ctrl-f", "cards view: yours vs claude's deck"),
            ("ctrl-a", "ask claude: chat panel about the open module"),
            ("ctrl-j / ctrl-k", "next / previous week (across modules)"),
            ("ctrl-o", "jump back (after gd, a graph or search jump)"),
            ("ctrl + / ctrl - / ctrl 0", "zoom in / out / reset (saved)"),
            ("esc", "close overlay · leave insert · leave graph or cards"),
        ],
    },
    Group {
        title: "tree (sidebar)",
        keys: &[
            ("j / k", "down / up (counts: 5j)"),
            ("gg / G", "first / last"),
            ("ctrl-d / ctrl-u", "half page down / up"),
            ("l / enter", "open module or week (week moves focus to notes)"),
            ("h", "collapse module / go to its module"),
            ("o", "new week: stages the next Week N - title (from module.json, reading weeks skipped; :w to commit)"),
            ("< / >", "narrow / widen sidebar (5> works)"),
        ],
    },
    Group {
        title: "notes: move",
        keys: &[
            ("j / k", "down / up a bullet (counts: 5j)"),
            ("gg / G", "first / last bullet"),
            ("ctrl-d / ctrl-u", "half page down / up"),
            ("h", "parent bullet · at top level: focus tree"),
            ("l", "first child (unfolds)"),
            ("gd", "go to the [[ref]] on this bullet (3gd: third ref)"),
            ("enter", "fold / unfold"),
            ("za / zc / zo", "toggle / close / open fold"),
            ("zM / zR", "fold everything / unfold everything"),
            ("n / N", "next / previous match of the last / search"),
        ],
    },
    Group {
        title: "notes: edit",
        keys: &[
            ("i / I", "edit bullet, cursor at start (empty page: first bullet)"),
            ("a / A", "edit bullet, cursor at end"),
            ("o / O", "new bullet below / above (below = first child if open, or under >>> / >>A) / a heading)"),
            ("dd", "delete bullet and everything under it"),
            ("J", "join the next bullet onto this one (its children come too)"),
            ("yy / p / P", "yank bullet (and children) · paste below / above (dd cuts)"),
            ("V", "select bullets: j/k extend · d delete · y yank · > < indent · esc"),
            (".", "repeat the last change (dd, >>, J, p, alt-hjkl, o + its text)"),
            (">> / <<", "indent / outdent (3>> works)"),
            ("alt-j / alt-k", "move bullet down / up among siblings"),
            ("alt-h / alt-l", "outdent / indent (like << / >>)"),
            ("u / ctrl-r", "undo / redo (staged changes)"),
            ("gl", "link this bullet to a card in claude's deck"),
        ],
    },
    Group {
        title: "insert mode",
        keys: &[
            ("esc", "text cursor in the line (vim, see line) · esc again saves"),
            ("enter", "save, new bullet below (a child under >>> / >>A) / headings) · mid-line: split"),
            ("enter (empty bullet)", "outdent it one level · at the top level: drop it"),
            ("shift-enter", "newline inside the bullet (code blocks)"),
            ("tab / s-tab", "save and indent / outdent, keep editing"),
            ("backspace (empty)", "delete the bullet, edit the one above"),
            ("[[", "reference autocomplete · ctrl-n/p or ↑↓ move · tab/enter pick"),
            ("/ (after a space)", "commands: /h2 /ref /cloze /code /math /mcq /multi ... · tab/enter pick"),
            ("ctrl-b / ctrl-i / ctrl-`", "bold / italic / code the selection or word (again to undo)"),
            ("ctrl-m", "wrap the selection or word in $…$"),
            ("( [ { \" ` $", "type their closer too; the closer steps over · backspace deletes both"),
            ("== / ===", "become >> / >>> (backspace right after: literal ==)"),
            (":: ;; >> <> <<", "get a space each side as you type them (backspace: as typed)"),
            ("(never)", "none of this fires inside `code` or $latex$, or on paste"),
        ],
    },
    Group {
        title: "line (esc in insert: vim in the bullet)",
        keys: &[
            ("esc", "first esc: text cursor in the line · second esc: save"),
            ("h l  w b e  0 ^ $", "move"),
            ("f t F T + char", "to / just before that char"),
            ("i a I A", "back to typing (here / after / line start / end)"),
            ("x  D  C", "delete char · delete to end · change to end"),
            ("dw cw ciw diw de d$ dt)", "delete / change with a motion or the word"),
            ("dd / cc / yy", "delete the bullet / clear the line / yank the line"),
            ("p / P", "paste after / before the cursor"),
            ("v", "select chars: move, then y d c"),
            ("V", "save, select bullets (see notes: edit)"),
            ("o", "new bullet below"),
            ("j / k", "save, move to the next / previous bullet"),
        ],
    },
    Group {
        title: "syntax (what you type in a bullet)",
        keys: &[
            ("front :: back", "concept card"),
            ("front ;; back", "descriptor card"),
            ("front >> back", "basic card, forward"),
            ("front <> back", "basic card, both ways"),
            ("front >>>", "multi-line card: the children are the answer"),
            ("front >>A)", "multiple choice (children = options; writes pending)"),
            ("{{text}}", "cloze"),
            ("$x^2$", "inline LaTeX (rendered like KaTeX)"),
            ("**bold** *italic* `code`", "styles"),
            ("[[name]]", "reference to another rem"),
            ("[[name|shown]]", "reference that reads differently ([[Linear Program|LP]] shows LP; saved as a RemNote alias)"),
            ("# / ## / ###", "heading 1-3"),
            ("```code", "code block (only existing code blocks can be edited)"),
        ],
    },
    Group {
        title: "commit (staged edits → RemNote)",
        keys: &[
            (":w", "review the staged diff · enter commits, esc goes back"),
            (":w!", "commit straight away"),
            (":stage FILE", "stage edits prepared outside m3m0 (json list of {id, front, back?}); review with :diff"),
            (":log", "past commits · enter stages a revert of one"),
            ("u", "undo staged changes one by one (before :w)"),
            (":e!", "drop all staged changes and reload from RemNote"),
        ],
    },
    Group {
        title: "files pane (ctrl-p)",
        keys: &[
            ("j / k", "scroll"),
            ("ctrl-d / ctrl-u", "half page"),
            ("J / K", "next / previous page"),
            ("gg / G", "top / bottom of the file"),
            ("o / enter", "list every file for this week and pick one (also :pdf)"),
            ("] / [", "next / previous file in that list"),
            ("a", "quiz: show / hide answers and explanations"),
            ("f", "follow a link: hint letters appear on each link, type one"),
            ("b / backspace", "back to the file you came from"),
            ("click a link", "follow it (http links open in the browser)"),
            ("(non-pdf)", "j/k scroll · J/K or ctrl-d/u page · gg/G ends"),
            ("+ / - / =", "zoom / reset"),
            ("< / >", "widen / narrow the panel"),
            ("h", "back to notes"),
        ],
    },
    Group {
        title: "graph (ctrl-g)",
        keys: &[
            ("h j k l", "select the nearest node in that direction"),
            ("tab / s-tab", "walk the selected node's links, strongest first"),
            ("enter", "open the selected node in the notes"),
            ("/", "find a node by name"),
            ("L", "local graph: two hops around the selection"),
            ("f", "filter by module (cycles, then all)"),
            ("+ / - / =", "zoom in / out / fit everything"),
            ("c", "follow the selection"),
            ("r", "rebuild from the current notes"),
            ("esc / ctrl-g", "back to notes"),
        ],
    },
    Group {
        title: "cards (ctrl-f)",
        keys: &[
            ("h / l", "your column / claude's column"),
            ("j / k  gg / G  ctrl-d / u", "move"),
            ("] / [", "next / previous gap in this column"),
            ("enter", "yours: open in notes · claude's: jump to your matching card"),
            ("i / a", "edit your card (jumps to it in the notes)"),
            ("gl / gL", "link / unlink your card to one of claude's"),
            ("s", "stage a missing claude card into your notes"),
            ("esc / ctrl-f", "back to notes"),
        ],
    },
    Group {
        title: "review (RemNote's scheduler, synced)",
        keys: &[
            ("gr / gR", "review due cards on this page / in this module"),
            (":review all", "everything due"),
            ("space / enter", "reveal the answer"),
            ("1 2 3 4", "again / hard / good / easy (sent to RemNote)"),
            ("s", "skip this card"),
            ("esc", "stop (shows how many were graded)"),
        ],
    },
    Group {
        title: "chat (ctrl-a)",
        keys: &[
            ("enter", "send (claude sees this page + the module's files)"),
            ("shift-enter", "newline"),
            ("ctrl-d / ctrl-u", "scroll the conversation"),
            ("ctrl-n", "new conversation"),
            ("esc / tab", "back to notes / next pane"),
            ("(claude may edit)", "only its own decks in revision/, when you ask"),
        ],
    },
    Group {
        title: "commands (:)",
        keys: &[
            (":q", "quit (refuses with staged changes)"),
            (":wq / :x", "commit, then quit"),
            (":q!", "quit and drop staged changes"),
            (":w  :w!  :log  :e!", "see commit"),
            (":db", "pick which RemNote database to read"),
            (":reload / :e", "re-read RemNote (refuses with staged changes)"),
            (":zoom 130", "set zoom (also ctrl +/-)"),
            (":scroll 1.5", "wheel / touchpad scroll speed (saved; 1 = follows your fingers)"),
            (":find", "same as ctrl-t"),
            (":pdf", "open the files pane and list this week's files"),
            (":remnote", "start RemNote (the sync engine) if it isn't running"),
            (":remnote restart", "restart it so it loads the bridge plugin from m3m0"),
            (":help", "this reference"),
        ],
    },
    Group {
        title: "mouse (optional)",
        keys: &[
            ("click", "select a row / node · click a [[ref]] to follow it"),
            ("double-click", "fold a bullet · open a graph node"),
            ("drag edges", "resize sidebar / pdf panel / window"),
            ("drag top bar", "move window · double-click maximises"),
        ],
    },
];

/// Continuations for a pending prefix, for the which-key popup.
pub fn which(prefix: &str, ctx: &str) -> Vec<(&'static str, &'static str)> {
    match (prefix, ctx) {
        ("g", "cards") => vec![("g", "top"), ("l", "link your card"), ("L", "unlink")],
        ("g", "pdf") => vec![("g", "top of file")],
        ("g", _) => vec![("g", "top"), ("d", "go to [[ref]] on this bullet"), ("l", "link to a claude card"), ("r", "review this page's due cards"), ("R", "review this module")],
        ("z", _) => vec![("a", "toggle fold"), ("c", "close fold"), ("o", "open fold"), ("M", "fold all"), ("R", "unfold all")],
        ("d", _) => vec![("d", "delete bullet (and children)")],
        ("y", _) => vec![("y", "yank bullet (and children)")],
        (">", _) => vec![(">", "indent")],
        ("<", _) => vec![("<", "outdent")],
        (p, _) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => {
            vec![("j / k", "move that many"), (">> / <<", "indent / outdent that many"), ("gd", "go to that ref"), ("u", "undo that many")]
        }
        _ => vec![],
    }
}

/// One-line hint for the focused pane (status bar).
pub fn hint(ctx: &str) -> &'static str {
    match ctx {
        "tree" => "j/k move · l open · h collapse · </> width · tab notes · ctrl-t find",
        "notes" => "i/a edit · o new · dd del · >> indent · gd ref · / find · ctrl-f cards · ctrl-g graph",
        "line" => "esc save · i/a type · w b e 0 $ move · x dw cw ciw D C · v select · V bullets",
        "visual" => "move to extend · y yank · d delete · c change · esc back",
        "vbullets" => "j/k extend · d delete · y yank · > < indent · esc done",
        "insert" => "esc line cursor · enter new bullet · tab indent · [[ ref · / commands · ctrl-b bold",
        "pdf" => "f follow link · b back · o pick file · j/k scroll · ]/[ file · a answers · h notes",
        "graph" => "hjkl move · tab walk links · / find · enter open · L local · f module · = fit · esc back",
        "cards" => "h/l column · ]/[ gaps · enter open · gl link · s stage",
        "review" => "space reveal · 1 again 2 hard 3 good 4 easy · s skip · esc stop",
        "chat" => "enter send · shift-enter newline · ctrl-n new chat · esc notes",
        _ => "",
    }
}

/// Which section `?` opens on, for the focused pane.
pub fn tab_for(ctx: &str) -> usize {
    let want = match ctx {
        "tree" => "tree",
        "insert" => "insert",
        "line" | "visual" => "line",
        "vbullets" => "notes: edit",
        "pdf" => "files",
        "graph" => "graph",
        "cards" => "cards",
        "review" => "review",
        "chat" => "chat",
        _ => "notes: move",
    };
    HELP.iter().position(|g| g.title.starts_with(want)).unwrap_or(0)
}
