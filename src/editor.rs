//! Vim-style editing of the note outline: insert mode on the cursor bullet
//! (raw deck syntax, rich.rs), structure ops, undo, and [[ autocomplete.
//! Every change goes through edit::Changes, so it's staged for `:w`.

use super::*;
use crate::edit::{LineJson, Op};
use crate::typing;
use egui::text::{CCursor, CCursorRange};

pub struct Editing {
    pub id: String,
    pub buf: String,
    pub side: rich::Side,
    pub orig: rich::Line,
    pub fresh: bool,
    /// made by o/O/enter: dropped again if left empty
    pub created: bool,
    pub ac: Option<Ac>,
    /// selection (anchor, caret) in chars, kept in step with the text box
    pub sel: (usize, usize),
    /// set when we changed text or caret: the box takes `sel` next frame
    pub push: bool,
    /// last automatic change (typing.rs), for backspace-to-undo
    pub undo: Option<typing::Undo>,
    /// `/` menu: char index of the `/`, highlighted item
    pub slash: Option<(usize, usize)>,
    /// insert, or vim normal / visual inside the line (esc from insert)
    pub mode: LineMode,
    /// pending operator in line-normal mode: d c y, then i / f t F T
    pub op: String,
    /// set the List Item flag on save (typed `1.` in a >>> card's item)
    pub list: Option<bool>,
    /// line-mode undo (`u`): text and caret before each change
    pub hist: Vec<(String, usize)>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LineMode {
    Insert,
    Normal,
    /// charwise visual from this anchor
    Visual(usize),
}

/// What y / d put away, for p / P.
#[derive(Clone)]
pub enum Reg {
    Text(String),
    /// whole bullets, root first: (old id, old parent or None for a root, line)
    Bullets(Vec<(String, Option<String>, LineJson)>),
}

/// The last change in the notes, for `.`.
#[derive(Clone)]
pub enum Repeat {
    Key(String, K, usize),
    /// o / O and what was typed into the new bullet
    Insert(bool, String),
}

impl Editing {
    fn buf(&self) -> typing::Buf {
        typing::Buf { text: self.buf.clone(), anchor: self.sel.0, caret: self.sel.1 }
    }
    fn set(&mut self, b: typing::Buf) {
        self.buf = b.text;
        self.sel = (b.anchor, b.caret);
        self.push = true;
    }
    /// Open `/` menu items for what's typed after the `/`, if it's still open.
    pub fn slash_items(&self) -> Option<Vec<usize>> {
        let (start, _) = self.slash?;
        let cs: Vec<char> = self.buf.chars().collect();
        let caret = self.sel.1;
        if cs.get(start) != Some(&'/') || caret <= start || caret > cs.len() {
            return None;
        }
        let q: String = cs[start + 1..caret].iter().collect();
        if !q.chars().all(|c| c.is_ascii_alphanumeric()) || q.len() > 12 {
            return None;
        }
        let m = typing::slash_matches(&q);
        (!m.is_empty()).then_some(m)
    }
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
    /// While editing, typed text and these keys are ours (typing.rs); the text
    /// box keeps arrows, selection, paste, delete and shift-enter.
    pub(super) fn insert_keys(&mut self, ctx: &egui::Context) -> Vec<K> {
        let menu = self.editing.as_ref().is_some_and(|e| e.ac.as_ref().is_some_and(|a| !a.items.is_empty()) || e.slash_items().is_some());
        let vim = self.editing.as_ref().is_some_and(|e| e.mode != LineMode::Insert);
        let mut out = vec![];
        ctx.input_mut(|i| {
            let mods = i.modifiers;
            if vim {
                // vim normal / visual in the line: every key is a command
                i.events.retain(|e| match e {
                    egui::Event::Text(t) if !mods.ctrl && !mods.alt => {
                        out.extend(t.chars().map(K::Ch));
                        false
                    }
                    egui::Event::Key { key, pressed, modifiers, .. } => {
                        use egui::Key as E;
                        let k = match key {
                            E::Escape => Some(K::Esc),
                            E::Enter => Some(K::Enter),
                            E::Backspace => Some(K::Back),
                            E::Tab if modifiers.shift => Some(K::ShiftTab),
                            E::Tab => Some(K::Tab),
                            E::ArrowLeft => Some(K::Left),
                            E::ArrowRight => Some(K::Right),
                            E::ArrowUp => Some(K::Up),
                            E::ArrowDown => Some(K::Down),
                            _ if modifiers.ctrl || modifiers.alt => return true,
                            _ => None,
                        };
                        if let (Some(k), true) = (k, *pressed) {
                            out.push(k);
                        }
                        false
                    }
                    egui::Event::Paste(_) | egui::Event::Cut | egui::Event::Copy => false,
                    _ => true,
                });
                return;
            }
            i.events.retain(|e| {
                if let egui::Event::Text(t) = e {
                    if !mods.ctrl && !mods.alt {
                        out.extend(t.chars().map(K::Ch));
                        return false;
                    }
                    return true;
                }
                let egui::Event::Key { key, pressed, modifiers, .. } = e else { return true };
                use egui::Key as E;
                let plain = !modifiers.ctrl && !modifiers.alt;
                let k = match key {
                    E::Escape => Some(K::Esc),
                    E::Enter if !modifiers.shift => Some(K::Enter),
                    E::Tab if modifiers.shift => Some(K::ShiftTab),
                    E::Tab => Some(K::Tab),
                    E::Backspace if plain => Some(K::Back),
                    E::ArrowDown | E::ArrowUp if menu => Some(if *key == E::ArrowDown { K::Down } else { K::Up }),
                    E::N | E::P if menu && modifiers.ctrl => Some(if *key == E::N { K::Down } else { K::Up }),
                    E::B | E::I | E::M | E::Backtick if modifiers.ctrl && !modifiers.shift => Some(K::Ctrl(match key {
                        E::B => 'b',
                        E::I => 'i',
                        E::M => 'm',
                        _ => '`',
                    })),
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
        let at = if at_end { buf.chars().count() } else { 0 };
        let orig_text = buf.clone();
        self.editing = Some(Editing { id: id.to_owned(), buf, side, orig, fresh: true, created, ac: None, sel: (at, at), push: true, undo: None, slash: None, mode: LineMode::Insert, op: String::new(), list: None, hist: vec![(orig_text, at)] });
        self.focus = Focus::Notes;
    }

    /// Parse the buffer back and stage an Update if anything changed.
    /// Returns the rem id, or None if it was a blank new rem that got dropped.
    pub(super) fn finish_edit(&mut self, d: &mut Loaded) -> Option<String> {
        let e = self.editing.take()?;
        if let Some(above) = self.insert_from.take() {
            if e.created && !e.buf.trim().is_empty() {
                self.last_change = Some(Repeat::Insert(above, e.buf.clone()));
            }
        }
        if e.created && e.buf.trim().is_empty() {
            self.changes.drop_create(&mut d.kb, &e.id); // drop the Create
            return None;
        }
        let near = e.id.clone();
        let line = rich::text_to_line(&e.buf, &|n| d.kb.resolve(n, Some(&near)), &e.side, &mut cloze_id);
        let (mut before, mut after) = (LineJson::of(&e.orig), LineJson::of(&line));
        if let Some(l) = e.list {
            before.list = Some(d.kb.is_list_item(&e.id));
            after.list = Some(l);
        }
        // a bullet just made with o/O/enter: fold the text into its Create
        if e.created {
            if let Some(Op::Create { id, line: l, .. }) = self.changes.ops.last_mut() {
                if *id == e.id {
                    after.list = after.list.or(l.list);
                    let list = after.list;
                    self.changes.version += 1;
                    *l = after;
                    crate::edit::set_line(&mut d.kb, &e.id, &line);
                    if let Some(on) = list {
                        crate::edit::set_list(&mut d.kb, &e.id, on);
                    }
                    return Some(e.id);
                }
            }
        }
        if before != after {
            self.changes.apply(&mut d.kb, Op::Update { id: e.id.clone(), before, after });
        }
        Some(e.id)
    }

    /// Tab / shift-tab on a bullet made in this edit (o/O/enter) whose Create is
    /// still the last op: re-stage the Create under the new parent, keep editing.
    fn move_created(&mut self, d: &mut Loaded, indent: bool) -> bool {
        let Some(e) = &self.editing else { return false };
        if !e.created {
            return false;
        }
        let id = e.id.clone();
        let Some(Op::Create { id: cid, line, .. }) = self.changes.ops.last() else { return false };
        if *cid != id {
            return false;
        }
        let line = line.clone();
        let Some((p, pos)) = Self::parent_pos(d, &id) else { return false };
        let to = if indent {
            if pos == 0 {
                self.msg = Some("nothing above to indent under".into());
                return true;
            }
            let prev = d.kb.children(Some(&p))[pos - 1].clone();
            self.folded.remove(&prev);
            let n = d.kb.children(Some(&prev)).len();
            (prev, n)
        } else {
            if Some(&p) == self.week.as_ref() {
                self.msg = Some("already at the top level of this page".into());
                return true;
            }
            let Some((gp, ppos)) = Self::parent_pos(d, &p) else { return false };
            (gp, ppos + 1)
        };
        if !self.changes.drop_create(&mut d.kb, &id) {
            return false;
        }
        self.changes.apply(&mut d.kb, Op::Create { id: id.clone(), parent: to.0, pos: to.1, line });
        self.cursor_to(d, &id);
        true
    }

    /// Enter in insert mode. Empty bullet: outdent it (at the top level: drop
    /// it and leave insert). Caret mid-line: split, the rest goes to a new
    /// bullet. At the end: a new bullet (a child under >>> / >>A) / headings).
    fn enter(&mut self, d: &mut Loaded) {
        let Some(e) = &self.editing else { return };
        let id = e.id.clone();
        if e.buf.trim().is_empty() && d.kb.children(Some(&id)).is_empty() {
            let top = Self::parent_pos(d, &id).is_none_or(|(p, _)| Some(&p) == self.week.as_ref());
            if !top {
                if !self.move_created(d, false) {
                    if let Some(id) = self.finish_edit(d) {
                        self.outdent(d, &id);
                        self.start_edit(d, &id, true, false);
                    }
                }
                return;
            }
            let created = e.created;
            let page = self.week.clone().unwrap_or_default();
            let rows = note_rows(&d.kb, &page, &self.folded);
            let i = rows.iter().position(|r| r.0 == id).unwrap_or(0);
            self.editing = None;
            if !(created && self.changes.drop_create(&mut d.kb, &id)) {
                self.delete(d, &id);
            }
            if let Some(prev) = i.checked_sub(1).and_then(|i| rows.get(i)) {
                let prev = prev.0.clone();
                self.cursor_to(d, &prev);
            }
            return;
        }
        let (lo, hi) = (e.sel.0.min(e.sel.1), e.sel.0.max(e.sel.1));
        let n = e.buf.chars().count();
        if hi < n && lo > 0 {
            // split: this bullet keeps the text before the caret
            let rest: String = e.buf.chars().skip(hi).collect();
            let keep: String = e.buf.chars().take(lo).collect();
            let side = e.side.clone();
            if let Some(e) = &mut self.editing {
                e.buf = keep.trim_end().to_owned();
            }
            self.changes.begin();
            let id = self.finish_edit(d);
            let new = id.and_then(|id| self.new_rem(d, Some(&id), false));
            self.changes.end();
            if let Some(nid) = new {
                self.start_edit(d, &nid, false, true);
                if let Some(e) = &mut self.editing {
                    e.buf = rest.trim_start().to_owned();
                    e.side = side;
                    e.sel = (0, 0);
                }
            }
            return;
        }
        if let Some(id) = self.finish_edit(d) {
            if let Some(n) = self.new_rem(d, Some(&id), false) {
                self.start_edit(d, &n, true, true);
            }
        }
    }

    /// Number every item of >>> card `card` (the one being typed, `editing`,
    /// gets it when saved).
    fn make_ordered(&mut self, d: &mut Loaded, card: &str, editing: &str) {
        self.changes.begin();
        for k in d.kb.children(Some(card)).to_vec() {
            if k == editing || d.kb.is_list_item(&k) {
                continue;
            }
            let l = LineJson::of(&d.kb.line_of(&k));
            let (mut before, mut after) = (l.clone(), l);
            before.list = Some(false);
            after.list = Some(true);
            self.changes.apply(&mut d.kb, Op::Update { id: k, before, after });
        }
        self.changes.end();
        if let Some(e) = &mut self.editing {
            e.buf = e.buf[2..].trim_start().to_owned();
            e.sel = (0, 0);
            e.push = true;
            e.list = Some(true);
        }
        self.msg = Some("ordered card: its items are numbered".into());
    }

    /// `J`: append the next bullet's text to this one (its children move over).
    fn join(&mut self, d: &mut Loaded, rows: &[(String, usize)]) {
        let i = self.note_cur;
        let (Some((cur, _)), Some((next, _))) = (rows.get(i).cloned(), rows.get(i + 1).cloned()) else {
            self.msg = Some("nothing below to join".into());
            return;
        };
        let (a, b) = (d.kb.line_of(&cur), d.kb.line_of(&next));
        if a.kind == rich::Kind::Code || b.kind == rich::Kind::Code {
            self.msg = Some("can't join code blocks".into());
            return;
        }
        let sp = rich::Span::Text { t: " ".into(), b: false, i: false, u: false, code: false, cloze: None, extra: Default::default() };
        let mut l = a.clone();
        let tail = b.front.clone();
        if a.back.is_empty() {
            l.front.push(sp.clone());
            l.front.extend(tail);
            if matches!(a.kind, rich::Kind::Plain | rich::Kind::Heading(_)) && b.kind != rich::Kind::Plain {
                l.kind = if matches!(b.kind, rich::Kind::Heading(_)) { a.kind } else { b.kind };
                l.back = b.back.clone();
            }
        } else {
            if !l.back.is_empty() {
                l.back.push(sp.clone());
            }
            l.back.extend(tail);
            if !b.back.is_empty() {
                l.back.push(sp);
                l.back.extend(b.back.iter().cloned());
            }
        }
        let kids = d.kb.children(Some(&next)).to_vec();
        let base = match Self::parent_pos(d, &next) {
            Some((p, pos)) if p == cur => pos,
            _ => d.kb.children(Some(&cur)).len(),
        };
        self.changes.begin();
        for (j, k) in kids.iter().enumerate() {
            let from = (next.clone(), 0);
            self.changes.apply(&mut d.kb, Op::Move { id: k.clone(), from, to: (cur.clone(), base + j) });
        }
        self.changes.apply(&mut d.kb, Op::Update { id: cur.clone(), before: LineJson::of(&a), after: LineJson::of(&l) });
        self.delete(d, &next);
        self.changes.end();
        self.cursor_to(d, &cur);
    }

    /// Paste a register below (or above) the cursor bullet, as its sibling.
    fn paste(&mut self, d: &mut Loaded, cur: Option<&str>, above: bool, r: &Reg) {
        let Some(page) = self.week.clone() else { return };
        let (parent, pos) = match cur.and_then(|c| Self::parent_pos(d, c)) {
            Some((p, i)) => (p, if above { i } else { i + 1 }),
            None => (page, 0),
        };
        let items = match r {
            Reg::Bullets(b) => b.clone(),
            Reg::Text(t) => {
                let side = rich::Side::default();
                let near = parent.clone();
                let l = rich::text_to_line(t, &|n| d.kb.resolve(n, Some(&near)), &side, &mut cloze_id);
                vec![(String::new(), None, LineJson::of(&l))]
            }
        };
        let mut map: std::collections::HashMap<String, String> = Default::default();
        let mut slots: std::collections::HashMap<String, usize> = Default::default();
        let mut first = None;
        let mut roots = 0;
        self.changes.begin();
        for (old, op, line) in items {
            let id = self.changes.tmp_id();
            let (par, at) = match op.and_then(|p| map.get(&p).cloned()) {
                Some(np) => {
                    let s = slots.entry(np.clone()).or_default();
                    *s += 1;
                    (np, *s - 1)
                }
                None => {
                    roots += 1;
                    (parent.clone(), pos + roots - 1)
                }
            };
            self.changes.apply(&mut d.kb, Op::Create { id: id.clone(), parent: par, pos: at, line });
            map.insert(old, id.clone());
            first.get_or_insert(id);
        }
        self.changes.end();
        if let Some(f) = first {
            self.cursor_to(d, &f);
        }
    }

    /// `V` on the bullets: j/k extend, d y > < act on every bullet in range.
    fn visual_key(&mut self, d: &mut Loaded, prefix: &str, k: K, n: usize, rows: &[(String, usize)]) -> bool {
        let Some(anchor) = self.visual.clone() else { return false };
        if rows.is_empty() {
            self.visual = None;
            return false;
        }
        let a = rows.iter().position(|r| r.0 == anchor).unwrap_or(self.note_cur);
        let (lo, hi) = (a.min(self.note_cur), a.max(self.note_cur));
        // the selected bullets whose parent isn't selected too (children come along)
        let sel: Vec<String> = rows[lo..=hi.min(rows.len().saturating_sub(1))].iter().map(|r| r.0.clone()).collect();
        let tops: Vec<String> = sel
            .iter()
            .filter(|id| Self::parent_pos(d, id).is_none_or(|(p, _)| !sel.contains(&p)))
            .cloned()
            .collect();
        match (prefix, k) {
            (_, K::Esc | K::Ch('V')) => self.visual = None,
            (_, K::Ch('d' | 'x')) => {
                self.reg = Some(Reg::Bullets(clip(&d.kb, &tops)));
                self.changes.begin();
                for t in &tops {
                    self.delete(d, t);
                }
                self.changes.end();
                self.visual = None;
                self.note_cur = lo;
                self.msg = Some(format!("deleted {} (u to undo)", plural(tops.len(), "bullet")));
            }
            (_, K::Ch('y')) => {
                let c = clip(&d.kb, &tops);
                self.msg = Some(format!("yanked {}", plural(c.len(), "bullet")));
                self.reg = Some(Reg::Bullets(c));
                self.visual = None;
                self.note_cur = lo;
            }
            (_, K::Ch('>' | '<')) => {
                self.changes.begin();
                // indent top-down, outdent bottom-up, so siblings keep their order
                let order: Vec<&String> = if matches!(k, K::Ch('>')) { tops.iter().collect() } else { tops.iter().rev().collect() };
                for _ in 0..n {
                    for t in &order {
                        if matches!(k, K::Ch('>')) {
                            self.indent(d, t);
                        } else {
                            self.outdent(d, t);
                        }
                    }
                }
                self.changes.end();
                self.cursor_to(d, &anchor);
                let rows = note_rows(&d.kb, self.week.as_deref().unwrap_or_default(), &self.folded);
                let last = sel.last().cloned().unwrap_or_default();
                self.note_cur = rows.iter().position(|r| r.0 == last).unwrap_or(self.note_cur);
            }
            _ => return false,
        }
        true
    }

    /// vim normal / visual inside the line being edited:
    /// `[count] [op [count]] motion | text object`, plus x X r ~ s S D C p P u.
    fn line_normal(&mut self, d: &mut Loaded, k: K) {
        let Some(e) = &mut self.editing else { return };
        let mode = e.mode;
        let visual = matches!(mode, LineMode::Visual(_));
        // keys that aren't commands in the line
        match k {
            K::Esc => {
                if !e.op.is_empty() {
                    e.op.clear();
                } else if visual {
                    e.mode = LineMode::Normal;
                } else if let Some(id) = self.finish_edit(d) {
                    self.cursor_to(d, &id);
                }
                return;
            }
            K::Up | K::Down | K::Enter => return self.leave_line(d, if matches!(k, K::Up) { -1 } else { 1 }),
            _ => {}
        }
        let c = match k {
            K::Ch(c) => c,
            K::Left | K::Back => 'h',
            K::Right => 'l',
            _ => return,
        };
        e.op.push(c);
        let parsed = typing::parse(&e.op, visual);
        if parsed == typing::Parsed::More {
            return;
        }
        e.op.clear();
        let cs: Vec<char> = e.buf.chars().collect();
        let n = cs.len();
        let last = n.saturating_sub(1);
        let caret = e.sel.1.min(last);
        match parsed {
            typing::Parsed::More | typing::Parsed::Bad => {}
            typing::Parsed::Move { count, op, target } => {
                use typing::Target as T;
                // text objects in visual mode select
                if let (true, T::Obj(inner, o)) = (visual, target) {
                    if let Some((a, z)) = typing::object(&cs, caret, inner, o).filter(|r| r.1 > r.0) {
                        e.mode = LineMode::Visual(a);
                        e.sel = (z - 1, z - 1);
                        e.push = true;
                    }
                    return;
                }
                let range: Option<(usize, usize)> = match target {
                    T::Line => Some((0, n)),
                    T::Obj(inner, o) => typing::object(&cs, caret, inner, o),
                    T::Find(f, ch) => typing::find_n(&cs, caret, f, ch, count).map(|j| if "ft".contains(f) { (caret, j + 1) } else { (j, caret) }),
                    // cw on a word changes to its end (vim's quirk)
                    T::Motion('w') if op == Some('c') && cs.get(caret).is_some_and(|c| !c.is_whitespace()) => {
                        let mut z = typing::inner_word(&cs, caret).1;
                        for _ in 1..count {
                            z = (typing::word_end(&cs, z.saturating_sub(1)) + 1).min(n);
                        }
                        Some((caret, z))
                    }
                    T::Motion(m) => {
                        let t = typing::motion(&cs, caret, m, count);
                        let incl = "eE".contains(m);
                        Some(if t >= caret { (caret, if incl { (t + 1).min(n) } else { t }) } else { (t, caret) })
                    }
                };
                match op {
                    None => {
                        // a plain motion: move (never past the last char)
                        let to = match target {
                            T::Find(f, ch) => typing::find_n(&cs, caret, f, ch, count),
                            T::Motion(m) => Some(typing::motion(&cs, caret, m, count).min(last)),
                            _ => None,
                        };
                        if let Some(t) = to {
                            e.sel = (t, t);
                            e.push = true;
                        }
                    }
                    Some('d') if target == T::Line => {
                        let (id, created) = (e.id.clone(), e.created);
                        self.editing = None;
                        self.reg = Some(Reg::Bullets(clip(&d.kb, std::slice::from_ref(&id))));
                        if !(created && self.changes.drop_create(&mut d.kb, &id)) {
                            self.delete(d, &id);
                        }
                    }
                    Some(op) => {
                        if let Some((a, z)) = range {
                            self.line_op(op, a, z);
                        }
                    }
                }
            }
            typing::Parsed::Other { count, key, arg } => self.line_other(d, key, count, arg, &cs, caret),
        }
    }

    /// d / c / y over chars a..z of the line being edited.
    fn line_op(&mut self, op: char, a: usize, z: usize) {
        let Some(e) = &mut self.editing else { return };
        let n = e.buf.chars().count();
        let (a, z) = (a.min(n), z.min(n));
        let text: String = e.buf.chars().skip(a).take(z - a).collect();
        if !text.is_empty() {
            self.reg = Some(Reg::Text(text));
        }
        if op != 'y' && z > a {
            e.hist.push((e.buf.clone(), e.sel.1));
            let mut b = e.buf();
            b.anchor = a;
            b.caret = z;
            typing::backspace(&mut b, &mut None);
            e.set(b);
        }
        if op == 'c' {
            e.hist.push((e.buf.clone(), a));
            e.mode = LineMode::Insert;
            e.sel = (a, a);
        } else {
            let left = e.buf.chars().count();
            let c = a.min(left.saturating_sub(1));
            e.sel = (c, c);
            e.mode = LineMode::Normal;
        }
        e.push = true;
    }

    /// Line-mode commands that aren't motions.
    fn line_other(&mut self, d: &mut Loaded, key: char, count: usize, arg: Option<char>, cs: &[char], caret: usize) {
        let Some(e) = &mut self.editing else { return };
        let n = cs.len();
        let visual = match e.mode {
            LineMode::Visual(a) => Some((a.min(caret), (a.max(caret) + 1).min(n))),
            _ => None,
        };
        let insert = |e: &mut Editing, c: usize| {
            e.hist.push((e.buf.clone(), c));
            e.sel = (c, c);
            e.push = true;
            e.mode = LineMode::Insert;
        };
        let edit = |e: &mut Editing, f: &mut dyn FnMut(&mut Vec<char>) -> usize| {
            e.hist.push((e.buf.clone(), e.sel.1));
            let mut v: Vec<char> = e.buf.chars().collect();
            let c = f(&mut v);
            e.buf = v.into_iter().collect();
            let c = c.min(e.buf.chars().count().saturating_sub(1));
            e.sel = (c, c);
            e.push = true;
        };
        match (key, visual) {
            ('d' | 'x' | 'X' | 'c' | 's' | 'y', Some((a, z))) => {
                let op = match key {
                    'x' | 'X' => 'd',
                    's' => 'c',
                    k => k,
                };
                self.line_op(op, a, z);
            }
            ('~', Some((a, z))) => {
                edit(e, &mut |v| {
                    for c in &mut v[a..z] {
                        *c = flip(*c);
                    }
                    a
                });
                e.mode = LineMode::Normal;
            }
            ('v', Some(_)) => e.mode = LineMode::Normal,
            ('v', None) if n > 0 => e.mode = LineMode::Visual(caret),
            ('i', _) => insert(e, caret),
            ('a', _) => insert(e, (caret + 1).min(n)),
            ('I', _) => insert(e, typing::first_non_blank(cs)),
            ('A', _) => insert(e, n),
            ('x', None) if n > 0 => {
                let z = (caret + count).min(n);
                self.line_op('d', caret, z);
            }
            ('X', None) if caret > 0 => {
                let a = caret.saturating_sub(count);
                self.line_op('d', a, caret);
            }
            ('s', None) => self.line_op('c', caret, (caret + count).min(n)),
            ('S', _) => self.line_op('c', 0, n),
            ('D', None) => self.line_op('d', caret, n),
            ('C', None) => self.line_op('c', caret, n),
            ('r', None) if n > 0 => {
                if let Some(ch) = arg {
                    let z = (caret + count).min(n);
                    edit(e, &mut |v| {
                        for c in &mut v[caret..z] {
                            *c = ch;
                        }
                        z - 1
                    });
                }
            }
            ('~', None) if n > 0 => {
                let z = (caret + count).min(n);
                edit(e, &mut |v| {
                    for c in &mut v[caret..z] {
                        *c = flip(*c);
                    }
                    z
                });
            }
            ('u', _) => match e.hist.pop() {
                Some((b, c)) => {
                    e.buf = b;
                    let c = c.min(e.buf.chars().count().saturating_sub(1));
                    e.sel = (c, c);
                    e.push = true;
                    e.mode = LineMode::Normal;
                }
                None => self.msg = Some("nothing to undo in this line".into()),
            },
            ('p' | 'P', _) => {
                let t = match &self.reg {
                    Some(Reg::Text(t)) => t.clone(),
                    Some(Reg::Bullets(b)) => b.iter().filter(|x| x.1.is_none()).map(|x| rich::plain(&x.2.line().front, &|r| d.kb.name(r))).collect::<Vec<_>>().join(" "),
                    None => return,
                };
                let t = t.repeat(count);
                let at = if key == 'p' && n > 0 { caret + 1 } else { caret };
                // pasted text goes in as is, no auto-anything
                edit(e, &mut |v| {
                    for (i, ch) in t.chars().enumerate() {
                        v.insert(at + i, ch);
                    }
                    (at + t.chars().count()).saturating_sub(1)
                });
            }
            ('o', None) => {
                e.sel = (n, n);
                e.mode = LineMode::Insert;
                self.enter(d);
            }
            ('V', _) => {
                if let Some(id) = self.finish_edit(d) {
                    self.cursor_to(d, &id);
                    self.visual = Some(id);
                }
            }
            ('j', _) => self.leave_line(d, count as isize),
            ('k', _) => self.leave_line(d, -(count as isize)),
            _ => {}
        }
    }

    /// Save the line and move the bullet cursor by `by`.
    fn leave_line(&mut self, d: &mut Loaded, by: isize) {
        if let Some(id) = self.finish_edit(d) {
            self.cursor_to(d, &id);
            let page = self.week.clone().unwrap_or_default();
            let len = note_rows(&d.kb, &page, &self.folded).len();
            self.note_cur = (self.note_cur as isize + by).clamp(0, len as isize - 1) as usize;
        }
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

    /// New empty rem after `after` (as its first child if it has visible children
    /// or is a >>> / >>A) card or a heading, see `nests`),
    /// or before it with `above`; at the top of the page if there's no cursor row.
    fn new_rem(&mut self, d: &mut Loaded, after: Option<&str>, above: bool) -> Option<String> {
        let page = self.week.clone()?;
        let (parent, pos) = match after {
            None => (page, 0),
            Some(a) => {
                let has_open_kids = !d.kb.children(Some(a)).is_empty() && !self.folded.contains(a);
                if !above && (has_open_kids || nests(&d.kb, a)) {
                    self.folded.remove(a);
                    (a.to_owned(), 0)
                } else {
                    let (p, i) = Self::parent_pos(d, a)?;
                    (p, if above { i } else { i + 1 })
                }
            }
        };
        let id = self.changes.tmp_id();
        let line = rich::Line { kind: rich::Kind::Plain, front: vec![], back: vec![] };
        let mut line = LineJson::of(&line);
        // a new item of an ordered >>> card is numbered too
        if d.kb.line_of(&parent).kind == rich::Kind::MultiLine && d.kb.children(Some(&parent)).iter().any(|k| d.kb.is_list_item(k)) {
            line.list = Some(true);
        }
        self.changes.apply(&mut d.kb, Op::Create { id: id.clone(), parent, pos, line });
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
        // cards view hides the notes: editing there typed into an invisible bullet
        if self.focus != Focus::Notes || self.week.is_none() || self.cards.is_some() {
            return false;
        }
        // a commit in flight owns the staged ops: no edits until it's back
        if self.committing.is_some() {
            if matches!(k, K::Ch('i' | 'I' | 'a' | 'A' | 'o' | 'O' | 'd' | '>' | '<' | 'u' | 'p' | 'P' | 'J' | '.' | 'V') | K::Alt(_) | K::Ctrl('r')) {
                self.msg = Some("committing… edits wait until it's done".into());
                return true;
            }
            return false;
        }
        let cur = rows.get(self.note_cur).map(|r| r.0.clone());
        if self.visual.is_some() {
            return self.visual_key(d, prefix, k, n, rows);
        }
        let repeatable = matches!(
            (prefix, k),
            ("d", K::Ch('d')) | (">", K::Ch('>')) | ("<", K::Ch('<')) | ("", K::Alt('h' | 'j' | 'k' | 'l')) | ("", K::Ch('J' | 'p' | 'P'))
        );
        if repeatable {
            self.last_change = Some(Repeat::Key(prefix.to_owned(), k, n));
        }
        match (prefix, k) {
            ("", K::Ch('.')) => {
                let Some(r) = self.last_change.clone() else {
                    self.msg = Some("nothing to repeat".into());
                    return true;
                };
                match r {
                    Repeat::Key(p, k, m) => {
                        let rows = note_rows(&d.kb, self.week.as_deref().unwrap_or_default(), &self.folded);
                        let times = if n > 1 { n } else { m };
                        self.edit_key(d, &p, k, times, &rows);
                    }
                    Repeat::Insert(above, text) => {
                        if let Some(id) = self.new_rem(d, cur.as_deref(), above) {
                            self.start_edit(d, &id, true, true);
                            if let Some(e) = &mut self.editing {
                                e.buf = text.clone();
                            }
                            self.finish_edit(d);
                            self.cursor_to(d, &id);
                            self.last_change = Some(Repeat::Insert(above, text));
                        }
                    }
                }
            }
            ("y", K::Ch('y')) => {
                if let Some(c) = cur {
                    let clip = clip(&d.kb, &[c]);
                    self.msg = Some(format!("yanked {}", plural(clip.len(), "bullet")));
                    self.reg = Some(Reg::Bullets(clip));
                }
            }
            ("", K::Ch('y')) => self.pending.push('y'),
            ("", K::Ch('p' | 'P')) => {
                if let Some(r) = self.reg.clone() {
                    for _ in 0..n {
                        self.paste(d, cur.as_deref(), matches!(k, K::Ch('P')), &r);
                    }
                } else {
                    self.msg = Some("nothing yanked".into());
                }
            }
            ("", K::Ch('V')) => {
                if let Some(c) = cur {
                    self.visual = Some(c);
                }
            }
            ("", K::Ch('i' | 'I')) | ("", K::Ch('a' | 'A')) if cur.is_some() => {
                let end = matches!(k, K::Ch('a' | 'A'));
                self.start_edit(d, cur.as_deref().unwrap(), end, false);
            }
            // o / O; and i / a on an empty page start its first bullet
            ("", K::Ch('o' | 'O' | 'i' | 'I' | 'a' | 'A')) => {
                let above = matches!(k, K::Ch('O'));
                if let Some(id) = self.new_rem(d, cur.as_deref(), above) {
                    self.start_edit(d, &id, true, true);
                    self.insert_from = Some(above);
                }
            }
            ("d", K::Ch('d')) => {
                if let Some(c) = cur {
                    self.reg = Some(Reg::Bullets(clip(&d.kb, std::slice::from_ref(&c))));
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
            ("", K::Ch('J')) => {
                for _ in 0..n {
                    let rows = note_rows(&d.kb, self.week.as_deref().unwrap_or_default(), &self.folded);
                    self.join(d, &rows);
                }
            }
            ("", K::Alt('l' | 'h')) => {
                if let Some(c) = cur {
                    for _ in 0..n {
                        if matches!(k, K::Alt('l')) {
                            self.indent(d, &c);
                        } else {
                            self.outdent(d, &c);
                        }
                    }
                }
            }
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
        if self.editing.as_ref().is_some_and(|e| e.mode != LineMode::Insert) {
            self.line_normal(d, k);
            return;
        }
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
                            // the ]] auto-paired with [[ is already there
                            let after = after.strip_prefix("]]").unwrap_or(&after).to_owned();
                            let token = format!("[[{name}]]");
                            e.side.refs.insert(name.clone(), id);
                            e.buf = format!("{before}{token}{after}");
                            let at = ac.start + token.chars().count();
                            e.ac = None;
                            e.sel = (at, at);
                            e.push = true;
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
        // `/` menu navigation / pick
        if let Some(e) = &mut self.editing {
            if let (Some(items), Some((start, cur))) = (e.slash_items(), e.slash) {
                match k {
                    K::Down => {
                        e.slash = Some((start, (cur + 1) % items.len()));
                        return;
                    }
                    K::Up => {
                        e.slash = Some((start, (cur + items.len() - 1) % items.len()));
                        return;
                    }
                    K::Tab | K::Enter => {
                        let mut b = e.buf();
                        typing::slash_apply(&mut b, start, items[cur.min(items.len() - 1)]);
                        e.set(b);
                        e.slash = None;
                        e.undo = None;
                        return;
                    }
                    K::Esc => {
                        e.slash = None;
                        return;
                    }
                    _ => {}
                }
            }
        }
        // `1. ` at the start of a >>> card's item: the card becomes ordered
        if let (K::Ch(' '), Some(e)) = (k, &self.editing) {
            if e.sel == (2, 2) && e.buf.starts_with("1.") {
                let id = e.id.clone();
                if let Some((p, _)) = Self::parent_pos(d, &id).filter(|(p, _)| d.kb.line_of(p).kind == rich::Kind::MultiLine) {
                    self.make_ordered(d, &p, &id);
                    return;
                }
            }
        }
        match k {
            K::Ch(c) => {
                if let Some(e) = &mut self.editing {
                    let mut b = e.buf();
                    let lo = b.anchor.min(b.caret);
                    let prev = lo.checked_sub(1).and_then(|i| e.buf.chars().nth(i));
                    let opens = c == '/' && prev.is_none_or(char::is_whitespace) && typing::protected_at(&e.buf, lo).is_none();
                    typing::type_char(&mut b, c, &mut e.undo);
                    e.set(b);
                    if opens {
                        e.slash = Some((lo, 0));
                    } else if e.slash.is_some() && e.slash_items().is_none() {
                        e.slash = None;
                    } else if let Some((s, _)) = e.slash {
                        e.slash = Some((s, 0));
                    }
                }
            }
            K::Ctrl(c @ ('b' | 'i' | 'm' | '`')) => {
                if let Some(e) = &mut self.editing {
                    let mut b = e.buf();
                    typing::wrap(&mut b, match c {
                        'b' => "**",
                        'i' => "*",
                        'm' => "$",
                        _ => "`",
                    });
                    e.set(b);
                    e.undo = None;
                }
            }
            K::Back if self.editing.as_ref().is_some_and(|e| !e.buf.is_empty()) => {
                if let Some(e) = &mut self.editing {
                    let mut b = e.buf();
                    typing::backspace(&mut b, &mut e.undo);
                    e.set(b);
                }
            }
            K::Esc => {
                // esc stops at a text cursor in the line first (vim normal)
                if let Some(e) = &mut self.editing {
                    e.mode = LineMode::Normal;
                    e.slash = None;
                    e.ac = None;
                    e.undo = None;
                    let c = e.sel.1.saturating_sub(1);
                    e.sel = (c, c);
                    e.push = true;
                }
            }
            K::Enter => self.enter(d),
            K::Tab | K::ShiftTab => {
                // a fresh bullet: move it by re-staging its Create (saving it would drop it if empty)
                if self.move_created(d, matches!(k, K::Tab)) {
                    return;
                }
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
                if e.created && self.changes.drop_create(&mut d.kb, &e.id) {
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

/// The bullet in front of the edit box: same glyph, size and width as
/// render::line's, red, on the edit text's baseline.
pub fn edit_bullet(ui: &mut egui::Ui) {
    let baseline = |t: RichText| {
        let g = egui::WidgetText::from(t).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::FontSelection::Default);
        let b = g.rows.first().and_then(|r| r.glyphs.first()).map(|g| g.pos.y).unwrap_or(0.0);
        (g, b)
    };
    let (dot, db) = baseline(RichText::new("•  ").size(NOTE_TEXT - 2.0).color(RED));
    let (_, tb) = baseline(RichText::new("x").font(FontId::monospace(NOTE_TEXT)));
    let h = ui.fonts(|f| f.row_height(&FontId::monospace(NOTE_TEXT)));
    let (r, _) = ui.allocate_exact_size(Vec2::new(dot.size().x, h), Sense::hover());
    ui.painter().galley(Pos2::new(r.min.x, r.min.y + tb - db), dot, RED);
    // render::line packs the bullet and text with no gap
    ui.add_space(-ui.spacing().item_spacing.x);
}

/// The text box for the bullet being edited, plus the [[ autocomplete popup.
pub fn edit_ui(ui: &mut egui::Ui, e: &mut Editing, kb: &Kb) {
    let id = egui::Id::new(EDIT_ID);
    if e.mode != LineMode::Insert {
        ui.visuals_mut().text_cursor.stroke.color = Color32::TRANSPARENT;
    }
    let out = egui::TextEdit::multiline(&mut e.buf)
        .id(id)
        .font(FontId::monospace(NOTE_TEXT))
        .text_color(FG_BRIGHT)
        .desired_rows(1)
        .desired_width(f32::INFINITY)
        .frame(false)
        .margin(egui::Margin::ZERO)
        .lock_focus(true)
        .show(ui);
    let n = e.buf.chars().count();
    if e.mode != LineMode::Insert {
        // vim normal / visual: we own the caret; the box only shows it
        if out.response.clicked() {
            if let Some(r) = out.cursor_range {
                let c = r.primary.ccursor.index.min(n.saturating_sub(1));
                e.sel = (c, c);
            }
        }
        let c = e.sel.1.min(n.saturating_sub(1));
        let (a, z) = match e.mode {
            LineMode::Visual(a) => (a.min(c), (a.max(c) + 1).min(n)),
            _ => (c, c),
        };
        let mut st = out.state.clone();
        st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(z))));
        st.store(ui.ctx(), id);
        e.push = false;
        e.fresh = false;
        if e.mode == LineMode::Normal {
            let at = |i: usize| out.galley.pos_from_ccursor(CCursor::new(i)).translate(out.galley_pos.to_vec2());
            let r = at(c);
            let nx = at(c + 1);
            let w = if c < n && (nx.min.y - r.min.y).abs() < 1.0 && nx.min.x > r.min.x { nx.min.x - r.min.x } else { NOTE_TEXT * 0.6 };
            ui.painter().rect_filled(Rect::from_min_size(r.min, Vec2::new(w, r.height())), 0.0, RED_CURSOR.gamma_multiply(0.55));
        }
        if !out.response.has_focus() {
            out.response.request_focus();
        }
        return;
    }
    if e.push {
        e.push = false;
        e.fresh = false;
        let (a, c) = (e.sel.0.min(n), e.sel.1.min(n));
        let mut st = out.state.clone();
        st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(c))));
        st.store(ui.ctx(), id);
        out.response.request_focus();
        ui.ctx().request_repaint();
    } else if let Some(r) = out.cursor_range {
        e.sel = (r.secondary.ccursor.index, r.primary.ccursor.index);
    }
    if !out.response.has_focus() {
        out.response.request_focus();
    }
    if e.slash.is_some() && e.slash_items().is_none() {
        e.slash = None;
    }
    if let (Some(items), Some((_, cur))) = (e.slash_items(), e.slash) {
        let (at, pivot) = popup_at(ui, out.response.rect, items.len().min(8));
        egui::Area::new(egui::Id::new("slash")).order(egui::Order::Foreground).pivot(pivot).fixed_pos(at).show(ui.ctx(), |ui| {
            egui::Frame::new().fill(BG_ALT).stroke((1.0, GREY1)).inner_margin(4.0).show(ui, |ui| {
                ui.set_min_width(320.0);
                for (i, &it) in items.iter().enumerate().take(8) {
                    let sel = i == cur.min(items.len() - 1);
                    let (name, what) = typing::SLASH[it];
                    let r = egui::Frame::new().fill(if sel { SELECTION } else { Color32::TRANSPARENT }).inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(RichText::new(format!("/{name:<11}")).size(NOTE_TEXT - 1.0).color(if sel { FG_BRIGHT } else { FG })).selectable(false));
                            ui.add(egui::Label::new(RichText::new(what).size(NOTE_TEXT - 2.0).color(FG_DIM)).truncate().selectable(false));
                        });
                    });
                    if sel {
                        ui.painter().rect_filled(Rect::from_min_size(r.response.rect.min, Vec2::new(2.0, r.response.rect.height())), 0.0, RED_CURSOR);
                    }
                }
                ui.label(RichText::new("tab/enter pick · ctrl-n/p move · esc close").size(11.0).color(COMMENT));
            });
        });
    }

    // autocomplete: text between the last unclosed [[ and the cursor
    let caret = e.sel.1.min(n);
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
            let (at, pivot) = popup_at(ui, out.response.rect, a.items.len());
            egui::Area::new(egui::Id::new("ac")).order(egui::Order::Foreground).pivot(pivot).fixed_pos(at).show(ui.ctx(), |ui| {
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

fn flip(c: char) -> char {
    if c.is_uppercase() {
        c.to_lowercase().next().unwrap_or(c)
    } else {
        c.to_uppercase().next().unwrap_or(c)
    }
}

/// Bullets (with everything under them) as register items, root first.
fn clip(kb: &Kb, ids: &[String]) -> Vec<(String, Option<String>, LineJson)> {
    let mut out = vec![];
    for id in ids {
        for (i, k) in edit::subtree(kb, id).into_iter().enumerate() {
            let parent = if i == 0 { None } else { kb.q.get(&k).and_then(|x| x.get("parent")).and_then(|p| p.as_str()).map(str::to_owned) };
            let mut l = LineJson::of(&kb.line_of(&k));
            if kb.is_list_item(&k) {
                l.list = Some(true);
            }
            out.push((k.clone(), parent, l));
        }
    }
    out
}

/// Bullets whose new bullet (enter at the end, `o`) goes in as their first
/// child: >>> and >>A) cards (the children are the answer) and headings.
fn nests(kb: &Kb, id: &str) -> bool {
    matches!(kb.line_of(id).kind, rich::Kind::MultiLine | rich::Kind::Mcq | rich::Kind::Heading(_))
}

/// Where a popup of `n` rows goes: under the edit box, or above it when it
/// wouldn't fit below (so it never covers the line being typed).
fn popup_at(ui: &egui::Ui, r: Rect, n: usize) -> (Pos2, Align2) {
    let h = n as f32 * (NOTE_TEXT + 8.0) + 30.0;
    if r.bottom() + 4.0 + h > ui.ctx().screen_rect().bottom() - STATUS_H {
        (r.left_top() - Vec2::new(0.0, 4.0), Align2::LEFT_BOTTOM)
    } else {
        (r.left_bottom() + Vec2::new(0.0, 4.0), Align2::LEFT_TOP)
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
