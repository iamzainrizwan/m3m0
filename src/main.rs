mod boot;
mod cards;
mod chat;
mod bridge;
mod commit;
mod config;
mod deck;
mod edit;
mod files;
mod editor;
mod icon;
mod kb;
mod graph;
mod keys;
mod math;
mod pdf;
mod render;
mod review;
mod rich;
mod theme;

use config::Config;
use eframe::egui::{self, Align2, Color32, CursorIcon, FontFamily, FontId, Pos2, Rect, RichText, Sense, Vec2};
use egui::viewport::ResizeDirection;
use kb::{DbCandidate, Kb};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;
use theme::*;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("commit-test") {
        std::process::exit(commit_test(args.iter().any(|a| a == "--keep")));
    }
    if args.get(1).map(String::as_str) == Some("roundtrip") {
        std::process::exit(roundtrip());
    }
    if args.get(1).map(String::as_str) == Some("bridge-test") {
        if let Err(e) = bridge::test(&args[2..]) {
            eprintln!("bridge-test: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if std::env::args().any(|a| a == "--install") {
        if let Err(e) = icon::install() {
            eprintln!("install failed: {e}");
        }
        return Ok(());
    }
    // previews (M3M0_SHOTS): a fixed laptop-sized window at a fixed spot, so
    // scripts/preview.sh can put it on the secondary monitor via XWayland
    let preview = std::env::var("M3M0_SHOTS").is_ok();
    let mut vp = egui::ViewportBuilder::default()
        .with_title("m3m0")
        .with_app_id("m3m0")
        .with_decorations(false) // our own rice titlebar instead of the toolkit's
        .with_inner_size([1280.0, 820.0])
        .with_min_inner_size([520.0, 360.0])
        .with_maximized(!preview)
        .with_icon(std::sync::Arc::new(egui::IconData { rgba: icon::rgba(256), width: 256, height: 256 }));
    if preview {
        let pos = std::env::var("M3M0_PREVIEW_POS").ok().and_then(|p| {
            let (x, y) = p.split_once(',')?;
            Some([x.parse().ok()?, y.parse().ok()?])
        });
        vp = vp.with_inner_size([1706.0, 1066.0]);
        if let Some(p) = pos {
            vp = vp.with_position(p);
        }
    }
    let opts = eframe::NativeOptions {
        viewport: vp,
        ..Default::default()
    };
    eframe::run_native("m3m0", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

// --- sizes (points, before zoom) ---------------------------------------------
const BAR_H: f32 = 34.0;
const STATUS_H: f32 = 24.0;
const TREE_TEXT: f32 = 14.5;
const NOTE_TEXT: f32 = 15.0;
const HEADINGS: [f32; 5] = [21.0, 18.5, 17.0, 16.0, 15.5];
/// Max width of the note column, so lines stay readable on wide windows.
const MEASURE: f32 = 860.0;
const SIDEBAR_MIN: f32 = 120.0;
const SIDEBAR_MAX: f32 = 480.0;
const BAR_LOGO: (Pos2, f32) = (Pos2::new(14.0, 8.0), 2.6);
const HALF_PAGE: usize = 12;

struct Loaded {
    kb: Kb,
    mods: Vec<String>,
    db: PathBuf,
    log: Vec<(bool, String)>,
}

#[derive(PartialEq, Clone, Copy)]
enum Focus {
    Tree,
    Notes,
    Pdf,
    Chat,
}

enum Overlay {
    None,
    Help { scroll: usize, tab: usize },
    /// ctrl-t: jump to any week or rem by name
    Find { q: String, items: Vec<(String, String)>, cur: usize, for_q: Option<String> },
    Db { list: Vec<DbCandidate>, cur: usize },
    Cmd(String),
    /// staged changes; enter commits
    Diff { scroll: usize },
    Log { entries: Vec<commit::Entry>, cur: usize },
    Link(cards::LinkPick),
    /// list of the week's PDFs (o in the pdf panel, :pdf)
    Pdfs { cur: usize },
}

/// Actions that need the loaded data, queued during key handling (when the
/// data is borrowed out) and run right after.
enum Deferred {
    Revert(Vec<edit::Op>),
    Link(String, usize),
    Commit,
    Search(isize),
    Goto(String),
    Review(Option<String>),
}

#[derive(Clone, Copy, Debug)]
enum K {
    Ch(char),
    Ctrl(char),
    Alt(char),
    ShiftTab,
    Enter,
    Esc,
    Tab,
    Back,
    Up,
    Down,
    Left,
    Right,
}

struct App {
    boot: boot::Boot,
    rx: mpsc::Receiver<Result<Loaded, String>>,
    data: Option<Result<Loaded, String>>,
    cfg: Config,
    saved: Config,
    focus: Focus,
    open_mods: HashSet<String>,
    tree_cur: usize,
    week: Option<String>,
    note_cur: usize,
    folded: HashSet<String>,
    pending: String,
    overlay: Overlay,
    scroll_tree: bool,
    scroll_notes: bool,
    msg: Option<String>,
    reloaded: bool,
    replayed: bool,
    math: math::Math,
    /// vim jumplist for gd / ctrl-o: (page, cursor)
    jumps: Vec<(String, usize)>,
    changes: edit::Changes,
    editing: Option<editor::Editing>,
    /// cursor position to set in the edit box next frame (after autocomplete)
    pending_cursor: Option<usize>,
    bridge: bridge::Bridge,
    committing: Option<mpsc::Receiver<(commit::Outcome, Vec<edit::Op>)>>,
    pdfs: pdf::Pdfs,
    graph: Option<graph::Graph>,
    view_graph: bool,
    cards: Option<cards::CardsView>,
    graph_anchor: Option<usize>,
    deferred: Vec<Deferred>,
    chat: chat::Chat,
    review: Option<review::Review>,
    quit_after_commit: bool,
    quit_now: bool,
    replay_chunk: usize,
    replay_at: f64,
    last_search: Option<String>,
    links: std::collections::HashMap<String, String>,
}

fn load(db: Option<PathBuf>) -> Result<Loaded, String> {
    let db = db.ok_or("no RemNote database found under ~/remnote (try :db)")?;
    let kb = Kb::load(&db)?;
    let mods = kb.modules();
    let files = walk(&kb::home().join("uni-git"));
    let pdfs = files.iter().filter(|p| p.extension().is_some_and(|e| e == "pdf")).count();
    let decks = files.iter().filter(|p| p.to_string_lossy().ends_with("-flashcards.md")).count();
    let names: Vec<String> = mods.iter().map(|m| kb.name(m).to_lowercase()).collect();
    let log = vec![
        (true, format!("snapshot remnote.db  {:.1} MB", kb.db_bytes as f64 / 1e6)),
        (true, format!("{} rems · {} cards", kb.q.len(), kb.cards)),
        (true, format!("modules  {}", names.join(" "))),
        (true, format!("uni-git  {pdfs} pdfs · {decks} claude decks")),
        (false, "sync bridge  offline (phase 0)".to_owned()),
    ];
    Ok(Loaded { kb, mods, db, log })
}

fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
            continue;
        }
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

/// Rows of the sidebar: (rem id, is_module).
fn tree_rows(d: &Loaded, open: &HashSet<String>) -> Vec<(String, bool)> {
    let mut v = vec![];
    for m in &d.mods {
        v.push((m.clone(), true));
        if open.contains(m) {
            for w in d.kb.children(Some(m)) {
                if d.kb.name(w).starts_with("Week") {
                    v.push((w.clone(), false));
                }
            }
        }
    }
    v
}

/// Visible rows of the note pane: (rem id, depth), skipping folded subtrees and blank spacers.
fn note_rows(kb: &Kb, week: &str, folded: &HashSet<String>) -> Vec<(String, usize)> {
    fn go(kb: &Kb, id: &str, depth: usize, folded: &HashSet<String>, out: &mut Vec<(String, usize)>) {
        for k in kb.children(Some(id)) {
            let kids = kb.children(Some(k));
            // hide blank spacer rems, but never ones made in this session (being typed)
            if kids.is_empty() && !k.starts_with("tmp-") && kb.line(k).trim().is_empty() {
                continue;
            }
            out.push((k.clone(), depth));
            if !folded.contains(k) {
                go(kb, k, depth + 1, folded, out);
            }
        }
    }
    let mut out = vec![];
    go(kb, week, 0, folded, &mut out);
    out
}

impl App {
    fn new(cc: &eframe::CreationContext) -> App {
        theme::install(&cc.egui_ctx);
        // previews never touch the real config
        let preview = std::env::var("M3M0_SHOTS").is_ok();
        let cfg = if preview { Config { zoom: Some(1.15), ..Config::default() } } else { Config::load() };
        if let Some(z) = cfg.zoom {
            cc.egui_ctx.set_zoom_factor(z);
        }
        let rx = Self::spawn_load(&cc.egui_ctx, kb::resolve_db(cfg.db.as_deref()));
        let rc = cc.egui_ctx.clone();
        let bridge = bridge::Bridge::start(move || rc.request_repaint());
        let rc2 = cc.egui_ctx.clone();
        let pdfs = pdf::Pdfs::new(move || rc2.request_repaint());
        App {
            boot: boot::Boot::new(),
            rx,
            data: None,
            saved: cfg.clone(),
            cfg,
            focus: Focus::Tree,
            open_mods: HashSet::new(),
            tree_cur: 0,
            week: None,
            note_cur: 0,
            folded: HashSet::new(),
            pending: String::new(),
            overlay: Overlay::None,
            scroll_tree: false,
            scroll_notes: false,
            msg: None,
            reloaded: false,
            replayed: false,
            math: math::Math::default(),
            jumps: vec![],
            changes: edit::Changes::default(),
            editing: None,
            pending_cursor: None,
            bridge,
            committing: None,
            pdfs,
            graph: None,
            view_graph: false,
            cards: None,
            graph_anchor: None,
            deferred: vec![],
            chat: chat::Chat::default(),
            review: None,
            quit_after_commit: false,
            quit_now: false,
            replay_chunk: 0,
            replay_at: 0.0,
            last_search: None,
            links: deck::load_links(),
        }
    }

    fn spawn_load(ctx: &egui::Context, db: Option<PathBuf>) -> mpsc::Receiver<Result<Loaded, String>> {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(load(db));
            ctx.request_repaint();
        });
        rx
    }

    fn open_week(&mut self, w: String) {
        if self.week.as_ref() != Some(&w) {
            self.week = Some(w);
            self.note_cur = 0;
            self.scroll_notes = true;
        }
        self.focus = Focus::Notes;
    }

    /// M3M0_OPEN="FC2/Week 8" opens a week on load (for previews).
    fn preselect(&mut self) {
        // M3M0_OPEN_ID=<rem id>: open any rem as the page (scratch-tree tests)
        if let (Some(id), Some(Ok(_))) = (std::env::var("M3M0_OPEN_ID").ok().filter(|s| !s.is_empty()), &self.data) {
            self.open_week(id);
            return;
        }
        let Ok(path) = std::env::var("M3M0_OPEN") else { return };
        let Some(Ok(d)) = &self.data else { return };
        let (m, w) = path.split_once('/').unwrap_or((&path, ""));
        let Some(mi) = d.mods.iter().find(|i| d.kb.name(i) == m).cloned() else { return };
        let wk = d.kb.children(Some(&mi)).iter().find(|k| !w.is_empty() && d.kb.name(k).starts_with(w)).cloned();
        self.open_mods.insert(mi.clone());
        let rows = tree_rows(d, &self.open_mods);
        if let Some(wk) = wk {
            self.tree_cur = rows.iter().position(|r| r.0 == wk).unwrap_or(0);
            self.open_week(wk);
        }
    }

    // --- keys -----------------------------------------------------------------

    fn keys(&mut self, ctx: &egui::Context) -> Vec<K> {
        let mut out = vec![];
        ctx.input_mut(|i| {
            for e in &i.events {
                match e {
                    egui::Event::Text(t) if !i.modifiers.alt => out.extend(t.chars().map(K::Ch)),
                    egui::Event::Key { key, pressed: true, modifiers, .. } => {
                        if (modifiers.ctrl || modifiers.alt) && !modifiers.shift {
                            let n = key.name();
                            if n.len() == 1 {
                                let c = n.to_ascii_lowercase().chars().next().unwrap();
                                out.push(if modifiers.ctrl { K::Ctrl(c) } else { K::Alt(c) });
                            }
                            continue;
                        }
                        out.push(match key {
                            egui::Key::Enter => K::Enter,
                            egui::Key::Escape => K::Esc,
                            egui::Key::Tab if modifiers.shift => K::ShiftTab,
                            egui::Key::Tab => K::Tab,
                            egui::Key::Backspace => K::Back,
                            egui::Key::ArrowUp => K::Up,
                            egui::Key::ArrowDown => K::Down,
                            egui::Key::ArrowLeft => K::Left,
                            egui::Key::ArrowRight => K::Right,
                            _ => continue,
                        });
                    }
                    _ => {}
                }
            }
            // stop egui from using Tab / arrows to move widget focus
            i.events.retain(|e| !matches!(e, egui::Event::Key { key: egui::Key::Tab, .. }));
        });
        out
    }

    fn handle(&mut self, ctx: &egui::Context, k: K) {
        if std::env::var("M3M0_DEBUG").is_ok() {
            eprintln!("key {k:?}");
        }
        let mut taken = self.data.take();
        self.reloaded = false;
        if let Some(Ok(d)) = &mut taken {
            if self.focus == Focus::Chat && self.cfg.chat_open && matches!(self.overlay, Overlay::None) {
                self.handle_chat(ctx, d, k);
            } else if self.editing.is_some() {
                self.handle_insert(d, k);
            } else {
                self.handle_loaded(ctx, d, k);
            }
        } else {
            self.handle_loading(ctx, k);
        }
        if !self.reloaded {
            self.data = taken;
        }
    }

    fn handle_loading(&mut self, ctx: &egui::Context, k: K) {
        {
            if let (K::Ch(':'), Overlay::None) = (k, &self.overlay) {
                self.overlay = Overlay::Cmd(String::new());
            } else if let Overlay::Cmd(_) | Overlay::Db { .. } = self.overlay {
                self.handle_overlay(ctx, k);
            }
        }
    }

    fn handle_loaded(&mut self, ctx: &egui::Context, d: &mut Loaded, k: K) {
        if !matches!(self.overlay, Overlay::None) {
            self.handle_overlay(ctx, k);
            return;
        }
        self.msg = None;
        // count prefix: 5j
        if let K::Ch(c @ '0'..='9') = k {
            if c != '0' || self.pending.chars().any(|c| c.is_ascii_digit()) {
                self.pending.push(c);
                return;
            }
        }
        let digits: String = self.pending.chars().filter(char::is_ascii_digit).collect();
        let n: usize = digits.parse().unwrap_or(1).max(1);
        let prefix: String = self.pending.chars().filter(|c| !c.is_ascii_digit()).collect();
        self.pending.clear();

        let trows = tree_rows(d, &self.open_mods);
        let nrows = self.week.as_ref().map(|w| note_rows(&d.kb, w, &self.folded)).unwrap_or_default();

        if let K::Ctrl('f') = k {
            if self.cards.is_some() {
                self.cards = None;
            } else {
                self.view_graph = false;
                self.open_cards(d);
                self.focus = Focus::Notes;
            }
            return;
        }
        if self.review.is_some() && self.focus == Focus::Notes && !matches!(k, K::Ch(':') | K::Ch('?') | K::Tab | K::Ctrl(_)) && self.review_key(k) {
            return;
        }
        // gr review this page · gR this module
        if prefix == "g" && matches!(k, K::Ch('r') | K::Ch('R')) && self.cards.is_none() {
            let scope = match k {
                K::Ch('r') => self.week.clone(),
                _ => self.week.as_ref().and_then(|w| d.kb.module_of(w)),
            };
            self.start_review(d, scope);
            return;
        }
        if self.cards.is_some() && self.focus == Focus::Notes && self.cards_key(d, &prefix, k, n) {
            return;
        }
        // gl on a bullet in the notes: link it to a Claude card
        if prefix == "g" && matches!(k, K::Ch('l')) && self.focus == Focus::Notes && self.cards.is_none() {
            if let Some((id, _)) = nrows.get(self.note_cur).cloned() {
                self.open_cards(d);
                self.link_picker(d, &id);
                self.cards = None;
            }
            return;
        }
        if let K::Ctrl('g') = k {
            self.toggle_graph(d, &nrows);
            return;
        }
        if self.view_graph && self.focus != Focus::Pdf && self.focus != Focus::Tree {
            if self.graph_key(d, k, n) {
                return;
            }
        }
        if let K::Ctrl('a') = k {
            self.cfg.chat_open = !self.cfg.chat_open;
            if self.cfg.chat_open {
                self.cfg.pdf_open = false;
                self.focus = Focus::Chat;
            } else {
                self.focus = Focus::Notes;
            }
            return;
        }
        if let K::Ctrl('p') = k {
            self.cfg.chat_open = false;
            self.cfg.pdf_open = !self.cfg.pdf_open;
            self.focus = if self.cfg.pdf_open { Focus::Pdf } else { Focus::Notes };
            return;
        }
        if self.focus == Focus::Pdf && !matches!(k, K::Tab | K::Ch(':') | K::Ch('?') | K::Ctrl(_)) {
            if self.pdf_key(&prefix, k, n) {
                return;
            }
        }
        if self.edit_key(d, &prefix, k, n, &nrows) {
            return;
        }
        match (prefix.as_str(), k) {
            ("g", K::Ch('g')) => self.jump(0, trows.len(), nrows.len()),
            ("g", K::Ch('d')) if self.focus == Focus::Notes => {
                // go to the [[ref]] on this line (the nth with a count)
                if let Some((id, _)) = nrows.get(self.note_cur) {
                    let l = d.kb.line_of(id);
                    let refs: Vec<String> = l.front.iter().chain(&l.back).filter_map(|s| if let rich::Span::Ref { id, .. } = s { Some(id.clone()) } else { None }).collect();
                    match refs.get(n.saturating_sub(1)).or(refs.first()) {
                        Some(r) => self.goto(d, r),
                        None => self.msg = Some("no [[ref]] on this line".into()),
                    }
                }
            }
            ("z", K::Ch(c)) => self.fold(d, &nrows, c),
            (_, K::Ch('g')) => self.pending = format!("{digits}g"),
            (_, K::Ch('z')) => self.pending.push('z'),
            (_, K::Ch(':')) => self.overlay = Overlay::Cmd(String::new()),
            (_, K::Ch('/')) => self.overlay = Overlay::Cmd("/".into()),
            (_, K::Ch('n')) if self.focus == Focus::Notes => self.deferred.push(Deferred::Search(1)),
            (_, K::Ch('N')) if self.focus == Focus::Notes => self.deferred.push(Deferred::Search(-1)),
            (_, K::Ch('?')) => self.overlay = Overlay::Help { scroll: 0, tab: keys::tab_for(self.context()) },
            (_, K::Ctrl('t')) => self.overlay = Overlay::Find { q: String::new(), items: vec![], cur: 0, for_q: None },
            (_, K::Ch('G')) => self.jump(usize::MAX, trows.len(), nrows.len()),
            (_, K::Tab) => self.toggle_focus(),
            (_, K::Ctrl('h')) if self.cfg.sidebar_open => self.focus = Focus::Tree,
            (_, K::Ctrl('l')) => self.focus = Focus::Notes,
            (_, K::Ctrl('b')) => {
                self.cfg.sidebar_open = !self.cfg.sidebar_open;
                if !self.cfg.sidebar_open {
                    self.focus = Focus::Notes;
                }
            }
            (_, K::Ctrl('o')) => {
                if let Some((p, c)) = self.jumps.pop() {
                    self.week = Some(p);
                    self.note_cur = c;
                    self.scroll_notes = true;
                    self.focus = Focus::Notes;
                }
            }
            (_, K::Ctrl('j')) => self.step_week(d, n as isize),
            (_, K::Ctrl('k')) => self.step_week(d, -(n as isize)),
            (_, K::Ctrl('d')) => self.mv(HALF_PAGE as isize * n as isize, trows.len(), nrows.len()),
            (_, K::Ctrl('u')) => self.mv(-(HALF_PAGE as isize) * n as isize, trows.len(), nrows.len()),
            (_, K::Ch('j') | K::Down) => self.mv(n as isize, trows.len(), nrows.len()),
            (_, K::Ch('k') | K::Up) => self.mv(-(n as isize), trows.len(), nrows.len()),
            (_, K::Ch('<')) if self.focus == Focus::Tree => self.cfg.sidebar_w = (self.cfg.sidebar_w - 20.0 * n as f32).max(SIDEBAR_MIN),
            (_, K::Ch('>')) if self.focus == Focus::Tree => self.cfg.sidebar_w = (self.cfg.sidebar_w + 20.0 * n as f32).min(SIDEBAR_MAX),
            (_, K::Ch('l') | K::Right | K::Enter) => match self.focus {
                Focus::Tree => {
                    if let Some((id, is_mod)) = trows.get(self.tree_cur).cloned() {
                        if is_mod && !self.open_mods.contains(&id) {
                            self.open_mods.insert(id);
                        } else if is_mod {
                            self.tree_cur += 1;
                        } else {
                            self.open_week(id);
                        }
                        self.scroll_tree = true;
                    }
                }
                Focus::Pdf | Focus::Chat => {}
                Focus::Notes => {
                    if let Some((id, depth)) = nrows.get(self.note_cur).cloned() {
                        if self.folded.remove(&id) {
                        } else if matches!(k, K::Enter) {
                            if !d.kb.children(Some(&id)).is_empty() {
                                self.folded.insert(id);
                            }
                        } else if nrows.get(self.note_cur + 1).is_some_and(|r| r.1 > depth) {
                            self.note_cur += 1;
                            self.scroll_notes = true;
                        }
                    }
                }
            },
            (_, K::Ch('h') | K::Left) => match self.focus {
                Focus::Tree => {
                    if let Some((id, is_mod)) = trows.get(self.tree_cur).cloned() {
                        if is_mod {
                            self.open_mods.remove(&id);
                        } else if let Some(p) = trows[..self.tree_cur].iter().rposition(|r| r.1) {
                            self.tree_cur = p;
                        }
                        self.scroll_tree = true;
                    }
                }
                Focus::Pdf | Focus::Chat => self.focus = Focus::Notes,
                Focus::Notes => match nrows.get(self.note_cur) {
                    Some((_, 0)) | None => {
                        if self.cfg.sidebar_open {
                            self.focus = Focus::Tree;
                        }
                    }
                    Some((_, depth)) => {
                        if let Some(p) = nrows[..self.note_cur].iter().rposition(|r| r.1 < *depth) {
                            self.note_cur = p;
                            self.scroll_notes = true;
                        }
                    }
                },
            },
            (_, K::Esc) => {}
            _ => {}
        }
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Tree => Focus::Notes,
            Focus::Notes if self.cfg.pdf_open => Focus::Pdf,
            Focus::Notes if self.cfg.chat_open => Focus::Chat,
            Focus::Chat if self.cfg.sidebar_open => Focus::Tree,
            Focus::Chat => Focus::Notes,
            Focus::Notes | Focus::Pdf if self.cfg.sidebar_open => Focus::Tree,
            Focus::Pdf => Focus::Notes,
            f => f,
        };
    }

    /// Jump to the next (dir 1) / previous (-1) bullet on the page containing the last search.
    fn search_notes(&mut self, dir: isize) {
        let (Some(q), Some(Ok(d)), Some(w)) = (self.last_search.clone(), &self.data, self.week.clone()) else { return };
        let ql = q.to_lowercase();
        // search folded rems too: unfold everything on the page first
        let all = note_rows(&d.kb, &w, &HashSet::new());
        let n = all.len() as isize;
        if n == 0 {
            return;
        }
        let rows = note_rows(&d.kb, &w, &self.folded);
        let cur_id = rows.get(self.note_cur).map(|r| r.0.clone());
        let start = cur_id.and_then(|c| all.iter().position(|r| r.0 == c)).unwrap_or(0) as isize;
        let name = |id: &str| d.kb.name(id);
        for k in 1..=n {
            let i = (start + dir * k).rem_euclid(n) as usize;
            let l = d.kb.line_of(&all[i].0);
            let text = format!("{} {}", rich::plain(&l.front, &name), rich::plain(&l.back, &name)).to_lowercase();
            if text.contains(&ql) {
                let id = all[i].0.clone();
                // unfold its ancestors
                let mut cur = id.clone();
                while let Some(p) = d.kb.q.get(&cur).and_then(|x| x.get("parent")).and_then(|p| p.as_str()).map(str::to_owned) {
                    if p == w {
                        break;
                    }
                    self.folded.remove(&p);
                    cur = p;
                }
                let rows = note_rows(&d.kb, &w, &self.folded);
                self.note_cur = rows.iter().position(|r| r.0 == id).unwrap_or(0);
                self.scroll_notes = true;
                self.focus = Focus::Notes;
                self.msg = Some(format!("/{q}"));
                return;
            }
        }
        self.msg = Some(format!("not found on this page: {q}"));
    }

    fn toggle_graph(&mut self, d: &Loaded, rows: &[(String, usize)]) {
        self.view_graph = !self.view_graph;
        if !self.view_graph {
            return;
        }
        self.focus = Focus::Notes;
        if self.graph.is_none() {
            self.graph = Some(graph::Graph::build(&d.kb, &d.mods));
        }
        // select the node nearest the cursor rem (or the open page)
        let g = self.graph.as_mut().unwrap();
        let mut cur = rows.get(self.note_cur).map(|r| r.0.clone()).or(self.week.clone());
        while let Some(c) = cur {
            if g.index.contains_key(&c) {
                g.select_id(&c);
                break;
            }
            cur = d.kb.q.get(&c).and_then(|x| x.get("parent")).and_then(|p| p.as_str()).map(str::to_owned);
        }
    }

    fn graph_key(&mut self, d: &Loaded, k: K, n: usize) -> bool {
        if !matches!(k, K::Tab | K::ShiftTab) {
            self.graph_anchor = None;
        }
        let Some(g) = self.graph.as_mut() else { return false };
        let _ = n;
        match k {
            K::Ch('h') | K::Left => g.step(Vec2::new(-1.0, 0.0)),
            K::Ch('l') | K::Right => g.step(Vec2::new(1.0, 0.0)),
            K::Ch('j') | K::Down => g.step(Vec2::new(0.0, 1.0)),
            K::Ch('k') | K::Up => g.step(Vec2::new(0.0, -1.0)),
            K::Ch('L') => {
                let on = g.local.is_none();
                g.set_local(on);
            }
            K::Ch('f') => g.cycle_filter(),
            K::Tab => g.walk_neighbours(1, &mut self.graph_anchor),
            K::ShiftTab => g.walk_neighbours(-1, &mut self.graph_anchor),
            K::Ch('/') => self.overlay = Overlay::Cmd("/".into()),
            K::Ch('+') => {
                g.scale = (g.scale * 1.25).min(6.0);
                g.fit = false;
                g.follow = g.sel.is_some();
            }
            K::Ch('-') => {
                g.scale = (g.scale / 1.25).max(0.02);
                g.fit = false;
                g.follow = g.sel.is_some();
            }
            K::Ch('c') => {
                g.fit = false;
                g.follow = true;
            }
            K::Ch('=') => {
                g.fit = true;
                g.follow = false;
            }
            K::Ch('r') => {
                self.graph = Some(graph::Graph::build(&d.kb, &d.mods));
            }
            K::Enter => {
                if let Some(id) = g.sel.map(|s| g.nodes[s].id.clone()) {
                    self.view_graph = false;
                    self.goto(d, &id);
                }
            }
            K::Esc => self.view_graph = false,
            _ => return false,
        }
        true
    }

    /// Keys while the PDF panel has focus. Returns true if handled.
    fn pdf_key(&mut self, prefix: &str, k: K, n: usize) -> bool {
        // link hints: letters pick a link, esc cancels
        if self.pdfs.hinting.is_some() {
            match k {
                K::Ch(c) if c.is_ascii_lowercase() => self.pdfs.hint_key(c),
                _ => self.pdfs.hinting = None,
            }
            return true;
        }
        match k {
            K::Ch('f') if !self.pdfs.links.is_empty() => {
                self.pdfs.hinting = Some(String::new());
                return true;
            }
            K::Ch('b') | K::Back => {
                if !self.pdfs.go_back() {
                    self.msg = Some("nothing to go back to".into());
                }
                return true;
            }
            _ => {}
        }
        // non-PDF files scroll as text; a toggles quiz answers
        if self.pdfs.current_kind().is_some_and(|x| x != files::FileKind::Pdf) {
            let line = 60.0 * n as f32;
            match (prefix, k) {
                ("g", K::Ch('g')) => {
                    self.pdfs.text_scroll = 0.0;
                    self.pdfs.set_text_scroll = true;
                    return true;
                }
                (_, K::Ch('g')) => {
                    self.pending.push('g');
                    return true;
                }
                (_, K::Ch('G')) => {
                    self.pdfs.scroll_text(1.0e7);
                    return true;
                }
                (_, K::Ch('j') | K::Down) => {
                    self.pdfs.scroll_text(line);
                    return true;
                }
                (_, K::Ch('k') | K::Up) => {
                    self.pdfs.scroll_text(-line);
                    return true;
                }
                (_, K::Ctrl('d') | K::Ch('J')) => {
                    self.pdfs.scroll_text(400.0 * n as f32);
                    return true;
                }
                (_, K::Ctrl('u') | K::Ch('K')) => {
                    self.pdfs.scroll_text(-400.0 * n as f32);
                    return true;
                }
                (_, K::Ch('a')) => {
                    self.pdfs.show_answers = !self.pdfs.show_answers;
                    return true;
                }
                _ => {}
            }
        }
        let line = 60.0 * n as f32;
        let half = 400.0 * n as f32;
        let heights = self.pdfs.page_heights(self.cfg.pdf_w * self.pdfs.zoom);
        let page_top = |i: usize| heights.iter().take(i).map(|h| h + 10.0).sum::<f32>();
        let cur_page = {
            let mut acc = 0.0;
            let mut pg = 0;
            for (i, h) in heights.iter().enumerate() {
                if acc + h / 2.0 > self.pdfs.scroll {
                    pg = i;
                    break;
                }
                acc += h + 10.0;
                pg = i;
            }
            pg
        };
        match (prefix, k) {
            ("g", K::Ch('g')) => self.pdfs.scroll = 0.0,
            (_, K::Ch('g')) => self.pending.push('g'),
            (_, K::Ch('G')) => self.pdfs.scroll = f32::MAX,
            (_, K::Ch('j') | K::Down) => self.pdfs.scroll += line,
            (_, K::Ch('k') | K::Up) => self.pdfs.scroll = (self.pdfs.scroll - line).max(0.0),
            (_, K::Ctrl('d')) => self.pdfs.scroll += half,
            (_, K::Ctrl('u')) => self.pdfs.scroll = (self.pdfs.scroll - half).max(0.0),
            (_, K::Ch('J')) => self.pdfs.scroll = page_top((cur_page + n).min(heights.len().saturating_sub(1))),
            (_, K::Ch('K')) => self.pdfs.scroll = page_top(cur_page.saturating_sub(n)),
            (_, K::Ch(']')) => self.pdfs.step(n as isize),
            (_, K::Ch('[')) => self.pdfs.step(-(n as isize)),
            (_, K::Ch('+')) => self.pdfs.zoom = (self.pdfs.zoom * 1.15).min(4.0),
            (_, K::Ch('-')) => self.pdfs.zoom = (self.pdfs.zoom / 1.15).max(0.4),
            (_, K::Ch('=')) => self.pdfs.zoom = 1.0,
            (_, K::Ch('h') | K::Left) => self.focus = Focus::Notes,
            (_, K::Ch('o') | K::Enter) => self.overlay = Overlay::Pdfs { cur: self.pdfs.cur },
            (_, K::Ch('<')) => self.cfg.pdf_w = (self.cfg.pdf_w + 40.0 * n as f32).min(1400.0),
            (_, K::Ch('>')) => self.cfg.pdf_w = (self.cfg.pdf_w - 40.0 * n as f32).max(280.0),
            _ => return false,
        }
        true
    }

    fn mv(&mut self, by: isize, tn: usize, nn: usize) {
        let (cur, len, flag) = match self.focus {
            Focus::Tree => (&mut self.tree_cur, tn, &mut self.scroll_tree),
            Focus::Notes | Focus::Pdf | Focus::Chat => (&mut self.note_cur, nn, &mut self.scroll_notes),
        };
        if len > 0 {
            *cur = (*cur as isize + by).clamp(0, len as isize - 1) as usize;
            *flag = true;
        }
    }

    fn jump(&mut self, to: usize, tn: usize, nn: usize) {
        match self.focus {
            Focus::Tree => self.tree_cur = to.min(tn.saturating_sub(1)),
            Focus::Notes | Focus::Pdf | Focus::Chat => self.note_cur = to.min(nn.saturating_sub(1)),
        }
        self.scroll_tree = true;
        self.scroll_notes = true;
    }

    fn fold(&mut self, d: &Loaded, rows: &[(String, usize)], c: char) {
        let Some((id, _)) = rows.get(self.note_cur).cloned() else { return };
        let has_kids = !d.kb.children(Some(&id)).is_empty();
        match c {
            'a' if has_kids => {
                if !self.folded.remove(&id) {
                    self.folded.insert(id);
                }
            }
            'c' if has_kids => {
                self.folded.insert(id);
            }
            'o' => {
                self.folded.remove(&id);
            }
            'M' => {
                for (r, _) in rows {
                    if !d.kb.children(Some(r)).is_empty() {
                        self.folded.insert(r.clone());
                    }
                }
                // keep the cursor on the same rem's top-level ancestor
                let mut top = id.clone();
                while let Some(p) = d.kb.q[&top].get("parent").and_then(|p| p.as_str()) {
                    if Some(p) == self.week.as_deref() {
                        break;
                    }
                    top = p.to_owned();
                }
                let after = note_rows(&d.kb, self.week.as_deref().unwrap_or(""), &self.folded);
                self.note_cur = after.iter().position(|r| r.0 == top).unwrap_or(0);
            }
            'R' => self.folded.clear(),
            _ => {}
        }
        self.scroll_notes = true;
    }

    fn step_week(&mut self, d: &Loaded, by: isize) {
        // weeks across all modules, in sidebar order
        let all: Vec<(String, String)> = d
            .mods
            .iter()
            .flat_map(|m| d.kb.children(Some(m)).iter().filter(|w| d.kb.name(w).starts_with("Week")).map(move |w| (m.clone(), w.clone())))
            .collect();
        if all.is_empty() {
            return;
        }
        let i = self.week.as_ref().and_then(|w| all.iter().position(|x| &x.1 == w));
        let j = match i {
            Some(i) => (i as isize + by).clamp(0, all.len() as isize - 1) as usize,
            None => 0,
        };
        let (m, w) = all[j].clone();
        self.open_mods.insert(m);
        self.tree_cur = tree_rows(d, &self.open_mods).iter().position(|r| r.0 == w).unwrap_or(0);
        self.scroll_tree = true;
        let f = self.focus;
        self.open_week(w);
        self.focus = f;
    }

    fn handle_overlay(&mut self, ctx: &egui::Context, k: K) {
        match &mut self.overlay {
            Overlay::Help { scroll, tab } => match k {
                K::Ch('l') | K::Right | K::Tab => {
                    *tab = (*tab + 1) % keys::HELP.len();
                    *scroll = 0;
                }
                K::Ch('h') | K::Left | K::ShiftTab => {
                    *tab = (*tab + keys::HELP.len() - 1) % keys::HELP.len();
                    *scroll = 0;
                }
                K::Ch(c @ '1'..='9') => {
                    *tab = ((c as u8 - b'1') as usize).min(keys::HELP.len() - 1);
                    *scroll = 0;
                }
                K::Ch('j') | K::Down => *scroll += 1,
                K::Ch('k') | K::Up => *scroll = scroll.saturating_sub(1),
                K::Ctrl('d') => *scroll += 12,
                K::Ctrl('u') => *scroll = scroll.saturating_sub(12),
                K::Ch('G') => *scroll = 999,
                K::Ch('g') => *scroll = 0,
                K::Esc | K::Ch('q') | K::Ch('?') | K::Enter => self.overlay = Overlay::None,
                _ => {}
            },
            Overlay::Find { q, items, cur, .. } => match k {
                K::Ch(c) => {
                    q.push(c);
                    *cur = 0;
                }
                K::Back => {
                    q.pop();
                    *cur = 0;
                }
                K::Down | K::Ctrl('n') | K::Tab => *cur = (*cur + 1).min(items.len().saturating_sub(1)),
                K::Up | K::Ctrl('p') | K::ShiftTab => *cur = cur.saturating_sub(1),
                K::Enter => {
                    if let Some((id, _)) = items.get(*cur).cloned() {
                        self.overlay = Overlay::None;
                        self.deferred.push(Deferred::Goto(id));
                    }
                }
                K::Esc => self.overlay = Overlay::None,
                _ => {}
            },
            Overlay::Cmd(s) => match k {
                K::Ch(c) => s.push(c),
                K::Back => {
                    if s.pop().is_none() {
                        self.overlay = Overlay::None;
                    }
                }
                K::Esc => self.overlay = Overlay::None,
                K::Enter => {
                    let cmd = s.trim().to_owned();
                    self.overlay = Overlay::None;
                    self.run(ctx, &cmd);
                }
                _ => {}
            },
            Overlay::Db { list, cur } => match k {
                K::Ch('j') | K::Down => *cur = (*cur + 1).min(list.len().saturating_sub(1)),
                K::Ch('k') | K::Up => *cur = cur.saturating_sub(1),
                K::Esc | K::Ch('q') => self.overlay = Overlay::None,
                K::Enter | K::Ch('l') => {
                    if let Some(c) = list.get(*cur) {
                        let p = c.path.clone();
                        self.cfg.db = Some(p.clone());
                        self.overlay = Overlay::None;
                        self.reload(ctx, Some(p));
                    }
                }
                _ => {}
            },
            Overlay::Diff { scroll } => match k {
                K::Ch('j') | K::Down => *scroll += 1,
                K::Ch('k') | K::Up => *scroll = scroll.saturating_sub(1),
                K::Enter => {
                    self.overlay = Overlay::None;
                    self.deferred.push(Deferred::Commit);
                }
                K::Esc | K::Ch('q') => self.overlay = Overlay::None,
                _ => {}
            },
            Overlay::Log { entries, cur } => match k {
                K::Ch('j') | K::Down => *cur = (*cur + 1).min(entries.len().saturating_sub(1)),
                K::Ch('k') | K::Up => *cur = cur.saturating_sub(1),
                K::Enter => {
                    if let Some(e) = entries.get(*cur) {
                        let ops = e.ops.clone();
                        self.overlay = Overlay::None;
                        self.deferred.push(Deferred::Revert(ops));
                    }
                }
                K::Esc | K::Ch('q') => self.overlay = Overlay::None,
                _ => {}
            },
            Overlay::Link(lp) => match k {
                K::Ch('j') | K::Down | K::Ctrl('n') => lp.cur = (lp.cur + 1).min(lp.items.len().saturating_sub(1)),
                K::Ch('k') | K::Up | K::Ctrl('p') => lp.cur = lp.cur.saturating_sub(1),
                K::Enter => {
                    let (rem, card) = (lp.rem.clone(), lp.items.get(lp.cur).map(|x| x.0));
                    self.overlay = Overlay::None;
                    if let Some(card) = card {
                        self.deferred.push(Deferred::Link(rem, card));
                    }
                }
                K::Esc | K::Ch('q') => self.overlay = Overlay::None,
                _ => {}
            },
            Overlay::Pdfs { cur } => match k {
                K::Ch('j') | K::Down | K::Ctrl('n') => *cur = (*cur + 1).min(self.pdfs.files.len().saturating_sub(1)),
                K::Ch('k') | K::Up | K::Ctrl('p') => *cur = cur.saturating_sub(1),
                K::Ch('g') => *cur = 0,
                K::Ch('G') => *cur = self.pdfs.files.len().saturating_sub(1),
                K::Enter | K::Ch('l') => {
                    let c = *cur;
                    self.overlay = Overlay::None;
                    if c < self.pdfs.files.len() {
                        self.pdfs.cur = c;
                        self.pdfs.open_cur();
                        self.cfg.pdf_open = true;
                        self.cfg.chat_open = false;
                        self.focus = Focus::Pdf;
                    }
                }
                K::Esc | K::Ch('q') | K::Ch('o') => self.overlay = Overlay::None,
                _ => {}
            },
            Overlay::None => {}
        }
    }

    /// Stage the inverse of a past commit (newest op first) for review with :w.
    fn stage_revert(&mut self, ops: &[edit::Op]) {
        let Some(Ok(d)) = &mut self.data else { return };
        let mut n = 0;
        for op in ops.iter().rev() {
            let inv = match op {
                // undoing a create = deleting it now, with a snapshot so undo works
                edit::Op::Create { id, .. } => {
                    let Some(p) = d.kb.q.get(id).and_then(|x| x.get("parent")).and_then(|p| p.as_str()).map(str::to_owned) else { continue };
                    let pos = d.kb.children(Some(&p)).iter().position(|k| k == id).unwrap_or(0);
                    edit::Op::Delete { id: id.clone(), parent: p, pos, docs: edit::snapshot(&d.kb, id) }
                }
                other => edit::inverse(other),
            };
            self.changes.apply(&mut d.kb, inv);
            n += 1;
        }
        self.msg = Some(format!("staged {n} ops reverting that commit: :w to review and commit"));
    }

    fn start_commit(&mut self) {
        if self.committing.is_some() {
            self.msg = Some("a commit is already running".into());
            return;
        }
        let ops = self.changes.ops.clone();
        if ops.is_empty() {
            self.msg = Some("nothing staged".into());
            return;
        }
        if let Err(e) = commit::validate(&ops) {
            self.msg = Some(e);
            return;
        }
        let Some(Ok(d)) = &self.data else { return };
        // children of rems becoming >>> cards, as they are now
        let mut children = std::collections::HashMap::new();
        for op in &ops {
            if let edit::Op::Update { id, after, .. } = op {
                if after.kind == "multiline" {
                    children.insert(id.clone(), d.kb.children(Some(id)).to_vec());
                }
            }
        }
        let bridge = self.bridge.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            if !bridge.is_online() {
                // RemNote loads the plugin from us at its startup: start it if it isn't running
                let started = !bridge::remnote_running() && bridge::launch_remnote().is_ok();
                if !bridge.wait_online(if started { 120 } else { 75 }) {
                    let err = if started {
                        "started RemNote but the bridge plugin didn't connect within 2 min".to_string()
                    } else {
                        "RemNote is running without the bridge: restart RemNote so it loads the plugin from m3m0".to_string()
                    };
                    let _ = tx.send((commit::Outcome { done: 0, idmap: Default::default(), err: Some(err) }, ops));
                    return;
                }
            }
            let out = commit::run(&bridge, &ops, &children);
            let _ = tx.send((out, ops));
        });
        self.committing = Some(rx);
        self.msg = Some("committing…".into());
    }

    /// Apply a finished commit: log what went through, map temporary ids to real ones.
    fn finish_commit(&mut self, out: commit::Outcome, ops: Vec<edit::Op>) {
        let m = &out.idmap;
        let done: Vec<edit::Op> = ops[..out.done].iter().map(|o| commit::rename_op(o, m)).collect();
        if !done.is_empty() {
            commit::append_log(&done);
        }
        let rest: Vec<edit::Op> = self.changes.ops.drain(..).skip(out.done).map(|o| commit::rename_op(&o, m)).collect();
        self.changes.ops = rest;
        self.changes.redo.clear();
        if let Some(Ok(d)) = &mut self.data {
            rename_ids(&mut d.kb, m);
        }
        self.folded = self.folded.iter().map(|f| m.get(f).cloned().unwrap_or_else(|| f.clone())).collect();
        if let Some(w) = &self.week {
            if let Some(r) = m.get(w) {
                self.week = Some(r.clone());
            }
        }
        if self.quit_after_commit && out.err.is_none() {
            self.changes = edit::Changes::default();
            self.quit_after_commit = false;
            self.quit_now = true;
        }
        self.quit_after_commit = false;
        self.msg = Some(match out.err {
            None => format!("committed {} changes to RemNote", out.done),
            Some(e) => format!("committed {} of {}; stopped: {e} (the rest stay staged)", out.done, ops.len()),
        });
    }

    fn reload(&mut self, ctx: &egui::Context, db: Option<PathBuf>) {
        self.rx = Self::spawn_load(ctx, db);
        self.data = None;
        self.reloaded = true;
        self.week = None;
        self.open_mods.clear();
        self.tree_cur = 0;
        self.note_cur = 0;
        self.folded.clear();
    }

    fn run(&mut self, ctx: &egui::Context, cmd: &str) {
        if let Some(q) = cmd.strip_prefix('/') {
            if q.is_empty() {
                return;
            }
            self.last_search = Some(q.to_owned());
            if self.view_graph {
                if let Some(g) = self.graph.as_mut() {
                    if !g.search(q) {
                        self.msg = Some(format!("no node matches {q}"));
                    }
                }
            } else {
                self.deferred.push(Deferred::Search(1));
            }
            return;
        }
        let mut it = cmd.split_whitespace();
        match (it.next(), it.next()) {
            (Some("q!"), _) => {
                self.changes = edit::Changes::default();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            (Some("q" | "quit"), _) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            (Some("wq" | "x"), _) => {
                self.quit_after_commit = true;
                self.deferred.push(Deferred::Commit);
            }
            (Some("db"), _) => {
                let list = kb::find_dbs();
                let cur = self
                    .data
                    .as_ref()
                    .and_then(|d| d.as_ref().ok())
                    .and_then(|d| list.iter().position(|c| c.path == d.db))
                    .unwrap_or(0);
                self.overlay = Overlay::Db { list, cur };
            }
            (Some("reload" | "e"), _) if !self.changes.ops.is_empty() => {
                self.msg = Some(format!("{} staged {} would be lost: :w first, or :e! to drop them", self.changes.ops.len(), plural(self.changes.ops.len(), "change")));
            }
            (Some("reload" | "e" | "e!"), _) => {
                self.changes = edit::Changes::default();
                let db = kb::resolve_db(self.cfg.db.as_deref());
                self.reload(ctx, db);
            }
            (Some("zoom"), Some(z)) => match z.trim_end_matches('%').parse::<f32>() {
                Ok(z) => {
                    let z = if z > 4.0 { z / 100.0 } else { z };
                    ctx.set_zoom_factor(z.clamp(0.6, 3.0));
                }
                Err(_) => self.msg = Some(format!("zoom: not a number: {z}")),
            },
            (Some("w" | "diff"), _) => {
                if self.changes.ops.is_empty() {
                    self.msg = Some("nothing staged".into());
                } else {
                    self.overlay = Overlay::Diff { scroll: 0 };
                }
            }
            (Some("w!"), _) => self.deferred.push(Deferred::Commit),
            (Some("log"), _) => self.overlay = Overlay::Log { entries: commit::read_log(), cur: 0 },
            (Some("help" | "h"), _) => self.overlay = Overlay::Help { scroll: 0, tab: 0 },
            (Some("review"), arg) => {
                if let Some(Ok(d)) = &self.data {
                    let _ = d;
                }
                self.deferred.push(Deferred::Review(arg.map(str::to_owned)));
            }
            (Some("pdf" | "pdfs"), _) => {
                self.cfg.chat_open = false;
                self.cfg.pdf_open = true;
                self.focus = Focus::Pdf;
                self.overlay = Overlay::Pdfs { cur: self.pdfs.cur };
            }
            (Some("remnote"), arg) => {
                let restart = arg == Some("restart");
                if restart && bridge::remnote_running() {
                    bridge::stop_remnote();
                    std::thread::sleep(std::time::Duration::from_millis(1500));
                }
                self.msg = Some(match (bridge::remnote_running(), bridge::launch_remnote()) {
                    (true, _) => "RemNote is already running (:remnote restart to reload the plugin)".into(),
                    (false, Ok(())) => "starting RemNote: the bridge comes online when it has loaded".into(),
                    (false, Err(e)) => e,
                });
            }
            (Some("find" | "f"), _) => self.overlay = Overlay::Find { q: String::new(), items: vec![], cur: 0, for_q: None },
            (Some(""), _) | (None, _) => {}
            (Some(c), _) => self.msg = Some(format!("not a command: {c}")),
        }
    }
}

impl eframe::App for App {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }

    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if self.data.is_none() {
            if let Ok(d) = self.rx.try_recv() {
                self.data = Some(d);
                self.preselect();
            }
        }
        // first run: pick a zoom from the monitor (the laptop is 2560x1600 at 1.5x)
        if self.cfg.zoom.is_none() {
            let z = match ctx.input(|i| i.viewport().monitor_size) {
                Some(m) if m.x > 2200.0 => (m.x / 1706.0 * 1.15).clamp(1.0, 2.5),
                Some(_) => 1.15,
                None => 1.15,
            };
            ctx.set_zoom_factor(z);
            self.cfg.zoom = Some(z);
        }

        if self.boot.finished {
            let mut ks = if self.editing.is_some() {
                self.insert_keys(ctx)
            } else if self.focus == Focus::Chat && self.cfg.chat_open {
                self.chat_keys(ctx)
            } else {
                self.keys(ctx)
            };
            if std::env::var("M3M0_SHOTS").is_ok() {
                ks.clear(); // previews: only replayed keys
            }
            // M3M0_KEYS replays keys after boot, for previews ("~b" = ctrl-b,
            // "\n" = enter, "\x1b" = esc, "|" = wait ~0.6 s for the UI to settle)
            if let (Ok(s), false) = (std::env::var("M3M0_KEYS"), self.replayed) {
                let now = ctx.input(|i| i.time);
                if self.data.as_ref().is_some_and(|d| d.is_ok()) && now >= self.replay_at {
                    let chunks: Vec<&str> = s.split('|').collect();
                    if let Some(chunk) = chunks.get(self.replay_chunk) {
                        let mut it = chunk.chars();
                        while let Some(c) = it.next() {
                            ks.push(match c {
                                '~' => K::Ctrl(it.next().unwrap_or(' ')),
                                '\n' => K::Enter,
                                '\u{1b}' => K::Esc,
                                c => K::Ch(c),
                            });
                        }
                        self.replay_chunk += 1;
                        self.replay_at = now + 0.6;
                    }
                    if self.replay_chunk >= chunks.len() {
                        self.replayed = true;
                    }
                }
                ctx.request_repaint();
            }
            for k in ks {
                self.handle(ctx, k);
            }
        }
        for job in std::mem::take(&mut self.deferred) {
            match job {
                Deferred::Revert(ops) => self.stage_revert(&ops),
                Deferred::Commit => self.start_commit(),
                Deferred::Search(dir) => self.search_notes(dir),
                Deferred::Review(arg) => {
                    if let Some(Ok(d)) = self.data.take() {
                        let scope = match arg.as_deref() {
                            Some("all") => None,
                            Some("module") => self.week.as_ref().and_then(|w| d.kb.module_of(w)),
                            _ => self.week.clone(),
                        };
                        self.start_review(&d, scope);
                        self.data = Some(Ok(d));
                    }
                }
                Deferred::Goto(id) => {
                    if let Some(Ok(d)) = self.data.take() {
                        self.view_graph = false;
                        self.cards = None;
                        if d.mods.contains(&id) {
                            self.open_mods.insert(id.clone());
                            self.focus = Focus::Tree;
                            self.tree_cur = tree_rows(&d, &self.open_mods).iter().position(|r| r.0 == id).unwrap_or(0);
                            self.scroll_tree = true;
                        } else if d.kb.q.get(&id).and_then(|x| x.get("parent")).and_then(|p| p.as_str()).is_some_and(|p| d.mods.iter().any(|m| m == p)) {
                            // a week: open it as the page
                            if let Some(w) = &self.week {
                                self.jumps.push((w.clone(), self.note_cur));
                            }
                            let m = d.kb.q[&id]["parent"].as_str().unwrap_or_default().to_owned();
                            self.open_mods.insert(m);
                            self.tree_cur = tree_rows(&d, &self.open_mods).iter().position(|r| r.0 == id).unwrap_or(0);
                            self.scroll_tree = true;
                            self.open_week(id);
                        } else {
                            self.goto(&d, &id);
                        }
                        self.data = Some(Ok(d));
                    }
                }
                Deferred::Link(rem, card) => {
                    if let Some(Ok(d)) = self.data.take() {
                        let reopen = self.cards.is_some();
                        if !reopen {
                            self.open_cards(&d);
                        }
                        self.do_link(&d, &rem, card);
                        if !reopen {
                            self.cards = None;
                        }
                        self.data = Some(Ok(d));
                    }
                }
            }
        }
        // don't lose staged edits: closing with changes staged asks first
        if ctx.input(|i| i.viewport().close_requested()) && !self.changes.ops.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.msg = Some(format!("{} staged {}: :w to commit, :wq to commit and quit, :q! to quit and drop them", self.changes.ops.len(), plural(self.changes.ops.len(), "change")));
        }
        if let Some(rx) = &self.committing {
            if let Ok((out, ops)) = rx.try_recv() {
                self.committing = None;
                self.finish_commit(out, ops);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        if self.quit_now {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        edge_resize(ctx);

        self.titlebar(ctx);
        self.statusbar(ctx);

        let taken = self.data.take();
        let data = match &taken {
            Some(Ok(d)) => d,
            other => {
                let err = match other {
                    Some(Err(e)) => Some(e.clone()),
                    _ => None,
                };
                egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(24.0)).show(ctx, |ui| {
                    match err {
                        Some(e) => ui.colored_label(RED_BRIGHT, format!("{e}\n\n:db to pick another database")),
                        None => ui.label(RichText::new("loading…").color(COMMENT)),
                    };
                });
                self.data = taken;
                self.overlays(ctx);
                self.boot.show(ctx, None, BAR_LOGO);
                return;
            }
        };

        self.sidebar(ctx, data);
        self.pdf_panel(ctx, data);
        self.chat_panel(ctx, data);
        self.notes(ctx, data);
        self.boot.show(ctx, Some(&data.log), BAR_LOGO);
        self.data = taken;
        self.overlays(ctx);

        self.cfg.zoom = Some(ctx.zoom_factor());
        if self.cfg != self.saved && !ctx.input(|i| i.pointer.any_down()) && std::env::var("M3M0_SHOTS").is_err() {
            self.cfg.save();
            self.saved = self.cfg.clone();
        }
        shots(ctx);
    }
}

// --- panels ---------------------------------------------------------------------

impl App {
    fn titlebar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("bar").exact_height(BAR_H).frame(egui::Frame::new().fill(BG)).show(ctx, |ui| {
            let r = ui.max_rect();
            // drag anywhere on the bar to move, double-click to maximise
            let bar = ui.interact(r, egui::Id::new("bar-drag"), Sense::click_and_drag());
            let maxed = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            if bar.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if bar.double_clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maxed));
            }
            let p = ui.painter();
            if self.boot.finished {
                boot::paint_logo(p, BAR_LOGO.0, BAR_LOGO.1, 1.0);
            }
            p.line_segment([r.left_bottom(), r.right_bottom()], (1.0, GREY0));
            if let Some(Ok(d)) = &self.data {
                let m = self.week.as_ref().and_then(|w| d.kb.q[w].get("parent").and_then(|p| p.as_str())).map(|m| d.kb.name(m));
                let w = self.week.as_ref().map(|w| d.kb.name(w));
                let crumbs: Vec<String> = [m, w].into_iter().flatten().collect();
                p.text(Pos2::new(92.0, r.center().y), Align2::LEFT_CENTER, crumbs.join("  /  "), FontId::monospace(13.0), FG_DIM);
            }
            // window buttons
            let bw = 40.0;
            let buttons: [(&str, egui::ViewportCommand, bool); 3] = [
                ("\u{f00d}", egui::ViewportCommand::Close, true),
                (if maxed { "\u{f2d2}" } else { "\u{f2d0}" }, egui::ViewportCommand::Maximized(!maxed), false),
                ("\u{f2d1}", egui::ViewportCommand::Minimized(true), false),
            ];
            for (i, (glyph, cmd, danger)) in buttons.into_iter().enumerate() {
                let br = Rect::from_min_size(Pos2::new(r.right() - bw * (i as f32 + 1.0), r.top()), Vec2::new(bw, BAR_H - 1.0));
                let resp = ui.interact(br, egui::Id::new(("winbtn", i)), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(br, 0.0, if danger { RED } else { SELECTION });
                }
                let col = if resp.hovered() { FG_BRIGHT } else { FG_DIM };
                ui.painter().text(br.center(), Align2::CENTER_CENTER, glyph, FontId::monospace(13.0), col);
                if resp.clicked() {
                    ctx.send_viewport_cmd(cmd);
                }
            }
        });
    }

    fn statusbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status").exact_height(STATUS_H).frame(egui::Frame::new().fill(BG_ALT)).show(ctx, |ui| {
            let r = ui.max_rect();
            let p = ui.painter();
            let f = FontId::monospace(12.5);
            if let Overlay::Cmd(s) = &self.overlay {
                let shown = if s.starts_with('/') { s.clone() } else { format!(":{s}") };
                let t = p.text(Pos2::new(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, shown, f.clone(), FG_BRIGHT);
                p.rect_filled(Rect::from_min_size(Pos2::new(t.right() + 1.0, r.top() + 4.0), Vec2::new(8.0, r.height() - 8.0)), 0.0, RED_CURSOR);
                return;
            }
            // the pill names what the keys drive right now (same source as the hint)
            let (mode, col) = match self.context() {
                "insert" => (" INSERT ", RED_BRIGHT),
                "tree" => (" TREE ", RED),
                "pdf" => (" FILES ", RED),
                "chat" => (" CHAT ", RED_BRIGHT),
                "review" => (" REVIEW ", RED),
                "cards" => (" CARDS ", RED),
                "graph" => (" GRAPH ", RED),
                _ => (" NOTES ", RED),
            };
            let pill = p.text(Pos2::new(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, mode, f.clone(), BG);
            p.rect_filled(pill.expand2(Vec2::new(2.0, 3.0)), 0.0, col);
            p.text(Pos2::new(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, mode, f.clone(), BG);
            let left = match (&self.msg, &self.data) {
                (Some(m), _) => m.clone(),
                (None, Some(Ok(_))) => {
                    let staged = self.changes.ops.len();
                    let hint = keys::hint(self.context());
                    if staged > 0 { format!("{staged} staged · :w commit · u undo   │   {hint}") } else { hint.to_owned() }
                }
                (None, Some(Err(_))) => "load failed".into(),
                (None, None) => "loading…".into(),
            };

            let db = match &self.data {
                Some(Ok(d)) => d.db.parent().and_then(|p| p.file_name()).map(|n| short_db(&n.to_string_lossy())).unwrap_or_default(),
                _ => String::new(),
            };
            let err = self.bridge.error.lock().ok().and_then(|e| e.clone());
            let br = if self.committing.is_some() {
                "committing".to_owned()
            } else if self.bridge.is_online() {
                "bridge online".to_owned()
            } else if let Some(e) = err {
                if e.contains("busy") { "bridge: in use by another m3m0".to_owned() } else { format!("bridge: {}", e.chars().take(40).collect::<String>()) }
            } else {
                "bridge offline".to_owned()
            };
            let right = format!("{}  {br}  ·  {db}  {:.0}%  ? keys", self.pending, ctx.zoom_factor() * 100.0);
            let rr = p.text(Pos2::new(r.right() - 10.0, r.center().y), Align2::RIGHT_CENTER, right, f.clone(), COMMENT);
            // left text never runs under the right-hand status
            let clip = Rect::from_min_max(Pos2::new(pill.right() + 14.0, r.top()), Pos2::new(rr.left() - 16.0, r.bottom()));
            // drop whole " · " items from the end until it fits, never cut a word
            let fits = |t: &str| ctx.fonts(|fo| fo.layout_no_wrap(t.to_owned(), f.clone(), FG_DIM).size().x) <= clip.width();
            let mut left = left;
            while !fits(&left) {
                match left.rfind(" · ") {
                    Some(i) => left.truncate(i),
                    None => break,
                }
            }
            p.with_clip_rect(clip).text(Pos2::new(pill.right() + 14.0, r.center().y), Align2::LEFT_CENTER, left, f.clone(), FG_DIM);

        });
    }

    fn sidebar(&mut self, ctx: &egui::Context, d: &Loaded) {
        let rows = tree_rows(d, &self.open_mods);
        self.tree_cur = self.tree_cur.min(rows.len().saturating_sub(1));
        let focused = self.focus == Focus::Tree;
        let panel = egui::SidePanel::left("tree")
            .resizable(false)
            .exact_width(self.cfg.sidebar_w)
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin { left: 6, right: 6, top: 8, bottom: 8 }))
            .show_animated(ctx, self.cfg.sidebar_open, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, (id, is_mod)) in rows.iter().enumerate() {
                        let sel = i == self.tree_cur;
                        let active = self.week.as_ref() == Some(id);
                        let (text, indent) = if *is_mod {
                            let chev = if self.open_mods.contains(id) { "\u{f078}" } else { "\u{f054}" };
                            (RichText::new(format!("{chev}  {}", d.kb.name(id))).family(bold()).size(TREE_TEXT).color(FG), 0.0)
                        } else {
                            let col = if active { RED_BRIGHT } else if sel && focused { FG } else { FG_DIM };
                            (RichText::new(d.kb.name(id)).size(TREE_TEXT - 0.5).color(col), 18.0)
                        };
                        let resp = row(ui, ("t", id), sel, focused, indent, |ui| {
                            ui.add(egui::Label::new(text).truncate().selectable(false));
                        });
                        if sel && self.scroll_tree {
                            resp.scroll_to_me(None);
                        }
                        if resp.clicked() {
                            self.tree_cur = i;
                            self.focus = Focus::Tree;
                            if *is_mod {
                                if !self.open_mods.remove(id) {
                                    self.open_mods.insert(id.clone());
                                }
                            } else {
                                self.open_week(id.clone());
                            }
                        }
                    }
                });
            });
        self.scroll_tree = false;

        // our own resize handle: the panel follows the pointer exactly, no drift
        if let Some(ir) = panel {
            let pr = ir.response.rect;
            if self.cfg.sidebar_open && (pr.width() - self.cfg.sidebar_w).abs() < 1.0 {
                let h = Rect::from_min_max(Pos2::new(pr.right() - 3.0, pr.top()), Pos2::new(pr.right() + 3.0, pr.bottom()));
                egui::Area::new(egui::Id::new("sb-handle")).order(egui::Order::Foreground).fixed_pos(h.min).show(ctx, |ui| {
                    let (rect, resp) = ui.allocate_exact_size(h.size(), Sense::drag());
                    if resp.hovered() || resp.dragged() {
                        ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
                        ui.painter().rect_filled(Rect::from_center_size(rect.center(), Vec2::new(2.0, rect.height())), 0.0, RED);
                    }
                    if resp.dragged() {
                        if let Some(x) = ctx.pointer_interact_pos() {
                            self.cfg.sidebar_w = (x.x - pr.left()).clamp(SIDEBAR_MIN, SIDEBAR_MAX);
                        }
                    }
                });
            }
        }
    }

    /// Keys while typing in the chat box: ours are esc / enter / scroll / new chat.
    fn chat_keys(&mut self, ctx: &egui::Context) -> Vec<K> {
        let mut out = vec![];
        ctx.input_mut(|i| {
            i.events.retain(|e| {
                let egui::Event::Key { key, pressed, modifiers, .. } = e else { return true };
                let k = match key {
                    egui::Key::Escape => Some(K::Esc),
                    egui::Key::Enter if !modifiers.shift => Some(K::Enter),
                    egui::Key::Tab if !modifiers.shift => Some(K::Tab),
                    egui::Key::D | egui::Key::U | egui::Key::N | egui::Key::A | egui::Key::P | egui::Key::B | egui::Key::G | egui::Key::F | egui::Key::T if modifiers.ctrl => {
                        Some(K::Ctrl(key.name().to_ascii_lowercase().chars().next().unwrap()))
                    }
                    _ => None,
                };
                match k {
                    Some(k) => {
                        if *pressed {
                            out.push(k);
                        }
                        false
                    }
                    None => true,
                }
            });
        });
        out
    }

    fn handle_chat(&mut self, ctx: &egui::Context, d: &mut Loaded, k: K) {
        match k {
            // only replayed keys (M3M0_KEYS) arrive as Ch here: type them
            K::Ch(c) => self.chat.input.push(c),
            K::Esc => self.focus = Focus::Notes,
            K::Tab => self.toggle_focus(),
            K::Enter => {
                let Some(w) = self.week.clone() else {
                    self.msg = Some("open a week first: the chat is about its module".into());
                    return;
                };
                let module = d.kb.module_of(&w).map(|m| d.kb.name(&m)).or_else(|| d.mods.contains(&w).then(|| d.kb.name(&w))).unwrap_or_default();
                if module.is_empty() {
                    self.msg = Some("this page isn't under a module".into());
                    return;
                }
                let context = page_text(&d.kb, &w);
                let rc = ctx.clone();
                self.chat.send(&module, &context, move || rc.request_repaint());
            }
            K::Ctrl('n') => {
                self.chat.reset();
                self.msg = Some("new chat".into());
            }
            K::Ctrl('d') | K::Ctrl('u') => {
                let by = if matches!(k, K::Ctrl('d')) { 300.0 } else { -300.0 };
                ctx.data_mut(|m| {
                    let v: f32 = m.get_temp(egui::Id::new("chat-scroll")).unwrap_or(0.0);
                    m.insert_temp(egui::Id::new("chat-scroll"), (v + by).max(0.0));
                });
                self.chat.scroll_end = false;
            }
            // global toggles still work from the chat box
            K::Ctrl('a') | K::Ctrl('p') | K::Ctrl('b') | K::Ctrl('g') | K::Ctrl('f') | K::Ctrl('t') => {
                self.focus = Focus::Notes;
                self.handle_loaded(ctx, d, k);
            }
            _ => {}
        }
    }

    fn chat_panel(&mut self, ctx: &egui::Context, d: &Loaded) {
        self.chat.poll();
        let focused = self.focus == Focus::Chat;
        let math = &mut self.math;
        let chat = &mut self.chat;
        let w = self.cfg.chat_w;
        let panel = egui::SidePanel::right("chat")
            .resizable(false)
            .exact_width(w)
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin { left: 10, right: 10, top: 8, bottom: 8 }))
            .show_animated(ctx, self.cfg.chat_open, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("\u{f075} claude").color(if focused { RED_BRIGHT } else { FG_DIM }).family(bold()).size(13.5));
                    let m = chat.module.clone().unwrap_or_else(|| "asks about the open module".into());
                    ui.label(RichText::new(format!("  {m}")).color(COMMENT).size(12.0));
                });
                ui.add_space(6.0);
                let input_h = 84.0;
                let avail = ui.available_height() - input_h - 10.0;
                let mut sa = egui::ScrollArea::vertical().id_salt("chat-msgs").max_height(avail).auto_shrink(false);
                if chat.scroll_end {
                    sa = sa.vertical_scroll_offset(1.0e9);
                    chat.scroll_end = false;
                } else if let Some(off) = ui.ctx().data(|m| m.get_temp::<f32>(egui::Id::new("chat-scroll"))) {
                    sa = sa.vertical_scroll_offset(off);
                    ui.ctx().data_mut(|m| m.remove::<f32>(egui::Id::new("chat-scroll")));
                }
                sa.show(ui, |ui| {
                    if chat.msgs.is_empty() {
                        ui.label(RichText::new("ask about this week: claude sees the page you're on and the module's slides, PDFs and decks.\nenter send · shift-enter newline · ctrl-n new chat · esc back to notes").color(COMMENT).size(12.5));
                    }
                    for m in &chat.msgs {
                        ui.add_space(8.0);
                        ui.label(RichText::new(if m.user { "you" } else { "claude" }).color(if m.user { RED } else { FG_DIM }).size(11.5).family(bold()));
                        if m.user {
                            ui.add(egui::Label::new(RichText::new(&m.text).color(FG_BRIGHT).size(NOTE_TEXT - 1.0)).wrap());
                        } else {
                            answer(ui, &d.kb, math, &m.text);
                        }
                    }
                });
                ui.add_space(6.0);
                let status = chat.status.clone().unwrap_or_default();
                if !status.is_empty() {
                    ui.label(RichText::new(status).color(RED_BRIGHT).size(11.5));
                }
                let id = egui::Id::new("chat-input");
                let out = egui::TextEdit::multiline(&mut chat.input)
                    .id(id)
                    .hint_text(RichText::new("ask claude…").color(COMMENT))
                    .font(FontId::monospace(NOTE_TEXT - 1.0))
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .show(ui);
                if focused && !out.response.has_focus() {
                    out.response.request_focus();
                }
                if !focused && out.response.has_focus() {
                    out.response.surrender_focus();
                }
                if out.response.clicked() {
                    // clicking the box focuses the chat
                    chat.focus_input = true;
                }
            });
        if self.chat.focus_input {
            self.chat.focus_input = false;
            self.focus = Focus::Chat;
        }
        if let Some(ir) = panel {
            let pr = ir.response.rect;
            if self.cfg.chat_open {
                let h = Rect::from_min_max(Pos2::new(pr.left() - 3.0, pr.top()), Pos2::new(pr.left() + 3.0, pr.bottom()));
                egui::Area::new(egui::Id::new("chat-handle")).order(egui::Order::Foreground).fixed_pos(h.min).show(ctx, |ui| {
                    let (rect, resp) = ui.allocate_exact_size(h.size(), Sense::drag());
                    if resp.hovered() || resp.dragged() {
                        ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
                        ui.painter().rect_filled(Rect::from_center_size(rect.center(), Vec2::new(2.0, rect.height())), 0.0, RED);
                    }
                    if resp.dragged() {
                        if let Some(x) = ctx.pointer_interact_pos() {
                            self.cfg.chat_w = (pr.right() - x.x).clamp(320.0, 1200.0);
                        }
                    }
                });
            }
        }
    }

    fn pdf_panel(&mut self, ctx: &egui::Context, d: &Loaded) {
        // follow the open week: module rem name -> uni-git folder, "Week N" -> week-NN
        if let Some(w) = &self.week {
            if let Some(m) = d.kb.q.get(w).and_then(|x| x.get("parent")).and_then(|p| p.as_str()).filter(|p| d.mods.iter().any(|m| m == p)) {
                self.pdfs.set_week(&d.kb.name(m), &d.kb.name(w));
            }
        }
        let focused = self.focus == Focus::Pdf;
        let panel = egui::SidePanel::right("pdf")
            .resizable(false)
            .exact_width(self.cfg.pdf_w)
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin { left: 8, right: 8, top: 8, bottom: 8 }))
            .show_animated(ctx, self.cfg.pdf_open, |ui| {
                // file switcher: [ / ] in the panel
                let n = self.pdfs.files.len();
                ui.horizontal(|ui| {
                    let name = self.pdfs.files.get(self.pdfs.cur).and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "no files".into());
                    let scope = if self.pdfs.cur < self.pdfs.n_week { "this week" } else { "module-wide" };
                    let ic = self.pdfs.current_kind().map(files::icon).unwrap_or("txt ");
                    ui.label(RichText::new(format!("{ic} ")).size(12.0).color(if focused { RED_BRIGHT } else { FG_DIM }));
                    ui.add(egui::Label::new(RichText::new(name).color(FG_BRIGHT).size(13.0)).truncate());
                    ui.label(RichText::new(format!("  file {}/{} · {scope}", (self.pdfs.cur + 1).min(n), n)).color(COMMENT).size(12.0));
                });
                ui.add_space(4.0);
                self.pdfs.show(ui, &d.kb, &mut self.math);
            });
        if let Some(ir) = panel {
            let pr = ir.response.rect;
            if self.cfg.pdf_open && (pr.width() - self.cfg.pdf_w).abs() < 1.0 {
                let h = Rect::from_min_max(Pos2::new(pr.left() - 3.0, pr.top()), Pos2::new(pr.left() + 3.0, pr.bottom()));
                egui::Area::new(egui::Id::new("pdf-handle")).order(egui::Order::Foreground).fixed_pos(h.min).show(ctx, |ui| {
                    let (rect, resp) = ui.allocate_exact_size(h.size(), Sense::drag());
                    if resp.hovered() || resp.dragged() {
                        ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
                        ui.painter().rect_filled(Rect::from_center_size(rect.center(), Vec2::new(2.0, rect.height())), 0.0, RED);
                    }
                    if resp.dragged() {
                        if let Some(x) = ctx.pointer_interact_pos() {
                            self.cfg.pdf_w = (pr.right() - x.x).clamp(280.0, 1400.0);
                        }
                    }
                });
            }
        }
    }

    fn notes(&mut self, ctx: &egui::Context, d: &Loaded) {
        let focused = self.focus == Focus::Notes;
        if self.review.is_some() {
            self.review_ui(ctx, d);
            return;
        }
        if self.cards.is_some() {
            self.cards_ui(ctx, d);
            return;
        }
        if self.view_graph {
            let mut open = None;
            egui::CentralPanel::default().frame(egui::Frame::new().fill(BG)).show(ctx, |ui| {
                if let Some(g) = self.graph.as_mut() {
                    open = g.show(ui);
                }
            });
            if let Some(id) = open.filter(|i| !i.starts_with("hub:")) {
                self.view_graph = false;
                self.goto(d, &id);
            }
            return;
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin { left: 24, right: 12, top: 16, bottom: 0 }))
            .show(ctx, |ui| {
                let Some(w) = self.week.clone() else {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("pick a week  ·  j/k to move, l to open, ? for keys").color(COMMENT).size(NOTE_TEXT))
                    });
                    return;
                };
                let rows = note_rows(&d.kb, &w, &self.folded);
                self.note_cur = self.note_cur.min(rows.len().saturating_sub(1));
                let mut clicked_ref: Option<String> = None;
                let mut editing = self.editing.take();
                let mut pending_cursor = self.pending_cursor.take();
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    let width = (ui.available_width() - 12.0).min(MEASURE);
                    let pad = ((ui.available_width() - width) / 2.0).max(0.0);
                    ui.horizontal_top(|ui| {
                        ui.add_space(pad);
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.label(RichText::new(d.kb.name(&w)).size(24.0).color(FG_BRIGHT).family(bold()));
                            ui.add_space(14.0);
                            for (i, (id, depth)) in rows.iter().enumerate() {
                                let sel = i == self.note_cur;
                                let kids = !d.kb.children(Some(id)).is_empty();
                                let folded = self.folded.contains(id);
                                let heading = d.kb.heading(id);
                                if heading.is_some() && i > 0 {
                                    ui.add_space(8.0);
                                }
                                let math = &mut self.math;
                                let mut refs = vec![];
                                let resp = row(ui, ("n", id), sel, focused, *depth as f32 * 22.0, |ui| {
                                    if let Some(e) = editing.as_mut().filter(|e| &e.id == id) {
                                        ui.label(RichText::new("\u{f040} ").size(NOTE_TEXT - 2.0).color(RED));
                                        editor::edit_ui(ui, e, &d.kb, &mut pending_cursor);
                                        return;
                                    }
                                    let line = d.kb.line_of(id);
                                    match heading {
                                        Some(h) => {
                                            let st = render::Style { size: HEADINGS[h.min(4) as usize], color: FG_BRIGHT, bold: true, bullet: None };
                                            refs = render::line(ui, &d.kb, math, &line, &st);
                                            if folded {
                                                ui.label(RichText::new(" …").color(RED).size(NOTE_TEXT));
                                            }
                                        }
                                        None => {
                                            let (b, c) = match (kids, folded) {
                                                (true, true) => ("\u{f054}", RED),
                                                (true, false) => ("•", RED),
                                                _ => ("•", COMMENT),
                                            };
                                            let st = render::Style { size: NOTE_TEXT, color: FG, bold: false, bullet: Some((b, c)) };
                                            refs = render::line(ui, &d.kb, math, &line, &st);
                                        }
                                    }
                                });
                                if resp.clicked() {
                                    let at = resp.interact_pointer_pos();
                                    if let Some((_, rid)) = refs.iter().find(|(r, _)| at.is_some_and(|p| r.contains(p))) {
                                        clicked_ref = Some(rid.clone());
                                    }
                                }
                                if sel && self.scroll_notes {
                                    resp.scroll_to_me(None);
                                }
                                if resp.clicked() {
                                    self.note_cur = i;
                                    self.focus = Focus::Notes;
                                }
                                if resp.double_clicked() && kids {
                                    if !self.folded.remove(id) {
                                        self.folded.insert(id.clone());
                                    }
                                }
                            }
                            ui.add_space(ui.ctx().screen_rect().height() * 0.4);
                        });
                    });
                });
                self.scroll_notes = false;
                self.editing = editing;
                self.pending_cursor = pending_cursor;
                if let Some(r) = clicked_ref {
                    self.goto(d, &r);
                }
            });
    }

    /// Open the page holding rem `id` (its week, else its parent) with the cursor on it.
    fn goto(&mut self, d: &Loaded, id: &str) {
        if let Some(w) = &self.week {
            self.jumps.push((w.clone(), self.note_cur));
        }
        let mut chain = vec![];
        let mut cur = id.to_owned();
        let mut page = None;
        while let Some(p) = d.kb.q.get(&cur).and_then(|x| x.get("parent")).and_then(|p| p.as_str()).map(str::to_owned) {
            if d.mods.contains(&p) {
                page = Some(cur.clone());
                break;
            }
            chain.push(p.clone());
            cur = p;
        }
        // not under a module week: show its parent (or itself if top-level)
        let page = page.unwrap_or_else(|| d.kb.q.get(id).and_then(|x| x.get("parent")).and_then(|p| p.as_str()).map(str::to_owned).unwrap_or(id.to_owned()));
        for c in &chain {
            self.folded.remove(c);
        }
        self.week = Some(page.clone());
        let rows = note_rows(&d.kb, &page, &self.folded);
        self.note_cur = rows.iter().position(|r| r.0 == id).unwrap_or(0);
        self.scroll_notes = true;
        self.focus = Focus::Notes;
        if let Some(m) = d.kb.module_of(&page).or_else(|| d.mods.iter().find(|m| **m == page).cloned()) {
            self.open_mods.insert(m);
        }
        self.tree_cur = tree_rows(d, &self.open_mods).iter().position(|r| r.0 == page).unwrap_or(self.tree_cur);
        self.scroll_tree = true;
    }

    fn context(&self) -> &'static str {
        if self.editing.is_some() {
            "insert"
        } else if self.focus == Focus::Tree {
            "tree"
        } else if self.focus == Focus::Pdf {
            "pdf"
        } else if self.focus == Focus::Chat {
            "chat"
        } else if self.review.is_some() {
            "review"
        } else if self.cards.is_some() {
            "cards"
        } else if self.view_graph {
            "graph"
        } else {
            "notes"
        }
    }

    fn which_key(&self, ctx: &egui::Context) {
        if self.pending.is_empty() || !matches!(self.overlay, Overlay::None) {
            return;
        }
        let items = keys::which(&self.pending, self.context());
        if items.is_empty() {
            return;
        }
        let screen = ctx.screen_rect();
        let h = 34.0 + items.len() as f32 * 20.0;
        let at = Pos2::new(screen.right() - 380.0, screen.bottom() - STATUS_H - h - 12.0);
        egui::Area::new(egui::Id::new("which")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
            egui::Frame::new().fill(BG_ALT).stroke((1.0, GREY1)).inner_margin(10.0).show(ui, |ui| {
                ui.set_width(350.0);
                ui.label(RichText::new(format!("{}…", self.pending)).color(RED_BRIGHT).family(bold()).size(13.0));
                for (k, what) in items {
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(Vec2::new(90.0, 18.0), egui::Layout::left_to_right(egui::Align::Min), |ui| {
                            ui.set_width(90.0);
                            ui.label(RichText::new(k).color(FG_BRIGHT).size(13.0));
                        });
                        ui.label(RichText::new(what).color(FG_DIM).size(13.0));
                    });
                }
            });
        });
    }

    fn overlays(&mut self, ctx: &egui::Context) {
        self.which_key(ctx);
        if let (Overlay::Find { q, items, cur, for_q }, Some(Ok(d))) = (&mut self.overlay, &self.data) {
            if for_q.as_deref() != Some(q.as_str()) {
                let here = self.week.as_ref().and_then(|w| d.kb.module_of(w));
                *items = find_items(d, q, here.as_deref());
                *for_q = Some(q.clone());
                *cur = (*cur).min(items.len().saturating_sub(1));
            }
        }
        let screen = ctx.screen_rect();
        match &self.overlay {
            Overlay::Help { scroll, tab } => {
                let (scroll, tab) = (*scroll, *tab);
                let g = &keys::HELP[tab];
                // one height for every section, so switching tabs doesn't move the box
                let most = keys::HELP.iter().map(|g| g.keys.len()).max().unwrap_or(0);
                let size = Vec2::new((screen.width() - 80.0).min(900.0), (screen.height() - 80.0).min(160.0 + most as f32 * 30.0 + 90.0).max(360.0));
                modal(ctx, "keys", screen, size, |ui| {
                    // section tabs
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 6.0);
                        for (i, s) in keys::HELP.iter().enumerate() {
                            let sel = i == tab;
                            let short = s.title.split(" (").next().unwrap_or(s.title); // "graph (ctrl-g)" -> "graph"
                            let t = RichText::new(format!(" {short} ")).size(12.5).color(if sel { BG } else { FG_DIM });
                            let t = if sel { t.background_color(RED) } else { t.background_color(GREY0) };
                            ui.label(t);
                        }
                    });
                    ui.add_space(14.0);
                    ui.label(RichText::new(g.title).color(RED_BRIGHT).family(bold()).size(17.0));
                    ui.add_space(8.0);
                    let keyw = 250.0;
                    egui::ScrollArea::vertical().max_height(size.y - 170.0).vertical_scroll_offset(scroll as f32 * 30.0).show(ui, |ui| {
                        for (k, what) in g.keys {
                            ui.horizontal_top(|ui| {
                                ui.allocate_ui_with_layout(Vec2::new(keyw, 24.0), egui::Layout::left_to_right(egui::Align::Min), |ui| {
                                    ui.set_width(keyw);
                                    ui.add(egui::Label::new(RichText::new(*k).color(FG_BRIGHT).size(15.0)).truncate());
                                });
                                ui.add(egui::Label::new(RichText::new(*what).color(FG).size(15.0)).wrap());
                            });
                            ui.add_space(6.0);
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("h/l section ({}/{}) · 1-9 jump · j/k scroll · esc close · typing a prefix (g z d > <) shows what can follow", tab + 1, keys::HELP.len())).color(COMMENT).size(12.0));
                });
            }
            Overlay::Pdfs { cur } => {
                let cur = *cur;
                let files = self.pdfs.files.clone();
                let n_week = self.pdfs.n_week;
                let open = self.pdfs.cur;
                let root = pdf::uni_dir();
                let h = (screen.height() - 80.0).min(120.0 + files.len() as f32 * 26.0 + 60.0);
                modal(ctx, "files for this week", screen, Vec2::new(820.0, h), |ui| {
                    if files.is_empty() {
                        ui.label(RichText::new("no files found: open a week (anything under a week-NN folder or named week-NN in uni-git/<module>)").color(COMMENT));
                    }
                    egui::ScrollArea::vertical().max_height(h - 110.0).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (i, f) in files.iter().enumerate() {
                            if i == 0 || i == n_week {
                                ui.add_space(if i == 0 { 0.0 } else { 10.0 });
                                ui.label(RichText::new(if i < n_week { "this week" } else { "module-wide pdfs" }).color(RED).family(bold()).size(13.0));
                                ui.add_space(3.0);
                            }
                            let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            let rel = f.parent().and_then(|p| p.strip_prefix(&root).ok()).map(|p| p.display().to_string()).unwrap_or_default();
                            // fixed columns: kind tag · name · folder
                            let r = row(ui, ("pdfpick", i), i == cur, true, 0.0, |ui| {
                                let col = |ui: &mut egui::Ui, w: f32, add: &mut dyn FnMut(&mut egui::Ui)| {
                                    ui.allocate_ui_with_layout(Vec2::new(w, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        ui.set_width(w);
                                        add(ui);
                                    });
                                };
                                col(ui, 48.0, &mut |ui| {
                                    ui.label(RichText::new(files::icon(files::kind(f))).color(if i == open { RED_BRIGHT } else { COMMENT }).size(12.0));
                                });
                                col(ui, 330.0, &mut |ui| {
                                    ui.add(egui::Label::new(RichText::new(&name).color(if i == cur { FG_BRIGHT } else { FG }).size(13.5)).truncate());
                                });
                                ui.add(egui::Label::new(RichText::new(&rel).color(COMMENT).size(11.5)).truncate());
                            });
                            if i == cur {
                                r.scroll_to_me(None);
                            }
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new("j/k move · enter open · esc close · ] / [ in the panel step through these in order").color(COMMENT).size(12.0));
                });
            }
            Overlay::Find { q, items, cur, .. } => {
                let cur = *cur;
                let q = q.clone();
                let items = items.clone();
                modal(ctx, "go to", screen, Vec2::new(760.0, 120.0 + 14.0 * 26.0), |ui| {
                    ui.horizontal(|ui| {
                        let t = ui.label(RichText::new(format!("> {q}")).color(FG_BRIGHT).size(15.0));
                        ui.painter().rect_filled(Rect::from_min_size(t.rect.right_top() + Vec2::new(2.0, 2.0), Vec2::new(8.0, t.rect.height() - 4.0)), 0.0, RED_CURSOR);
                    });
                    ui.add_space(6.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, (_, label)) in items.iter().enumerate() {
                        row(ui, ("find", i), i == cur, true, 0.0, |ui| {
                            ui.add(egui::Label::new(RichText::new(label).size(14.0).color(if i == cur { FG_BRIGHT } else { FG_DIM })).truncate());
                        });
                    }
                    if items.is_empty() {
                        ui.label(RichText::new(if q.is_empty() { "type a week, module or rem name" } else { "no matches" }).color(COMMENT));
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new("type to filter · ctrl-n/p or tab move · enter go · esc close").color(COMMENT).size(12.0));
                });
            }
            Overlay::Link(lp) => {
                // closest matches first; show a window of rows around the cursor
                const ROWS: usize = 14;
                let start = lp.cur.saturating_sub(ROWS / 2).min(lp.items.len().saturating_sub(ROWS));
                modal(ctx, "link to claude's card", screen, Vec2::new(820.0, 150.0 + ROWS as f32 * 26.0), |ui| {
                    ui.add(egui::Label::new(RichText::new(&lp.title).color(FG_BRIGHT).size(14.0)).truncate());
                    ui.add_space(8.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, ((_, score), label)) in lp.items.iter().zip(&lp.labels).enumerate().skip(start).take(ROWS) {
                        row(ui, ("link", i), i == lp.cur, true, 0.0, |ui| {
                            ui.label(RichText::new(format!("{:>3}%", (score * 100.0).round() as i32)).color(COMMENT).size(12.5));
                            ui.add(egui::Label::new(RichText::new(label).size(14.0).color(if i == lp.cur { FG_BRIGHT } else { FG_DIM })).truncate());
                        });
                    }
                    if lp.items.is_empty() {
                        ui.label(RichText::new("claude's deck for this week is empty").color(COMMENT));
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new("closest first · j/k or ctrl-n/p move · enter link · esc close").color(COMMENT).size(12.0));
                });
            }
            Overlay::Db { list, cur } => {
                let cur = *cur;
                let current = self.data.as_ref().and_then(|d| d.as_ref().ok()).map(|d| d.db.clone());
                modal(ctx, "databases", screen, Vec2::new(640.0, 120.0 + list.len() as f32 * 46.0), |ui| {
                    ui.label(RichText::new("RemNote databases under ~/remnote, largest first").color(FG_DIM).size(13.0));
                    ui.add_space(8.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, c) in list.iter().enumerate() {
                        let name = c.path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let age = c
                            .modified
                            .and_then(|m| m.elapsed().ok())
                            .map(|e| match e.as_secs() {
                                s if s < 3600 => format!("{}m ago", s / 60),
                                s if s < 86400 => format!("{}h ago", s / 3600),
                                s => format!("{}d ago", s / 86400),
                            })
                            .unwrap_or_default();
                        let using = current.as_ref() == Some(&c.path);
                        row(ui, ("db", i), i == cur, true, 0.0, |ui| {
                            ui.vertical(|ui| {
                                ui.label(RichText::new(format!("{name}{}", if using { "   (open)" } else { "" })).color(if using { RED_BRIGHT } else { FG }).size(14.0));
                                ui.label(RichText::new(format!("{:.1} MB · modified {age}", c.bytes as f64 / 1e6)).color(COMMENT).size(12.0));
                            });
                        });
                    }
                    ui.add_space(10.0);
                    ui.label(RichText::new("j/k move · enter open (saved) · esc cancel").color(COMMENT).size(12.0));
                });
            }
            Overlay::Diff { scroll } => {
                let Some(Ok(d)) = &self.data else { return };
                let name = |id: &str| if id.starts_with("tmp-") { "(new)".to_owned() } else { d.kb.name(id) };
                let lines: Vec<(char, String)> = self.changes.ops.iter().map(|o| commit::describe(o, &name)).collect();
                let scroll = (*scroll).min(lines.len().saturating_sub(1));
                let h = (screen.height() - 120.0).min(140.0 + lines.len() as f32 * 26.0);
                modal(ctx, &format!("staged · {} changes", lines.len()), screen, Vec2::new(900.0, h), |ui| {
                    egui::ScrollArea::vertical().max_height(h - 110.0).show(ui, |ui| {
                        for (i, (c, t)) in lines.iter().enumerate().skip(scroll) {
                            let col = match c {
                                '+' => FG_BRIGHT,
                                '-' => RED_BRIGHT,
                                '~' => FG,
                                _ => FG_DIM,
                            };
                            ui.horizontal_top(|ui| {
                                ui.label(RichText::new(format!("{c} ")).color(if *c == '-' { RED } else { COMMENT }).size(13.5));
                                ui.add(egui::Label::new(RichText::new(t).color(col).size(13.5)).wrap());
                            });
                            let _ = i;
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new("enter commit to RemNote · j/k scroll · esc back · u in the outline undoes").color(COMMENT).size(12.0));
                });
            }
            Overlay::Log { entries, cur } => {
                let cur = *cur;
                let Some(Ok(d)) = &self.data else { return };
                let name = |id: &str| d.kb.name(id);
                modal(ctx, "commit log", screen, Vec2::new(900.0, (screen.height() - 120.0).min(600.0)), |ui| {
                    if entries.is_empty() {
                        ui.label(RichText::new("no commits yet").color(COMMENT));
                    }
                    egui::ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (i, e) in entries.iter().enumerate() {
                            let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(e.time);
                            let ago = when.elapsed().map(|x| x.as_secs()).unwrap_or(0);
                            let ago = match ago {
                                s if s < 3600 => format!("{}m ago", s / 60),
                                s if s < 86400 => format!("{}h ago", s / 3600),
                                s => format!("{}d ago", s / 86400),
                            };
                            let first = e.ops.first().map(|o| commit::describe(o, &name).1).unwrap_or_default();
                            row(ui, ("log", i), i == cur, true, 0.0, |ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(format!("{ago} · {} changes", e.ops.len())).color(FG).size(13.5));
                                    ui.add(egui::Label::new(RichText::new(first).color(COMMENT).size(12.0)).truncate());
                                });
                            });
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new("enter stage a revert of this commit · j/k move · esc close").color(COMMENT).size(12.0));
                });
            }
            _ => {}
        }
    }
}

/// A full-width selectable row. The cursor row gets a highlight and a red left edge
/// (dim when its pane isn't focused).
fn row(ui: &mut egui::Ui, id: impl std::hash::Hash, sel: bool, focused: bool, indent: f32, add: impl FnOnce(&mut egui::Ui)) -> egui::Response {
    let fill = match (sel, focused) {
        (true, true) => SELECTION,
        (true, false) => BG_ALT,
        _ => Color32::TRANSPARENT,
    };
    let inner = egui::Frame::new().fill(fill).inner_margin(egui::Margin { left: 8, right: 6, top: 3, bottom: 3 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_top(|ui| {
            ui.add_space(indent);
            add(ui);
        });
    });
    let r = inner.response.rect;
    if sel {
        let edge = Rect::from_min_size(r.min, Vec2::new(2.0, r.height()));
        ui.painter().rect_filled(edge, 0.0, if focused { RED_CURSOR } else { GREY1 });
    }
    ui.interact(r, egui::Id::new(id), Sense::click())
}

fn modal(ctx: &egui::Context, title: &str, screen: Rect, size: Vec2, add: impl FnOnce(&mut egui::Ui)) {
    ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("dim"))).rect_filled(screen, 0.0, Color32::from_black_alpha(170));
    let size = size.min(screen.size() - Vec2::splat(40.0));
    egui::Area::new(egui::Id::new(("modal", title)))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.center() - size / 2.0)
        .show(ctx, |ui| {
            egui::Frame::new().fill(BG_ALT).stroke((1.0, GREY1)).inner_margin(18.0).show(ui, |ui| {
                ui.set_min_size(size - Vec2::splat(36.0));
                ui.set_max_width(size.x - 36.0);
                ui.label(RichText::new(title).color(FG_BRIGHT).family(bold()).size(16.0));
                ui.add_space(10.0);
                add(ui);
            });
        });
}

/// Undecorated windows need their own edge resizing (when not maximised).
fn edge_resize(ctx: &egui::Context) {
    if ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)) {
        return;
    }
    let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    let r = ctx.screen_rect();
    let m = 5.0;
    let (l, rt, t, b) = (p.x < r.left() + m, p.x > r.right() - m, p.y < r.top() + m, p.y > r.bottom() - m);
    use CursorIcon as C;
    use ResizeDirection as D;
    let (dir, icon) = match (l, rt, t, b) {
        (true, _, true, _) => (D::NorthWest, C::ResizeNorthWest),
        (_, true, true, _) => (D::NorthEast, C::ResizeNorthEast),
        (true, _, _, true) => (D::SouthWest, C::ResizeSouthWest),
        (_, true, _, true) => (D::SouthEast, C::ResizeSouthEast),
        (true, ..) => (D::West, C::ResizeWest),
        (_, true, ..) => (D::East, C::ResizeEast),
        (_, _, true, _) => (D::North, C::ResizeNorth),
        (.., true) => (D::South, C::ResizeSouth),
        _ => return,
    };
    ctx.set_cursor_icon(icon);
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
    }
}

/// M3M0_SHOTS=DIR saves frames at fixed times, then quits (for previews).
/// M3M0_KEYS="jjl..." replays keys after boot (so previews can show navigation).
fn shots(ctx: &egui::Context) {
    let Ok(dir) = std::env::var("M3M0_SHOTS") else { return };
    let last: f64 = std::env::var("M3M0_LAST").ok().and_then(|v| v.parse().ok()).unwrap_or(5.5);
    #[allow(non_snake_case)]
    let AT: [f64; 10] = [0.25, 0.7, 1.1, 1.45, 1.6, 1.85, 2.3, 2.9, 3.25, last];
    let t = ctx.input(|i| i.time);
    let id = egui::Id::new("shot-n");
    let n: usize = ctx.data(|d| d.get_temp(id)).unwrap_or(0);
    for e in ctx.input(|i| i.raw.events.clone()) {
        if let egui::Event::Screenshot { image, .. } = e {
            let k: usize = ctx.data(|d| d.get_temp(egui::Id::new("shot-saved"))).unwrap_or(0);
            let px: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            let _ = image::save_buffer(format!("{dir}/{k:02}.png"), &px, image.width() as u32, image.height() as u32, image::ColorType::Rgba8);
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("shot-saved"), k + 1));
            if k + 1 == AT.len() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
    if n < AT.len() && t >= AT[n] {
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        ctx.data_mut(|d| d.insert_temp(id, n + 1));
    }
    ctx.request_repaint();
}

/// `m3m0 roundtrip`: every rem -> editable text -> back, compared with the DB.
/// Editing a line must never change what we didn't touch.
fn roundtrip() -> i32 {
    let Some(db) = kb::resolve_db(Config::load().db.as_deref()) else { return 2 };
    let kb = match Kb::load(&db) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let name = |id: &str| kb.name(id);
    let (mut ok, mut bad) = (0, 0);
    for id in kb.q.keys() {
        let line = kb.line_of(id);
        let mut side = rich::Side::default();
        let text = rich::line_to_text(&line, &name, &mut side);
        let back = rich::text_to_line(&text, &|n| kb.resolve(n, Some(id)), &side, &mut || "NEW".into());
        let same = back.kind == line.kind && rich::to_rt(&back.front) == rich::to_rt(&line.front) && rich::to_rt(&back.back) == rich::to_rt(&line.back);
        if same {
            ok += 1;
        } else {
            bad += 1;
            if bad <= 15 {
                let cut = |v: serde_json::Value| { let s = v.to_string(); s.chars().take(240).collect::<String>() };
                println!("--- {id} {:?} -> {:?}\n text  {}\n front {} -> {}\n back  {} -> {}", line.kind, back.kind, text.chars().take(200).collect::<String>(), cut(rich::to_rt(&line.front)), cut(rich::to_rt(&back.front)), cut(rich::to_rt(&line.back)), cut(rich::to_rt(&back.back)));
            }
        }
    }
    println!("{ok} ok, {bad} differ");
    (bad > 0) as i32
}

/// After a commit: temporary ids -> the ids RemNote assigned, everywhere in the Kb.
fn rename_ids(kb: &mut Kb, m: &std::collections::HashMap<String, String>) {
    if m.is_empty() {
        return;
    }
    let touched: Vec<String> = kb.q.iter().filter(|(k, v)| k.starts_with("tmp-") || v.to_string().contains("\"tmp-")).map(|(k, _)| k.clone()).collect();
    for k in touched {
        if let Some(v) = kb.q.remove(&k) {
            let nk = m.get(&k).cloned().unwrap_or(k);
            let mut v = commit::map_ids(&v, m);
            v["_id"] = serde_json::json!(nk);
            kb.q.insert(nk, v);
        }
    }
    let kids = std::mem::take(&mut kb.kids);
    kb.kids = kids
        .into_iter()
        .map(|(p, v)| (p.map(|p| m.get(&p).cloned().unwrap_or(p)), v.into_iter().map(|c| m.get(&c).cloned().unwrap_or(c)).collect()))
        .collect();
}

/// `m3m0 commit-test`: run staged-style ops through commit::run against RemNote
/// on a dated scratch tree, read them back, then delete the tree (unless --keep).
fn commit_test(keep: bool) -> i32 {
    use edit::{LineJson, Op};
    use serde_json::json;
    let b = bridge::Bridge::start(|| {});
    println!("waiting for the RemNote plugin (up to 90 s)…");
    if !b.wait_online(90) {
        eprintln!("plugin didn't connect");
        return 1;
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let root = match b.call("create", json!({"parent": null, "text": [format!("m3m0 commit test {stamp}")]})) {
        Ok(r) => r["id"].as_str().unwrap_or_default().to_owned(),
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let side = rich::Side::default();
    let lj = |t: &str| LineJson::of(&rich::text_to_line(t, &|_| None, &side, &mut || "1795".into()));
    let ops = vec![
        Op::Create { id: "tmp-1".into(), parent: root.clone(), pos: 0, line: lj("plain A") },
        Op::Create { id: "tmp-2".into(), parent: root.clone(), pos: 1, line: lj("B :: def with $x^2$ and `code`") },
        Op::Update { id: "tmp-1".into(), before: lj("plain A"), after: lj("A >> back of A") },
        Op::Create { id: "tmp-3".into(), parent: "tmp-1".into(), pos: 0, line: lj("C child of A") },
        Op::Move { id: "tmp-3".into(), from: ("tmp-1".into(), 0), to: (root.clone(), 0) },
        Op::Create { id: "tmp-4".into(), parent: root.clone(), pos: 3, line: lj("D list") },
        Op::Create { id: "tmp-5".into(), parent: "tmp-4".into(), pos: 0, line: lj("item one") },
        Op::Create { id: "tmp-6".into(), parent: "tmp-4".into(), pos: 1, line: lj("item two") },
        Op::Update { id: "tmp-4".into(), before: lj("D list"), after: lj("D list >>>") },
        Op::Create { id: "tmp-7".into(), parent: root.clone(), pos: 4, line: lj("## a heading") },
        Op::Delete { id: "tmp-2".into(), parent: root.clone(), pos: 2, docs: vec![] },
    ];
    let mut children = std::collections::HashMap::new();
    children.insert("tmp-4".to_string(), vec!["tmp-5".to_string(), "tmp-6".to_string()]);
    let out = commit::run(&b, &ops, &children);
    println!("done {}/{}  err {:?}", out.done, ops.len(), out.err);
    for t in ["tmp-1", "tmp-3", "tmp-4", "tmp-5", "tmp-7"] {
        if let Some(id) = out.idmap.get(t) {
            match b.call("get", json!({"rem": id})) {
                Ok(g) => println!("  {t} {id}: type={} parent={} text={} back={} cards={}", g["type"], g["parent"], g["text"], g["back"], g["cards"].as_array().map_or(0, |c| c.len())),
                Err(e) => println!("  {t}: {e}"),
            }
        }
    }
    if let Some(id) = out.idmap.get("tmp-2") {
        println!("  tmp-2 (deleted) get -> {:?}", b.call("get", json!({"rem": id})).err());
    }
    // order under root
    if keep {
        println!("kept scratch tree {root}");
    } else {
        let _ = b.call("delete", json!({"rem": root}));
        println!("deleted scratch tree");
    }
    (out.err.is_some()) as i32
}

/// ctrl-t candidates: modules and weeks first, then any rem, by name.
/// ctrl-t results: modules, weeks, then any rem; best match first, ties go to the
/// module you're in (`here`), then shorter names.
fn find_items(d: &Loaded, q: &str, here: Option<&str>) -> Vec<(String, String)> {
    let ql = q.to_lowercase();
    let mut out: Vec<(u8, bool, usize, String, String)> = vec![];
    let rank = |name: &str| -> Option<u8> {
        let n = name.to_lowercase();
        if ql.is_empty() || n.starts_with(&ql) {
            Some(0)
        } else if n.contains(&ql) {
            Some(1)
        } else {
            // every word of the query somewhere in the name
            ql.split_whitespace().all(|w| n.contains(w)).then_some(2)
        }
    };
    let mut seen = HashSet::new();
    for m in &d.mods {
        let mn = d.kb.name(m);
        if let Some(r) = rank(&mn) {
            out.push((r, Some(m.as_str()) != here, mn.len(), m.clone(), format!("{mn}  (module)")));
        }
        for w in d.kb.children(Some(m)) {
            let wn = d.kb.name(w);
            if !wn.starts_with("Week") {
                continue;
            }
            let full = format!("{mn} / {wn}");
            if let Some(r) = rank(&full).or_else(|| rank(&wn)) {
                out.push((r, Some(m.as_str()) != here, full.len(), w.clone(), full));
                seen.insert(w.clone());
            }
        }
    }
    if !ql.is_empty() {
        for id in d.kb.q.keys() {
            if seen.contains(id) || d.mods.contains(id) {
                continue;
            }
            let n = rich::readable_str(&d.kb.name(id)).replace('\n', " ");
            if n.is_empty() || n.len() > 160 {
                continue;
            }
            if let Some(r) = rank(&n) {
                let m = d.kb.module_of(id);
                let where_ = m.as_ref().map(|m| d.kb.name(m)).unwrap_or_default();
                out.push((r + 1, m.as_deref() != here, n.len(), id.clone(), format!("{n}   · {where_}")));
            }
        }
    }
    out.sort();
    out.into_iter().take(14).map(|x| (x.3, x.4)).collect()
}

/// The open page as indented deck-syntax text (chat context).
fn page_text(kb: &Kb, page: &str) -> String {
    let mut out = format!("# {}\n", kb.name(page));
    for (id, depth) in note_rows(kb, page, &HashSet::new()) {
        let mut side = rich::Side::default();
        let t = rich::line_to_text(&kb.line_of(&id), &|r| kb.name(r), &mut side);
        out.push_str(&format!("{}- {}\n", "  ".repeat(depth), t));
    }
    out
}

/// Draw a chat answer: paragraphs with inline LaTeX, ``` code blocks.
fn answer(ui: &mut egui::Ui, kb: &Kb, math: &mut math::Math, text: &str) {
    let mut code: Option<String> = None;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            match code.take() {
                Some(c) => {
                    egui::Frame::new().fill(BG_ALT).stroke((1.0, GREY0)).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(RichText::new(c.trim_end()).monospace().size(NOTE_TEXT - 2.5).color(FG)).wrap());
                    });
                }
                None => code = Some(String::new()),
            }
            continue;
        }
        if let Some(c) = &mut code {
            c.push_str(line);
            c.push('\n');
            continue;
        }
        if line.trim().is_empty() {
            ui.add_space(6.0);
            continue;
        }
        let (bullet, body) = match line.trim_start().strip_prefix("- ").or_else(|| line.trim_start().strip_prefix("* ")) {
            Some(b) => (Some(("•", RED)), b),
            None => (None, line),
        };
        let (size, body, bold) = match body.strip_prefix("### ").or_else(|| body.strip_prefix("## ")).or_else(|| body.strip_prefix("# ")) {
            Some(h) => (NOTE_TEXT, h, true),
            None => (NOTE_TEXT - 1.0, body, false),
        };
        let spans = rich::inline(body, &|_| None, &rich::Side::default(), &mut || "c".into());
        let l = rich::Line { kind: rich::Kind::Plain, front: spans, back: vec![] };
        ui.horizontal_top(|ui| {
            render::line(ui, kb, math, &l, &render::Style { size, color: FG, bold, bullet });
        });
    }
    if let Some(c) = code {
        ui.add(egui::Label::new(RichText::new(c).monospace().size(NOTE_TEXT - 2.5).color(FG)).wrap());
    }
}

/// "1 change", "3 changes"
fn plural(n: usize, word: &str) -> String {
    if n == 1 { word.to_owned() } else { format!("{word}s") }
}

/// "remnote-68dc5b9f7e880c94ac6d230c" -> "remnote-68dc5b9f": the folder name, minus the long id tail
fn short_db(name: &str) -> String {
    match name.split_once('-') {
        Some((a, id)) if id.len() > 12 && id.chars().all(|c| c.is_ascii_hexdigit()) => format!("{a}-{}", &id[..8]),
        _ => name.to_owned(),
    }
}
