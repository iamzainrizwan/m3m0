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
}

impl LineJson {
    pub fn of(l: &Line) -> LineJson {
        LineJson { kind: kind_name(l.kind), front: to_rt(&l.front), back: to_rt(&l.back) }
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
    next_tmp: u64,
}

impl Changes {
    pub fn tmp_id(&mut self) -> String {
        self.next_tmp += 1;
        format!("tmp-{}", self.next_tmp)
    }

    pub fn apply(&mut self, kb: &mut Kb, op: Op) {
        apply(kb, &op);
        self.ops.push(op);
        self.redo.clear();
    }

    pub fn undo(&mut self, kb: &mut Kb) -> Option<String> {
        let op = self.ops.pop()?;
        apply(kb, &inverse(&op));
        let focus = focus_of(&op);
        self.redo.push(op);
        focus
    }

    pub fn redo(&mut self, kb: &mut Kb) -> Option<String> {
        let op = self.redo.pop()?;
        apply(kb, &op);
        let focus = focus_of(&op);
        self.ops.push(op);
        focus
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
            line: LineJson { kind: "__restore__".into(), front: json!(docs), back: Value::Null },
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
