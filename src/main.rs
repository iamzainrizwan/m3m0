mod boot;
mod bridge;
mod config;
mod icon;
mod kb;
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
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("m3m0")
            .with_app_id("m3m0")
            .with_decorations(false) // our own rice titlebar instead of the toolkit's
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([520.0, 360.0])
            .with_maximized(true)
            .with_icon(std::sync::Arc::new(egui::IconData { rgba: icon::rgba(256), width: 256, height: 256 })),
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
}

enum Overlay {
    None,
    Help,
    Db { list: Vec<DbCandidate>, cur: usize },
    Cmd(String),
}

#[derive(Clone, Copy, Debug)]
enum K {
    Ch(char),
    Ctrl(char),
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
            if kb.line(k).trim().is_empty() && kids.is_empty() {
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
        let cfg = Config::load();
        if let Some(z) = cfg.zoom {
            cc.egui_ctx.set_zoom_factor(z);
        }
        let rx = Self::spawn_load(&cc.egui_ctx, kb::resolve_db(cfg.db.as_deref()));
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
                    egui::Event::Text(t) => out.extend(t.chars().map(K::Ch)),
                    egui::Event::Key { key, pressed: true, modifiers, .. } => {
                        if modifiers.ctrl && !modifiers.shift {
                            let n = key.name();
                            if n.len() == 1 {
                                out.push(K::Ctrl(n.to_ascii_lowercase().chars().next().unwrap()));
                            }
                            continue;
                        }
                        out.push(match key {
                            egui::Key::Enter => K::Enter,
                            egui::Key::Escape => K::Esc,
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
        let taken = self.data.take();
        self.reloaded = false;
        if let Some(Ok(d)) = &taken {
            self.handle_loaded(ctx, d, k);
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

    fn handle_loaded(&mut self, ctx: &egui::Context, d: &Loaded, k: K) {
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

        match (prefix.as_str(), k) {
            ("g", K::Ch('g')) => self.jump(0, trows.len(), nrows.len()),
            ("z", K::Ch(c)) => self.fold(d, &nrows, c),
            (_, K::Ch('g')) => self.pending = format!("{digits}g"),
            (_, K::Ch('z')) => self.pending.push('z'),
            (_, K::Ch(':')) => self.overlay = Overlay::Cmd(String::new()),
            (_, K::Ch('?')) => self.overlay = Overlay::Help,
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
            Focus::Notes if self.cfg.sidebar_open => Focus::Tree,
            f => f,
        };
    }

    fn mv(&mut self, by: isize, tn: usize, nn: usize) {
        let (cur, len, flag) = match self.focus {
            Focus::Tree => (&mut self.tree_cur, tn, &mut self.scroll_tree),
            Focus::Notes => (&mut self.note_cur, nn, &mut self.scroll_notes),
        };
        if len > 0 {
            *cur = (*cur as isize + by).clamp(0, len as isize - 1) as usize;
            *flag = true;
        }
    }

    fn jump(&mut self, to: usize, tn: usize, nn: usize) {
        match self.focus {
            Focus::Tree => self.tree_cur = to.min(tn.saturating_sub(1)),
            Focus::Notes => self.note_cur = to.min(nn.saturating_sub(1)),
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
            Overlay::Help => {
                if matches!(k, K::Esc | K::Ch('q') | K::Ch('?') | K::Enter) {
                    self.overlay = Overlay::None;
                }
            }
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
            Overlay::None => {}
        }
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
        let mut it = cmd.split_whitespace();
        match (it.next(), it.next()) {
            (Some("q" | "quit" | "q!" | "wq"), _) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
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
            (Some("reload" | "e"), _) => {
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
            (Some("w"), _) => self.msg = Some("nothing staged (commits arrive in phase 3)".into()),
            (Some("help" | "h"), _) => self.overlay = Overlay::Help,
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
            let mut ks = self.keys(ctx);
            if std::env::var("M3M0_SHOTS").is_ok() {
                ks.clear(); // previews: only replayed keys
            }
            // M3M0_KEYS replays keys once after boot, for previews ("~b" = ctrl-b, "\n" = enter)
            if let (Ok(s), false) = (std::env::var("M3M0_KEYS"), self.replayed) {
                if self.data.as_ref().is_some_and(|d| d.is_ok()) {
                    self.replayed = true;
                    let mut it = s.chars();
                    while let Some(c) = it.next() {
                        ks.push(match c {
                            '~' => K::Ctrl(it.next().unwrap_or(' ')),
                            '\n' => K::Enter,
                            c => K::Ch(c),
                        });
                    }
                }
            }
            for k in ks {
                self.handle(ctx, k);
            }
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
        self.notes(ctx, data);
        self.boot.show(ctx, Some(&data.log), BAR_LOGO);
        self.data = taken;
        self.overlays(ctx);

        self.cfg.zoom = Some(ctx.zoom_factor());
        if self.cfg != self.saved && !ctx.input(|i| i.pointer.any_down()) {
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
                let t = p.text(Pos2::new(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, format!(":{s}"), f.clone(), FG_BRIGHT);
                p.rect_filled(Rect::from_min_size(Pos2::new(t.right() + 1.0, r.top() + 4.0), Vec2::new(8.0, r.height() - 8.0)), 0.0, RED_CURSOR);
                return;
            }
            let (mode, col) = match self.focus {
                Focus::Tree => (" TREE ", RED),
                Focus::Notes => (" NOTES ", RED),
            };
            let pill = p.text(Pos2::new(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, mode, f.clone(), BG);
            p.rect_filled(pill.expand2(Vec2::new(2.0, 3.0)), 0.0, col);
            p.text(Pos2::new(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, mode, f.clone(), BG);
            let left = match (&self.msg, &self.data) {
                (Some(m), _) => m.clone(),
                (None, Some(Ok(d))) => format!("{} rems · {} cards · bridge offline · read-only", d.kb.q.len(), d.kb.cards),
                (None, Some(Err(_))) => "load failed".into(),
                (None, None) => "loading…".into(),
            };
            p.text(Pos2::new(pill.right() + 14.0, r.center().y), Align2::LEFT_CENTER, left, f.clone(), FG_DIM);
            let db = match &self.data {
                Some(Ok(d)) => d.db.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                _ => String::new(),
            };
            let right = format!("{}  {db}  {:.0}%  ? keys", self.pending, ctx.zoom_factor() * 100.0);
            p.text(Pos2::new(r.right() - 10.0, r.center().y), Align2::RIGHT_CENTER, right, f, COMMENT);
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

    fn notes(&mut self, ctx: &egui::Context, d: &Loaded) {
        let focused = self.focus == Focus::Notes;
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
                                let resp = row(ui, ("n", id), sel, focused, *depth as f32 * 22.0, |ui| {
                                    let line = d.kb.line(id);
                                    match heading {
                                        Some(h) => {
                                            let t = RichText::new(line).size(HEADINGS[h.min(4) as usize]).color(FG_BRIGHT).family(bold());
                                            ui.add(egui::Label::new(t).wrap().selectable(false));
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
                                            ui.add(egui::Label::new(RichText::new(b).color(c).size(NOTE_TEXT - 2.0)).selectable(false));
                                            ui.add_space(4.0);
                                            ui.add(egui::Label::new(RichText::new(line).size(NOTE_TEXT)).wrap().selectable(false));
                                        }
                                    }
                                });
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
            });
    }

    fn overlays(&mut self, ctx: &egui::Context) {
        let screen = ctx.screen_rect();
        match &self.overlay {
            Overlay::Help => {
                let groups: &[(&str, &[(&str, &str)])] = &[
                    ("move", &[("j / k", "down / up (5j works)"), ("gg / G", "top / bottom"), ("ctrl-d / ctrl-u", "half page"), ("h / l", "out / in (tree: collapse / open)"), ("ctrl-j / ctrl-k", "next / previous week")]),
                    ("panes", &[("tab", "switch tree / notes"), ("ctrl-h / ctrl-l", "focus tree / notes"), ("ctrl-b", "toggle sidebar"), ("< / >", "narrow / widen sidebar (tree)")]),
                    ("fold", &[("za / enter", "toggle"), ("zc / zo", "close / open"), ("zM / zR", "fold all / unfold all")]),
                    ("app", &[(":", "command  (:q :db :reload :zoom 130)"), ("ctrl +/- / ctrl 0", "zoom (saved)"), ("?", "this help")]),
                ];
                modal(ctx, "keys", screen, Vec2::new(560.0, 480.0), |ui| {
                    for (g, ks) in groups {
                        ui.label(RichText::new(*g).color(RED).family(bold()).size(13.0));
                        ui.add_space(2.0);
                        for (k, what) in *ks {
                            ui.horizontal(|ui| {
                                ui.add_sized([170.0, 18.0], egui::Label::new(RichText::new(*k).color(FG_BRIGHT).size(13.5)));
                                ui.label(RichText::new(*what).color(FG_DIM).size(13.5));
                            });
                        }
                        ui.add_space(10.0);
                    }
                    ui.label(RichText::new("esc to close").color(COMMENT).size(12.0));
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
    const AT: [f64; 10] = [0.25, 0.7, 1.1, 1.45, 1.6, 1.85, 2.3, 2.9, 3.25, 5.5];
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
