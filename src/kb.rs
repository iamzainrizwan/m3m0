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
        let mut kids: HashMap<Option<String>, Vec<String>> = HashMap::new();
        for (i, d) in &q {
            let p = d.get("parent").and_then(Value::as_str).map(str::to_owned);
            kids.entry(p).or_default().push(i.clone());
        }
        let sk = |i: &String| q[i].get("f").and_then(Value::as_str).unwrap_or("").to_owned();
        for v in kids.values_mut() {
            v.sort_by_key(sk);
        }
        let mut dirs: HashMap<String, Vec<String>> = HashMap::new();
        let mut cards = 0;
        let mut st = conn.prepare("select doc from cards").map_err(|e| e.to_string())?;
        for d in st.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?.flatten() {
            cards += 1;
            if let Ok(d) = serde_json::from_str::<Value>(&d) {
                let r = d.get("rId").and_then(Value::as_str).unwrap_or("").to_owned();
                let k = d.get("k").and_then(Value::as_str).unwrap_or("");
                dirs.entry(r).or_default().push(k.rsplit('.').next().unwrap_or("").to_owned());
            }
        }
        drop(st);
        let db_bytes = std::fs::metadata(db).map(|m| m.len()).unwrap_or(0);
        Ok(Kb { q, kids, dirs, cards, db_bytes })
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
                    let id = p.get("_id").and_then(Value::as_str).unwrap_or("");
                    out.push_str("[[");
                    out.push_str(&self.text(self.q.get(id).and_then(|d| d.get("key")), md));
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
        self.text(self.q.get(i).and_then(|d| d.get("key")), false).trim().to_owned()
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
