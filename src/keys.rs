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
            ("ctrl-p", "show / hide the week's PDFs (focuses them)"),
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
            ("i / I", "edit bullet, cursor at start"),
            ("a / A", "edit bullet, cursor at end"),
            ("o / O", "new bullet below / above (below = first child if open)"),
            ("dd", "delete bullet and everything under it"),
            (">> / <<", "indent / outdent (3>> works)"),
            ("alt-j / alt-k", "move bullet down / up among siblings"),
            ("u / ctrl-r", "undo / redo (staged changes)"),
            ("gl", "link this bullet to a card in claude's deck"),
        ],
    },
    Group {
        title: "insert mode",
        keys: &[
            ("esc", "save the bullet, back to normal mode"),
            ("enter", "save, new bullet below"),
            ("shift-enter", "newline inside the bullet (code blocks)"),
            ("tab / s-tab", "save and indent / outdent, keep editing"),
            ("backspace (empty)", "delete the bullet, edit the one above"),
            ("[[", "reference autocomplete · ctrl-n/p or ↑↓ move · tab/enter pick"),
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
            ("# / ## / ###", "heading 1-3"),
            ("```code", "code block (only existing code blocks can be edited)"),
        ],
    },
    Group {
        title: "commit (staged edits → RemNote)",
        keys: &[
            (":w", "review the staged diff · enter commits, esc goes back"),
            (":w!", "commit straight away"),
            (":log", "past commits · enter stages a revert of one"),
            ("u", "undo staged changes one by one (before :w)"),
            (":e!", "drop all staged changes and reload from RemNote"),
        ],
    },
    Group {
        title: "pdf panel (ctrl-p)",
        keys: &[
            ("j / k", "scroll"),
            ("ctrl-d / ctrl-u", "half page"),
            ("J / K", "next / previous page"),
            ("gg / G", "top / bottom of the file"),
            ("] / [", "next / previous PDF (week first, then module-wide)"),
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
            (":q", "quit"),
            (":w  :w!  :log  :e!", "see commit"),
            (":db", "pick which RemNote database to read"),
            (":reload / :e", "re-read RemNote (refuses with staged changes)"),
            (":zoom 130", "set zoom (also ctrl +/-)"),
            (":find", "same as ctrl-t"),
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
        "insert" => "esc save · enter new bullet · tab indent · [[ ref",
        "pdf" => "j/k scroll · J/K page · ]/[ file · +/- zoom · h notes",
        "graph" => "hjkl move · tab links · / find · enter open · L local · f module",
        "cards" => "h/l column · ]/[ gaps · enter open · gl link · s stage",
        "review" => "space reveal · 1 again 2 hard 3 good 4 easy · s skip · esc stop",
        "chat" => "enter send · shift-enter newline · ctrl-n new chat · esc notes",
        _ => "",
    }
}
