//! Staged edits. Every change is an `Op` applied to the in-memory Kb straight
//! away (so the UI shows it) and recorded in a changeset; `:w` sends the
//! changeset to RemNote through the bridge (commit.rs). Undo applies the
//! inverse. New rems get temporary ids ("tmp-N") until the commit maps them.

use crate::kb::Kb;
use crate::rich::{to_rt, Kind, Line};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Op {
    Create { id: String, parent: String, pos: usize, line: LineJson },
    Update { id: String, before: LineJson, after: LineJson },
    /// `docs` = the whole subtree (root first), to restore on undo
    Delete { id: String, parent: String, pos: usize, docs: Vec<(String, Value)> },
    Move { id: String, from: (String, usize), to: (String, usize) },
}

/// A Line in a serialisable form (rich text JSON + kind), for the commit log.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LineJson {
    pub kind: String,
    pub front: Value,
    pub back: Value,
    /// make the rem a RemNote document (a page), e.g. a new week
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub doc: bool,
}

impl LineJson {
    pub fn of(l: &Line) -> LineJson {
        LineJson { kind: kind_name(l.kind), front: to_rt(&l.front), back: to_rt(&l.back), doc: false }
    }
    pub fn line(&self) -> Line {
        Line {
            kind: kind_from(&self.kind),
            front: crate::rich::parse_rt(Some(&self.front)),
            back: crate::rich::parse_rt(Some(&self.back)),
        }
    }
}

pub fn kind_name(k: Kind) -> String {
    match k {
        Kind::Heading(n) => format!("h{n}"),
        k => format!("{k:?}").to_lowercase(),
    }
}

pub fn kind_from(s: &str) -> Kind {
    match s {
        "concept" => Kind::Concept,
        "descriptor" => Kind::Descriptor,
        "basic" => Kind::Basic,
        "both" => Kind::Both,
        "multiline" => Kind::MultiLine,
        "mcq" => Kind::Mcq,
        "code" => Kind::Code,
        h if h.starts_with('h') => Kind::Heading(h[1..].parse().unwrap_or(1)),
        _ => Kind::Plain,
    }
}

#[derive(Default)]
pub struct Changes {
    pub ops: Vec<Op>,
    pub redo: Vec<Op>,
    pub next_tmp: u64,
    /// undo steps: how many ops each `u` takes back (a group is one step)
    steps: Vec<usize>,
    redo_steps: Vec<usize>,
    group: usize,
    group_fresh: bool,
}

impl Changes {
    pub fn tmp_id(&mut self) -> String {
        self.next_tmp += 1;
        format!("tmp-{}", self.next_tmp)
    }

    /// Ops applied until the matching `end` undo / redo as one step.
    pub fn begin(&mut self) {
        if self.group == 0 {
            self.group_fresh = true;
        }
        self.group += 1;
    }

    pub fn end(&mut self) {
        self.group = self.group.saturating_sub(1);
    }

    pub fn apply(&mut self, kb: &mut Kb, op: Op) {
        apply(kb, &op);
        self.ops.push(op);
        self.clear_redo();
        if self.group > 0 && !self.group_fresh && !self.steps.is_empty() {
            *self.steps.last_mut().unwrap() += 1;
        } else {
            self.steps.push(1);
            self.group_fresh = false;
        }
    }

    pub fn clear_redo(&mut self) {
        self.redo.clear();
        self.redo_steps.clear();
    }

    /// Take back exactly the last op, leaving no redo (a blank bullet dropped,
    /// a fresh bullet re-staged elsewhere).
    pub fn drop_last(&mut self, kb: &mut Kb) {
        let Some(op) = self.ops.pop() else { return };
        apply(kb, &inverse(&op));
        if let Some(n) = self.steps.last_mut() {
            *n -= 1;
            if *n == 0 {
                self.steps.pop();
            }
        }
        self.clear_redo();
    }

    /// After ops were replaced from outside (commit, restore): one step each.
    pub fn reset_steps(&mut self) {
        self.steps = vec![1; self.ops.len()];
        self.clear_redo();
    }

    pub fn undo(&mut self, kb: &mut Kb) -> Option<String> {
        let n = self.steps.pop().unwrap_or(1).min(self.ops.len());
        if n == 0 {
            return None;
        }
        let mut focus = None;
        for _ in 0..n {
            let op = self.ops.pop()?;
            apply(kb, &inverse(&op));
            focus = focus_of(&op);
            self.redo.push(op);
        }
        self.redo_steps.push(n);
        focus
    }

    pub fn redo(&mut self, kb: &mut Kb) -> Option<String> {
        let n = self.redo_steps.pop().unwrap_or(1).min(self.redo.len());
        if n == 0 {
            return None;
        }
        let mut focus = None;
        for _ in 0..n {
            let op = self.redo.pop()?;
            apply(kb, &op);
            focus = focus.or_else(|| focus_of(&op));
            self.ops.push(op);
        }
        self.steps.push(n);
        focus
    }
}

/// Staged ops saved to disk after every change, so a crash or a forced quit
/// doesn't lose them; restored on the next start against the same database.
#[derive(Serialize, Deserialize)]
pub struct Staged {
    pub db: std::path::PathBuf,
    pub next_tmp: u64,
    pub ops: Vec<Op>,
}

pub fn staged_path() -> std::path::PathBuf {
    crate::kb::home().join(".local/state/m3m0/staged.json")
}

impl Staged {
    pub fn save(&self) {
        let p = staged_path();
        if self.ops.is_empty() {
            let _ = std::fs::remove_file(p);
            return;
        }
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(s) = serde_json::to_string(self) {
            // write then rename: a crash mid-write never leaves half a file
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(tmp, p);
            }
        }
    }

    pub fn load() -> Option<Staged> {
        serde_json::from_str(&std::fs::read_to_string(staged_path()).ok()?).ok()
    }
}

fn focus_of(op: &Op) -> Option<String> {
    match op {
        Op::Create { id, .. } | Op::Update { id, .. } | Op::Move { id, .. } => Some(id.clone()),
        Op::Delete { parent, .. } => Some(parent.clone()),
    }
}

pub fn inverse(op: &Op) -> Op {
    match op {
        Op::Create { id, parent, pos, .. } => Op::Delete { id: id.clone(), parent: parent.clone(), pos: *pos, docs: vec![] },
        Op::Update { id, before, after } => Op::Update { id: id.clone(), before: after.clone(), after: before.clone() },
        Op::Delete { id, parent, pos, docs } => Op::Create {
            id: id.clone(),
            parent: parent.clone(),
            pos: *pos,
            // restore from the saved docs (handled in apply via the Restore path)
            line: LineJson { kind: "__restore__".into(), front: json!(docs), back: Value::Null, doc: false },
        },
        Op::Move { id, from, to } => Op::Move { id: id.clone(), from: to.clone(), to: from.clone() },
    }
}

fn apply(kb: &mut Kb, op: &Op) {
    match op {
        Op::Create { id, parent, pos, line } if line.kind == "__restore__" => {
            let docs: Vec<(String, Value)> = serde_json::from_value(line.front.clone()).unwrap_or_default();
            for (k, d) in &docs {
                kb.q.insert(k.clone(), d.clone());
            }
            // rebuild child lists for the restored subtree (root first, so parents exist)
            for (k, d) in &docs {
                let p = d.get("parent").and_then(Value::as_str).map(str::to_owned);
                if k == id {
                    let sibs = kb.kids.entry(Some(parent.clone())).or_default();
                    sibs.insert((*pos).min(sibs.len()), k.clone());
                } else {
                    kb.kids.entry(p).or_default().push(k.clone());
                }
            }
        }
        Op::Create { id, parent, pos, line } => {
            kb.q.insert(id.clone(), json!({ "_id": id, "parent": parent }));
            set_line(kb, id, &line.line());
            let sibs = kb.kids.entry(Some(parent.clone())).or_default();
            sibs.insert((*pos).min(sibs.len()), id.clone());
        }
        Op::Update { id, after, .. } => set_line(kb, id, &after.line()),
        Op::Delete { id, parent, .. } => {
            if let Some(s) = kb.kids.get_mut(&Some(parent.clone())) {
                s.retain(|k| k != id);
            }
            for k in subtree(kb, id) {
                kb.q.remove(&k);
                kb.kids.remove(&Some(k));
            }
        }
        Op::Move { id, from, to } => {
            if let Some(s) = kb.kids.get_mut(&Some(from.0.clone())) {
                s.retain(|k| k != id);
            }
            let sibs = kb.kids.entry(Some(to.0.clone())).or_default();
            sibs.insert(to.1.min(sibs.len()), id.clone());
            if let Some(d) = kb.q.get_mut(id) {
                d["parent"] = json!(to.0);
            }
        }
    }
}

/// `id` and everything under it, root first.
pub fn subtree(kb: &Kb, id: &str) -> Vec<String> {
    let mut out = vec![id.to_owned()];
    let mut i = 0;
    while i < out.len() {
        let kids = kb.children(Some(&out[i])).to_vec();
        out.extend(kids);
        i += 1;
    }
    out
}

/// Snapshot of a subtree's docs, for Delete (so undo can restore it).
pub fn snapshot(kb: &Kb, id: &str) -> Vec<(String, Value)> {
    subtree(kb, id).into_iter().filter_map(|k| kb.q.get(&k).map(|d| (k, d.clone()))).collect()
}

/// Write a Line into a rem's doc so `Kb::line_of` reads it back. The `m3m0`
/// field carries what only exists after a commit (multi-line / mcq / code).
pub fn set_line(kb: &mut Kb, id: &str, l: &Line) {
    let Some(d) = kb.q.get_mut(id) else { return };
    let o = d.as_object_mut().unwrap();
    o.insert("key".into(), to_rt(&l.front));
    if l.back.is_empty() && matches!(l.kind, Kind::Plain | Kind::Heading(_) | Kind::MultiLine | Kind::Mcq | Kind::Code) {
        o.remove("value");
    } else {
        o.insert("value".into(), to_rt(&l.back));
    }
    match l.kind {
        Kind::Concept => {
            o.insert("type".into(), json!(1));
        }
        Kind::Descriptor => {
            o.insert("type".into(), json!(2));
        }
        _ => {
            o.remove("type");
        }
    }
    o.insert("enableBackSR".into(), json!(l.kind == Kind::Both));
    match l.kind {
        Kind::Heading(n) => {
            o.insert("ps".into(), json!({ "r_s": { "v": { "v": [format!("H{n}")], "s": format!("H{n}") } } }));
        }
        _ => {
            if let Some(ps) = o.get_mut("ps").and_then(Value::as_object_mut) {
                ps.remove("r_s");
            }
        }
    }
    o.insert("m3m0".into(), json!(kind_name(l.kind)));
}
