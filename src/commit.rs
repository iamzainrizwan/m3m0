//! `:w`: send staged ops to RemNote through the bridge, in order, mapping
//! temporary ids to the real ones RemNote returns. Stops at the first failure
//! (the rest stay staged). Every commit is appended to the commit log
//! (~/.local/share/m3m0/commits.jsonl) so `:log` can revert it later.

use crate::bridge::Bridge;
use crate::edit::{LineJson, Op};
use crate::rich::Kind;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct Outcome {
    pub done: usize,
    pub idmap: HashMap<String, String>,
    pub err: Option<String>,
}

/// Things RemNote's plugin API can't do yet; refuse before sending anything.
pub fn validate(ops: &[Op]) -> Result<(), String> {
    for op in ops {
        let (before, after) = match op {
            Op::Create { line, .. } => (None, Some(line)),
            Op::Update { before, after, .. } => (Some(before), Some(after)),
            _ => (None, None),
        };
        if let Some(a) = after {
            let was = |k: &str| before.is_some_and(|b| b.kind == k);
            if a.kind == "mcq" && !was("mcq") {
                return Err("multiple-choice cards can't be written yet (need a sample card to learn RemNote's encoding)".into());
            }
            if a.kind == "code" && !was("code") {
                return Err("turning a rem into a code block isn't supported by RemNote's plugin API; edit code blocks in place".into());
            }
        }
    }
    Ok(())
}

pub fn map_ids(v: &Value, m: &HashMap<String, String>) -> Value {
    match v {
        Value::String(s) => Value::String(m.get(s).cloned().unwrap_or_else(|| s.clone())),
        Value::Array(a) => Value::Array(a.iter().map(|x| map_ids(x, m)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), map_ids(x, m))).collect()),
        x => x.clone(),
    }
}

/// set-fields for a line (docs/bridge.md).
fn fields(l: &LineJson, m: &HashMap<String, String>) -> Value {
    let kind = crate::edit::kind_from(&l.kind);
    let mut f = json!({ "text": map_ids(&l.front, m) });
    let is_card = matches!(kind, Kind::Concept | Kind::Descriptor | Kind::Basic | Kind::Both);
    // the SDK's setBackText needs an argument: omit "back" when there's none
    // (clearing a former card's back is done by the caller with [])
    if is_card || l.back.as_array().is_some_and(|a| !a.is_empty()) {
        f["back"] = map_ids(&l.back, m);
    }
    f["remType"] = match kind {
        Kind::Concept => json!(1),
        Kind::Descriptor => json!(2),
        _ => json!("DEFAULT_TYPE"), // the SDK's SetRemType.DEFAULT_TYPE is a string enum value
    };
    f["heading"] = match kind {
        Kind::Heading(n) => json!(format!("H{}", n.clamp(1, 3))),
        _ => Value::Null,
    };
    match kind {
        Kind::Basic | Kind::MultiLine => f["practice"] = json!("forward"),
        Kind::Both => f["practice"] = json!("both"),
        _ => {}
    }
    if kind == Kind::Code {
        // code blocks: only the text changes
        f = json!({ "text": map_ids(&l.front, m) });
    }
    f
}

/// Refs typed as `[[name|alias]]` carry `aliasText`: create the alias (or reuse
/// an equal one, getOrCreateAliasWithText) and point the ref at it by `aliasId`.
fn resolve_aliases(bridge: &Bridge, v: &mut Value, cache: &mut HashMap<(String, String), String>) -> Result<(), String> {
    match v {
        Value::Array(a) => a.iter_mut().try_for_each(|x| resolve_aliases(bridge, x, cache)),
        Value::Object(o) if o.get("i").and_then(Value::as_str) == Some("q") => {
            let (Some(rem), Some(text)) = (o.get("_id").and_then(Value::as_str), o.get("aliasText").and_then(Value::as_str)) else { return Ok(()) };
            let key = (rem.to_owned(), text.to_owned());
            let id = match cache.get(&key) {
                Some(id) => id.clone(),
                None => {
                    let res = bridge.call("alias", json!({ "rem": rem, "text": [text] }))?;
                    let id = res["id"].as_str().ok_or("alias returned no id")?.to_owned();
                    cache.insert(key, id.clone());
                    id
                }
            };
            o.remove("aliasText");
            o.insert("aliasId".into(), json!(id));
            Ok(())
        }
        _ => Ok(()),
    }
}

/// create / update with set-fields, creating any typed aliases first.
fn send(bridge: &Bridge, op: &str, mut f: Value, cache: &mut HashMap<(String, String), String>) -> Result<Value, String> {
    for k in ["text", "back"] {
        if let Some(v) = f.get_mut(k) {
            resolve_aliases(bridge, v, cache)?;
        }
    }
    bridge.call(op, f)
}

/// Run the ops. `children` gives a rem's children (for marking >>> items).
pub fn run(bridge: &Bridge, ops: &[Op], children: &HashMap<String, Vec<String>>) -> Outcome {
    let mut idmap: HashMap<String, String> = HashMap::new();
    let mut aliases = HashMap::new();
    let real = |m: &HashMap<String, String>, id: &str| m.get(id).cloned().unwrap_or_else(|| id.to_owned());
    for (i, op) in ops.iter().enumerate() {
        let r: Result<(), String> = (|| {
            match op {
                Op::Create { id, parent, pos, line } if line.kind == "__restore__" => {
                    // re-create a deleted subtree (revert of a delete): root first
                    let docs: Vec<(String, Value)> = serde_json::from_value(line.front.clone()).unwrap_or_default();
                    for (k, (old, d)) in docs.iter().enumerate() {
                        let l = line_of_doc(d);
                        let mut f = fields(&l, &idmap);
                        if k == 0 {
                            f["parent"] = json!(real(&idmap, parent));
                            f["position"] = json!(pos);
                        } else {
                            let p = d.get("parent").and_then(Value::as_str).unwrap_or_default();
                            f["parent"] = json!(real(&idmap, p));
                        }
                        let card_item = d.pointer("/tp/SrPWzY0xLzu625Ted").is_some() || d.pointer("/apu/w/v") == Some(&json!(true));
                        if card_item {
                            f["cardItem"] = json!(true);
                        }
                        let res = send(bridge, "create", f, &mut aliases)?;
                        let new = res["id"].as_str().ok_or("create returned no id")?.to_owned();
                        idmap.insert(old.clone(), new);
                        if k == 0 {
                            idmap.insert(id.clone(), idmap[old].clone());
                        }
                    }
                }
                Op::Create { id, parent, pos, line } => {
                    let mut f = fields(line, &idmap);
                    f["parent"] = json!(real(&idmap, parent));
                    f["position"] = json!(pos);
                    let res = send(bridge, "create", f, &mut aliases)?;
                    let new = res["id"].as_str().ok_or("create returned no id")?.to_owned();
                    idmap.insert(id.clone(), new);
                }
                Op::Update { id, before, after } => {
                    let mut f = fields(after, &idmap);
                    let had_back = before.back.as_array().is_some_and(|a| !a.is_empty());
                    if had_back && f.get("back").is_none() {
                        f["back"] = json!([]);
                    }
                    f["rem"] = json!(real(&idmap, id));
                    send(bridge, "update", f, &mut aliases)?;
                    if after.kind == "multiline" && before.kind != "multiline" {
                        for c in children.get(id).into_iter().flatten() {
                            bridge.call("update", json!({ "rem": real(&idmap, c), "cardItem": true }))?;
                        }
                    }
                }
                Op::Delete { id, .. } => {
                    bridge.call("delete", json!({ "rem": real(&idmap, id) }))?;
                }
                Op::Move { id, to, .. } => {
                    bridge.call("update", json!({ "rem": real(&idmap, id), "parent": real(&idmap, &to.0), "position": to.1 }))?;
                }
            }
            Ok(())
        })();
        if let Err(e) = r {
            return Outcome { done: i, idmap, err: Some(e) };
        }
    }
    Outcome { done: ops.len(), idmap, err: None }
}

/// Replace temporary ids everywhere in an op.
pub fn rename_op(op: &Op, m: &HashMap<String, String>) -> Op {
    let v = serde_json::to_value(op).unwrap();
    serde_json::from_value(map_ids(&v, m)).unwrap()
}

pub fn log_path() -> std::path::PathBuf {
    crate::kb::home().join(".local/share/m3m0/commits.jsonl")
}

pub fn append_log(ops: &[Op]) {
    let p = log_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let line = json!({ "time": t, "ops": ops }).to_string();
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "{line}");
    }
}

pub struct Entry {
    pub time: u64,
    pub ops: Vec<Op>,
}

pub fn read_log() -> Vec<Entry> {
    let Ok(s) = std::fs::read_to_string(log_path()) else { return vec![] };
    s.lines()
        .filter_map(|l| {
            let v: Value = serde_json::from_str(l).ok()?;
            Some(Entry { time: v["time"].as_u64()?, ops: serde_json::from_value(v["ops"].clone()).ok()? })
        })
        .rev()
        .collect()
}

/// One line per op, for the diff and log views.
pub fn describe(op: &Op, name: &dyn Fn(&str) -> String) -> (char, String) {
    let txt = |l: &LineJson| {
        let line = l.line();
        let mut side = crate::rich::Side::default();
        crate::rich::line_to_text(&line, name, &mut side)
    };
    match op {
        Op::Create { parent, line, .. } => ('+', format!("{}   (under {})", txt(line), name(parent))),
        Op::Update { before, after, .. } => ('~', format!("{}  →  {}", txt(before), txt(after))),
        Op::Delete { id, docs, .. } => {
            let first = docs.first().map(|(_, d)| crate::rich::plain(&crate::rich::parse_rt(d.get("key")), name)).unwrap_or_else(|| name(id));
            ('-', if docs.len() > 1 { format!("{first}   (+{} below)", docs.len() - 1) } else { first })
        }
        Op::Move { id, to, .. } => ('>', format!("{}   → under {} at {}", name(id), name(&to.0), to.1)),
    }
}

/// A rem's line straight from its raw doc (for restoring deleted subtrees).
fn line_of_doc(d: &Value) -> LineJson {
    let has_value = d.get("value").is_some_and(|v| !v.is_null());
    let heading = d.pointer("/ps/r_s/v/s").and_then(Value::as_str).and_then(|s| s.strip_prefix('H')).and_then(|n| n.parse::<u8>().ok());
    let kind = if let Some(h) = heading {
        format!("h{}", h.max(1))
    } else {
        match d.get("type").and_then(Value::as_i64) {
            Some(1) => "concept".into(),
            Some(2) => "descriptor".into(),
            _ if has_value && d.get("enableBackSR") == Some(&json!(true)) => "both".into(),
            _ if has_value => "basic".into(),
            _ => "plain".into(),
        }
    };
    LineJson { kind, front: d.get("key").cloned().unwrap_or(json!([])), back: d.get("value").cloned().unwrap_or(json!([])) }
}
