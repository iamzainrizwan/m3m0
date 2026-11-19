//! Vim-style editing of the note outline: insert mode on the cursor bullet
//! (raw deck syntax, rich.rs), structure ops, undo, and [[ autocomplete.
//! Every change goes through edit::Changes, so it's staged for `:w`.

use super::*;
use crate::edit::{LineJson, Op};
use egui::text::{CCursor, CCursorRange};

pub struct Editing {
    pub id: String,
    pub buf: String,
    pub side: rich::Side,
    pub orig: rich::Line,
    /// cursor goes to the end (a/A/o) or start (i/I) on the first frame
    pub at_end: bool,
    pub fresh: bool,
    /// made by o/O/enter: dropped again if left empty
    pub created: bool,
    pub ac: Option<Ac>,
}

/// [[ autocomplete: matches for the text typed after the last unclosed `[[`.
pub struct Ac {
    pub start: usize, // char index of the `[[`
    pub end: usize,   // char index of the cursor
    pub query: String,
    pub items: Vec<(String, String)>, // (id, name as typed into the token)
    pub cur: usize,
}

const EDIT_ID: &str = "m3m0-edit";

fn cloze_id() -> String {
    // RemNote cloze ids are numeric strings
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    format!("{}{}", t, N.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

impl App {
    /// While editing, only these keys are ours; everything else goes to the text box.
    pub(super) fn insert_keys(&mut self, ctx: &egui::Context) -> Vec<K> {
        let ac_open = self.editing.as_ref().is_some_and(|e| e.ac.as_ref().is_some_and(|a| !a.items.is_empty()));
        let empty = self.editing.as_ref().is_some_and(|e| e.buf.is_empty());
        let mut out = vec![];
        ctx.input_mut(|i| {
            i.events.retain(|e| {
                let egui::Event::Key { key, pressed, modifiers, .. } = e else { return true };
                use egui::Key as E;
                let k = match key {
                    E::Escape => Some(K::Esc),
                    E::Enter if !modifiers.shift => Some(K::Enter),
                    E::Tab if modifiers.shift => Some(K::ShiftTab),
                    E::Tab => Some(K::Tab),
                    E::Backspace if empty => Some(K::Back),
                    E::ArrowDown | E::ArrowUp if ac_open => Some(if *key == E::ArrowDown { K::Down } else { K::Up }),
                    E::N | E::P if ac_open && modifiers.ctrl => Some(if *key == E::N { K::Down } else { K::Up }),
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

    pub(super) fn start_edit(&mut self, d: &Loaded, id: &str, at_end: bool, created: bool) {
        let orig = d.kb.line_of(id);
        let mut side = rich::Side::default();
        let buf = rich::line_to_text(&orig, &|r| d.kb.name(r), &mut side);
        self.editing = Some(Editing { id: id.to_owned(), buf, side, orig, at_end, fresh: true, created, ac: None });
        self.focus = Focus::Notes;
    }

    /// Parse the buffer back and stage an Update if anything changed.
    /// Returns the rem id, or None if it was a blank new rem that got dropped.
    pub(super) fn finish_edit(&mut self, d: &mut Loaded) -> Option<String> {
        let e = self.editing.take()?;
        if e.created && e.buf.trim().is_empty() {
            self.changes.undo(&mut d.kb); // drop the Create
            self.changes.redo.clear();
            return None;
        }
        let near = e.id.clone();
        let line = rich::text_to_line(&e.buf, &|n| d.kb.resolve(n, Some(&near)), &e.side, &mut cloze_id);
        let (before, after) = (LineJson::of(&e.orig), LineJson::of(&line));
        // a bullet just made with o/O/enter: fold the text into its Create
        if e.created {
            if let Some(Op::Create { id, line: l, .. }) = self.changes.ops.last_mut() {
                if *id == e.id {
                    *l = after;
                    crate::edit::set_line(&mut d.kb, &e.id, &line);
                    return Some(e.id);
                }
            }
        }
        if before != after {
            self.changes.apply(&mut d.kb, Op::Update { id: e.id.clone(), before, after });
        }
        Some(e.id)
    }

    fn parent_pos(d: &Loaded, id: &str) -> Option<(String, usize)> {
        let p = d.kb.q.get(id)?.get("parent")?.as_str()?.to_owned();
        let pos = d.kb.children(Some(&p)).iter().position(|k| k == id)?;
        Some((p, pos))
    }

    fn cursor_to(&mut self, d: &Loaded, id: &str) {
        if let Some(page) = &self.week {
            // unfold ancestors so the rem is visible
            let mut cur = id.to_owned();
            while let Some((p, _)) = Self::parent_pos(d, &cur) {
                if &p == page {
                    break;
                }
                self.folded.remove(&p);
                cur = p;
            }
            let rows = note_rows(&d.kb, page, &self.folded);
            if let Some(i) = rows.iter().position(|r| r.0 == id) {
                self.note_cur = i;
                self.scroll_notes = true;
            }
        }
    }

    /// New empty rem after `after` (as its first child if it has visible children),
    /// or before it with `above`; at the top of the page if there's no cursor row.
    fn new_rem(&mut self, d: &mut Loaded, after: Option<&str>, above: bool) -> Option<String> {
        let page = self.week.clone()?;
        let (parent, pos) = match after {
            None => (page, 0),
            Some(a) => {
                let has_open_kids = !d.kb.children(Some(a)).is_empty() && !self.folded.contains(a);
                if !above && has_open_kids {
                    (a.to_owned(), 0)
                } else {
                    let (p, i) = Self::parent_pos(d, a)?;
                    (p, if above { i } else { i + 1 })
                }
            }
        };
        let id = self.changes.tmp_id();
        let line = rich::Line { kind: rich::Kind::Plain, front: vec![], back: vec![] };
        self.changes.apply(&mut d.kb, Op::Create { id: id.clone(), parent, pos, line: LineJson::of(&line) });
        self.cursor_to(d, &id);
        Some(id)
    }

    fn indent(&mut self, d: &mut Loaded, id: &str) {
        let Some((p, pos)) = Self::parent_pos(d, id) else { return };
        if pos == 0 {
            self.msg = Some("nothing above to indent under".into());
            return;
        }
        let prev = d.kb.children(Some(&p))[pos - 1].clone();
        let to = (prev.clone(), d.kb.children(Some(&prev)).len());
        self.folded.remove(&prev);
        self.changes.apply(&mut d.kb, Op::Move { id: id.to_owned(), from: (p, pos), to });
        self.cursor_to(d, id);
    }

    fn outdent(&mut self, d: &mut Loaded, id: &str) {
        let Some((p, pos)) = Self::parent_pos(d, id) else { return };
        if Some(&p) == self.week.as_ref() {
            self.msg = Some("already at the top level of this page".into());
            return;
        }
        let Some((gp, ppos)) = Self::parent_pos(d, &p) else { return };
        self.changes.apply(&mut d.kb, Op::Move { id: id.to_owned(), from: (p, pos), to: (gp, ppos + 1) });
        self.cursor_to(d, id);
    }

    fn shift(&mut self, d: &mut Loaded, id: &str, by: isize) {
        let Some((p, pos)) = Self::parent_pos(d, id) else { return };
        let n = d.kb.children(Some(&p)).len() as isize;
        let to = pos as isize + by;
        if to < 0 || to >= n {
            return;
        }
        self.changes.apply(&mut d.kb, Op::Move { id: id.to_owned(), from: (p.clone(), pos), to: (p, to as usize) });
        self.cursor_to(d, id);
    }

    fn delete(&mut self, d: &mut Loaded, id: &str) {
        let Some((p, pos)) = Self::parent_pos(d, id) else { return };
        let docs = edit::snapshot(&d.kb, id);
        let n = docs.len();
        self.changes.apply(&mut d.kb, Op::Delete { id: id.to_owned(), parent: p, pos, docs });
        if n > 1 {
            self.msg = Some(format!("deleted {n} rems (u to undo)"));
        }
    }

    /// Normal-mode editing keys on the notes pane. Returns true if handled.
    pub(super) fn edit_key(&mut self, d: &mut Loaded, prefix: &str, k: K, n: usize, rows: &[(String, usize)]) -> bool {
        if self.focus != Focus::Notes || self.week.is_none() {
            return false;
        }
        let cur = rows.get(self.note_cur).map(|r| r.0.clone());
        match (prefix, k) {
            ("", K::Ch('i' | 'I')) | ("", K::Ch('a' | 'A')) if cur.is_some() => {
                let end = matches!(k, K::Ch('a' | 'A'));
                self.start_edit(d, cur.as_deref().unwrap(), end, false);
            }
            ("", K::Ch('o')) | ("", K::Ch('O')) => {
                if let Some(id) = self.new_rem(d, cur.as_deref(), matches!(k, K::Ch('O'))) {
                    self.start_edit(d, &id, true, true);
                }
            }
            ("d", K::Ch('d')) => {
                if let Some(c) = cur {
                    self.delete(d, &c);
                }
            }
            ("", K::Ch('d')) => self.pending.push('d'),
            (">", K::Ch('>')) => {
                if let Some(c) = cur {
                    for _ in 0..n {
                        self.indent(d, &c);
                    }
                }
            }
            ("<", K::Ch('<')) => {
                if let Some(c) = cur {
                    for _ in 0..n {
                        self.outdent(d, &c);
                    }
                }
            }
            ("", K::Ch('>')) => self.pending.push('>'),
            ("", K::Ch('<')) => self.pending.push('<'),
            ("", K::Alt('j')) | ("", K::Alt('k')) => {
                if let Some(c) = cur {
                    let by = if matches!(k, K::Alt('j')) { n as isize } else { -(n as isize) };
                    self.shift(d, &c, by);
                }
            }
            ("", K::Ch('u')) => {
                for _ in 0..n {
                    match self.changes.undo(&mut d.kb) {
                        Some(f) => self.cursor_to(d, &f),
                        None => {
                            self.msg = Some("nothing to undo".into());
                            break;
                        }
                    }
                }
            }
            ("", K::Ctrl('r')) => {
                for _ in 0..n {
                    match self.changes.redo(&mut d.kb) {
                        Some(f) => self.cursor_to(d, &f),
                        None => break,
                    }
                }
            }
            _ => return false,
        }
        true
    }

    pub(super) fn handle_insert(&mut self, d: &mut Loaded, k: K) {
        // autocomplete navigation / accept
        if let Some(e) = &mut self.editing {
            if let Some(ac) = &mut e.ac {
                if !ac.items.is_empty() {
                    match k {
                        K::Down => {
                            ac.cur = (ac.cur + 1) % ac.items.len();
                            return;
                        }
                        K::Up => {
                            ac.cur = (ac.cur + ac.items.len() - 1) % ac.items.len();
                            return;
                        }
                        K::Tab | K::Enter => {
                            let (id, name) = ac.items[ac.cur].clone();
                            let chars: Vec<char> = e.buf.chars().collect();
                            let before: String = chars[..ac.start].iter().collect();
                            let after: String = chars[ac.end..].iter().collect();
                            let token = format!("[[{name}]]");
                            e.side.refs.insert(name.clone(), id);
                            e.buf = format!("{before}{token}{after}");
                            let at = ac.start + token.chars().count();
                            e.ac = None;
                            self.pending_cursor = Some(at);
                            return;
                        }
                        K::Esc => {
                            e.ac = None;
                            return;
                        }
                        _ => {}
                    }
                }
            }
        }
        match k {
            // only replayed keys (M3M0_KEYS) arrive as Ch in insert mode: type them
            K::Ch(c) => {
                if let Some(e) = &mut self.editing {
                    e.buf.push(c);
                    self.pending_cursor = Some(e.buf.chars().count());
                }
            }
            K::Esc => {
                if let Some(id) = self.finish_edit(d) {
                    self.cursor_to(d, &id);
                }
            }
            K::Enter => {
                if let Some(id) = self.finish_edit(d) {
                    if let Some(n) = self.new_rem(d, Some(&id), false) {
                        self.start_edit(d, &n, true, true);
                    }
                }
            }
            K::Tab | K::ShiftTab => {
                let at_end = true;
                if let Some(id) = self.finish_edit(d) {
                    if matches!(k, K::Tab) {
                        self.indent(d, &id);
                    } else {
                        self.outdent(d, &id);
                    }
                    self.start_edit(d, &id, at_end, false);
                }
            }
            K::Back => {
                // backspace on an empty bullet: delete it, continue at the end of the one above
                let Some(e) = self.editing.take() else { return };
                let page = self.week.clone().unwrap_or_default();
                let rows = note_rows(&d.kb, &page, &self.folded);
                let i = rows.iter().position(|r| r.0 == e.id).unwrap_or(0);
                if e.created {
                    self.changes.undo(&mut d.kb);
                    self.changes.redo.clear();
                } else if d.kb.children(Some(&e.id)).is_empty() {
                    self.delete(d, &e.id);
                } else {
                    self.editing = Some(e);
                    return;
                }
                if i > 0 {
                    let prev = rows[i - 1].0.clone();
                    self.cursor_to(d, &prev);
                    self.start_edit(d, &prev, true, false);
                }
            }
            _ => {}
        }
    }
}

/// The text box for the bullet being edited, plus the [[ autocomplete popup.
pub fn edit_ui(ui: &mut egui::Ui, e: &mut Editing, kb: &Kb, pending_cursor: &mut Option<usize>) {
    let id = egui::Id::new(EDIT_ID);
    let out = egui::TextEdit::multiline(&mut e.buf)
        .id(id)
        .font(FontId::monospace(NOTE_TEXT))
        .text_color(FG_BRIGHT)
        .desired_rows(1)
        .desired_width(f32::INFINITY)
        .frame(false)
        .lock_focus(true)
        .show(ui);
    let set = if e.fresh {
        e.fresh = false;
        Some(if e.at_end { e.buf.chars().count() } else { 0 })
    } else {
        pending_cursor.take()
    };
    if let Some(at) = set {
        let mut st = out.state.clone();
        st.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(at))));
        st.store(ui.ctx(), id);
        out.response.request_focus();
    }
    if !out.response.has_focus() {
        out.response.request_focus();
    }

    // autocomplete: text between the last unclosed [[ and the cursor
    let caret = out.cursor_range.map(|r| r.primary.ccursor.index).unwrap_or(e.buf.chars().count());
    let chars: Vec<char> = e.buf.chars().collect();
    let upto: String = chars[..caret.min(chars.len())].iter().collect();
    let open = upto.rfind("[[").filter(|&b| !upto[b..].contains("]]") && !upto[b..].contains('\n'));
    match open {
        Some(b) => {
            let start = upto[..b].chars().count();
            let query = upto[b + 2..].to_owned();
            let stale = e.ac.as_ref().is_none_or(|a| a.query != query);
            if stale {
                let items = complete(kb, &query, &e.id);
                e.ac = Some(Ac { start, end: caret, query, items, cur: 0 });
            } else if let Some(a) = &mut e.ac {
                a.end = caret;
            }
        }
        None => e.ac = None,
    }
    if let Some(a) = &e.ac {
        if !a.items.is_empty() {
            let at = out.response.rect.left_bottom() + Vec2::new(0.0, 4.0);
            egui::Area::new(egui::Id::new("ac")).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
                egui::Frame::new().fill(BG_ALT).stroke((1.0, GREY1)).inner_margin(4.0).show(ui, |ui| {
                    ui.set_min_width(320.0);
                    for (i, (_, name)) in a.items.iter().enumerate() {
                        let sel = i == a.cur;
                        let t = RichText::new(name).size(NOTE_TEXT - 1.0).color(if sel { FG_BRIGHT } else { FG_DIM });
                        let r = egui::Frame::new().fill(if sel { SELECTION } else { Color32::TRANSPARENT }).inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(egui::Label::new(t).truncate().selectable(false));
                        });
                        if sel {
                            ui.painter().rect_filled(Rect::from_min_size(r.response.rect.min, Vec2::new(2.0, r.response.rect.height())), 0.0, RED_CURSOR);
                        }
                    }
                    ui.label(RichText::new("tab/enter pick · ctrl-n/p move · esc close").size(11.0).color(COMMENT));
                });
            });
        }
    }
}

/// Rem names matching `q`: prefix matches first, then same module, then shorter.
fn complete(kb: &Kb, q: &str, near: &str) -> Vec<(String, String)> {
    let ql = q.to_lowercase();
    let module = kb.module_of(near);
    let mut hits: Vec<(u8, bool, usize, String, String)> = vec![];
    for id in kb.q.keys() {
        let name = kb.name(id).replace(['[', ']'], "").replace('\n', " ");
        if name.is_empty() || name.len() > 120 || id == near || kb.is_hidden(id) {
            continue;
        }
        let nl = name.to_lowercase();
        let rank = if nl.starts_with(&ql) {
            0
        } else if nl.contains(&ql) {
            1
        } else {
            continue;
        };
        let same = module.is_some() && kb.module_of(id) == module;
        hits.push((rank, !same, name.len(), name, id.clone()));
    }
    hits.sort();
    hits.dedup_by(|a, b| a.3 == b.3);
    hits.into_iter().take(8).map(|h| (h.4, h.3)).collect()
}
