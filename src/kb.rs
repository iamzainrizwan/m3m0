//! Read model of the local RemNote knowledge base.
//!
//! Port of uni-git/_tools/remnote.py's KB. Reads always go through a snapshot
//! taken with SQLite's backup API, so they are safe while RemNote is running.
//! Storage notes (RemNote 1.28.50): quanta(_id, doc JSON) is one row per rem;
//! doc.key / doc.value are rich text, doc.parent + doc.f give the tree order;
//! cards(_id, doc) is one row per card direction.

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const CARD_ITEM: &str = "SrPWzY0xLzu625Ted";

/// A RemNote database found on disk, for the :db picker.
#[derive(Clone)]
pub struct DbCandidate {
    pub path: PathBuf,
    pub bytes: u64,
    pub modified: Option<std::time::SystemTime>,
}

/// Every ~/remnote/*/remnote.db, largest first. RemNote names the folders
/// remnote-<knowledge base id>; browser/ and remnote-browser/ hold other copies.
/// If RemNote ever moves its storage, look at how the app's JS opens it
/// (asar under the install dir) and add the new location here.
pub fn find_dbs() -> Vec<DbCandidate> {
    let mut v = vec![];
    if let Ok(rd) = std::fs::read_dir(home().join("remnote")) {
        for e in rd.flatten() {
            let p = e.path().join("remnote.db");
            if let Ok(m) = std::fs::metadata(&p) {
                v.push(DbCandidate { path: p, bytes: m.len(), modified: m.modified().ok() });
            }
        }
    }
    v.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    v
}

/// $M3M0_DB, then the configured path, then the largest database found.
pub fn resolve_db(configured: Option<&Path>) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("M3M0_DB") {
        return Some(p.into());
    }
    if let Some(p) = configured.filter(|p| p.exists()) {
        return Some(p.to_owned());
    }
    find_dbs().into_iter().next().map(|c| c.path)
}

pub fn home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| "/".into())
}

pub struct Kb {
    pub q: HashMap<String, Value>,
    pub kids: HashMap<Option<String>, Vec<String>>,
    #[allow(dead_code)] // card directions, for the flashcard views
    pub dirs: HashMap<String, Vec<String>>,
    pub cards: usize,
    pub db_bytes: u64,
    /// raw card docs (scheduling: n = next due, ml = memory level, h = history)
    pub card_docs: Vec<Value>,
    /// alias id -> its rich text. RemNote keeps aliases on the target rem
    /// (`doc.al = [{aliasId, alias}]`), not as rems of their own.
    pub aliases: HashMap<String, Value>,
}

/// Consistent in-memory copy of the live DB (WAL included), never writing to it.
fn snapshot(db: &Path) -> rusqlite::Result<Connection> {
    let src = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut dst = Connection::open_in_memory()?;
    {
        let b = rusqlite::backup::Backup::new(&src, &mut dst)?;
        b.run_to_completion(256, std::time::Duration::ZERO, None)?;
    }
    Ok(dst)
}

impl Kb {
    pub fn load(db: &Path) -> Result<Kb, String> {
        let conn = snapshot(db).map_err(|e| format!("{}: {e}", db.display()))?;
        let mut q = HashMap::new();
        let mut st = conn.prepare("select _id, doc from quanta").map_err(|e| e.to_string())?;
        let rows = st
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?;
        for r in rows.flatten() {
            if let Ok(v) = serde_json::from_str::<Value>(&r.1) {
                q.insert(r.0, v);
            }
        }
        let mut alias_rt = HashMap::new();
        for d in q.values() {
            for a in d.get("al").and_then(Value::as_array).into_iter().flatten() {
                if let (Some(id), Some(t)) = (a.get("aliasId").and_then(Value::as_str), a.get("alias")) {
                    alias_rt.insert(id.to_owned(), t.clone());
                }
            }
        }
        // in case an alias also exists as a rem: never show it as a bullet
        let mut aliases = std::collections::HashSet::new();
        for d in q.values() {
            for side in ["key", "value"] {
                for p in d.get(side).and_then(Value::as_array).into_iter().flatten() {
                    if let Some(a) = p.get("aliasId").and_then(Value::as_str) {
                        aliases.insert(a.to_owned());
                    }
                }
            }
        }
        let mut kids: HashMap<Option<String>, Vec<String>> = HashMap::new();
        for (i, d) in &q {
            if aliases.contains(i) {
                continue;
            }
            let p = d.get("parent").and_then(Value::as_str).map(str::to_owned);
            kids.entry(p).or_default().push(i.clone());
        }
        let sk = |i: &String| q[i].get("f").and_then(Value::as_str).unwrap_or("").to_owned();
        for v in kids.values_mut() {
            v.sort_by_key(sk);
        }
        let mut dirs: HashMap<String, Vec<String>> = HashMap::new();
        let mut cards = 0;
        let mut card_docs = vec![];
        let mut st = conn.prepare("select doc from cards").map_err(|e| e.to_string())?;
        for d in st.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?.flatten() {
            cards += 1;
            if let Ok(d) = serde_json::from_str::<Value>(&d) {
                let r = d.get("rId").and_then(Value::as_str).unwrap_or("").to_owned();
                let k = d.get("k").and_then(Value::as_str).unwrap_or("");
                dirs.entry(r).or_default().push(k.rsplit('.').next().unwrap_or("").to_owned());
                card_docs.push(d);
            }
        }
        drop(st);
        let db_bytes = std::fs::metadata(db).map(|m| m.len()).unwrap_or(0);
        Ok(Kb { q, kids, dirs, cards, db_bytes, card_docs, aliases: alias_rt })
    }

    pub fn children(&self, i: Option<&str>) -> &[String] {
        self.kids.get(&i.map(str::to_owned)).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn text(&self, rt: Option<&Value>, md: bool) -> String {
        let Some(rt) = rt else { return String::new() };
        if let Some(s) = rt.as_str() {
            return s.to_owned();
        }
        let mut out = String::new();
        for p in rt.as_array().into_iter().flatten() {
            if let Some(s) = p.as_str() {
                out.push_str(s);
                continue;
            }
            let t = p.get("text").and_then(Value::as_str).unwrap_or("");
            match p.get("i").and_then(Value::as_str) {
                Some("x") => out.push_str(&format!("${t}$")),
                Some("q") => {
                    // through an alias: the alias's text (its rem, or a not-yet-created one)
                    let alias = p.get("aliasText").and_then(Value::as_str).map(str::to_owned).or_else(|| {
                        let a = p.get("aliasId").and_then(Value::as_str)?;
                        let rt = self.q.get(a).and_then(|d| d.get("key")).or_else(|| self.aliases.get(a))?;
                        Some(self.text(Some(rt), md)).filter(|t| !t.trim().is_empty())
                    });
                    let id = p.get("_id").and_then(Value::as_str).unwrap_or("");
                    out.push_str("[[");
                    out.push_str(&alias.unwrap_or_else(|| self.text(self.q.get(id).and_then(|d| d.get("key")), md)));
                    out.push_str("]]");
                }
                Some("i") => out.push_str("[image]"),
                Some("m") if md && !t.trim().is_empty() => {
                    let mut s = t.to_owned();
                    if p.get("cId").is_some() {
                        s = format!("{{{{{s}}}}}");
                    }
                    if p.get("q").is_some() {
                        s = format!("`{s}`");
                    }
                    if p.get("b").is_some() {
                        s = format!("**{s}**");
                    }
                    if p.get("l").is_some() {
                        s = format!("*{s}*");
                    }
                    out.push_str(&s);
                }
                _ => out.push_str(t),
            }
        }
        out
    }

    pub fn name(&self, i: &str) -> String {
        let rt = self.q.get(i).and_then(|d| d.get("key")).or_else(|| self.aliases.get(i));
        self.text(rt, false).trim().to_owned()
    }

    /// The id of `rem`'s alias reading `text`, straight from a fresh snapshot of
    /// the DB (the plugin SDK hands back aliases without their ids).
    pub fn alias_id(db: &Path, rem: &str, text: &str) -> Option<String> {
        let conn = snapshot(db).ok()?;
        let doc: String = conn.query_row("select doc from quanta where _id = ?1", [rem], |r| r.get(0)).ok()?;
        let d: Value = serde_json::from_str(&doc).ok()?;
        let plain = |v: &Value| -> String {
            v.as_array().into_iter().flatten().map(|p| p.as_str().map(str::to_owned).unwrap_or_else(|| p.get("text").and_then(Value::as_str).unwrap_or("").to_owned())).collect()
        };
        d.get("al")?.as_array()?.iter().find(|a| a.get("alias").is_some_and(|t| plain(t) == text))?.get("aliasId")?.as_str().map(str::to_owned)
    }

    pub fn heading(&self, i: &str) -> Option<u8> {
        let s = self.q.get(i)?.pointer("/ps/r_s/v/s")?.as_str()?;
        s.strip_prefix('H')?.parse().ok()
    }

    /// One outline line in the deck syntax (:: ;; >> <> >>>), without the heading marker.
    pub fn line(&self, i: &str) -> String {
        let d = &self.q[i];
        let key = self.text(d.get("key"), true);
        let val = self.text(d.get("value"), true);
        // card items: old notes tag children with the Card Item power-up in tp;
        // the plugin SDK's setIsCardItem writes apu.w (MultiLineCard) instead
        let multi = self.children(Some(i)).iter().any(|k| {
            let d = &self.q[k];
            d.get("tp").and_then(|t| t.get(CARD_ITEM)).is_some()
                || d.pointer("/apu/w/v").and_then(Value::as_bool) == Some(true)
        });
        match d.get("type").and_then(Value::as_i64) {
            Some(1) => return format!("{key} :: {val}"),
            Some(2) => return format!("{key} ;; {val}"),
            _ => {}
        }
        if multi {
            return format!("{key} >>> {val}").trim_end().to_owned();
        }
        if !val.is_empty() || d.get("value").is_some_and(|v| !v.is_null()) {
            let arrow = if d.get("enableBackSR").is_some_and(|v| v.as_bool() == Some(true)) { "<>" } else { ">>" };
            return format!("{key} {arrow} {val}");
        }
        key
    }

    /// Top-level rems that hold "Week ..." children: the uni modules.
    pub fn modules(&self) -> Vec<String> {
        self.children(None)
            .iter()
            .filter(|r| self.children(Some(r)).iter().any(|k| self.name(k).starts_with("Week")))
            .cloned()
            .collect()
    }
}

// --- lines (rich model) -----------------------------------------------------------

impl Kb {
    pub fn is_multiline(&self, i: &str) -> bool {
        self.children(Some(i)).iter().any(|k| {
            let d = &self.q[k];
            d.get("tp").and_then(|t| t.get(CARD_ITEM)).is_some() || d.pointer("/apu/w/v").and_then(Value::as_bool) == Some(true)
        })
    }

    pub fn line_of(&self, i: &str) -> crate::rich::Line {
        use crate::rich::{parse_rt, Kind, Line};
        let d = &self.q[i];
        let front = parse_rt(d.get("key"));
        let back = parse_rt(d.get("value"));
        let has_value = d.get("value").is_some_and(|v| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()));
        let staged = d.get("m3m0").and_then(Value::as_str).map(crate::edit::kind_from);
        let kind = if let Some(k) = staged {
            k
        } else if d.pointer("/apu/cd/v").and_then(Value::as_bool) == Some(true) {
            Kind::Code
        } else if let Some(h) = self.heading(i) {
            Kind::Heading(h.max(1))
        } else {
            match d.get("type").and_then(Value::as_i64) {
                Some(1) => Kind::Concept,
                Some(2) => Kind::Descriptor,
                _ if self.is_multiline(i) => Kind::MultiLine,
                _ if has_value && d.get("enableBackSR").and_then(Value::as_bool) == Some(true) => Kind::Both,
                _ if has_value => Kind::Basic,
                _ => Kind::Plain,
            }
        };
        Line { kind, front, back }
    }

    /// Rem id for a [[name]]: exact name match, preferring rems under `near`'s module.
    pub fn resolve(&self, name: &str, near: Option<&str>) -> Option<String> {
        let hits: Vec<&String> = self.q.keys().filter(|k| self.name(k) == name && !self.is_hidden(k)).collect();
        let module = near.and_then(|n| self.module_of(n));
        hits.iter().find(|h| module.is_some() && self.module_of(h) == module).or(hits.first()).map(|s| (*s).clone())
    }

    /// Not in the tree (alias rems): not a [[ref]] target by name either.
    pub fn is_hidden(&self, i: &str) -> bool {
        match self.q.get(i).and_then(|d| d.get("parent")).and_then(Value::as_str) {
            Some(p) => !self.children(Some(p)).iter().any(|k| k == i),
            None => false,
        }
    }

    pub fn module_of(&self, i: &str) -> Option<String> {
        let mut cur = i.to_owned();
        loop {
            let p = self.q.get(&cur)?.get("parent").and_then(Value::as_str)?.to_owned();
            if self.q.get(&p).is_some_and(|d| d.get("parent").is_none_or(Value::is_null)) {
                return Some(p);
            }
            cur = p;
        }
    }
}
