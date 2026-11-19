//! Claude's flashcard decks (uni-git/<module>/revision/week-NN-flashcards.md):
//! parse, match against Zain's own cards, and remember explicit links.
//!
//! Deck format: a markdown outline in RemNote deck syntax; `$$x$$` is inline
//! LaTeX (converted to the `$x$` our parser uses) and `**...**` items are
//! section titles. Links (rem id -> Claude card front) live in
//! ~/.local/share/m3m0/links.json, keyed by the card's normalised front text so
//! they survive reordering of either side.

use crate::kb::Kb;
use crate::rich::{self, Kind, Line};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct DeckCard {
    pub section: String,
    #[allow(dead_code)] // kept for debugging / future "copy raw" action
    pub text: String, // deck syntax with single-$ latex
    pub line: Line,
    pub key: String, // normalised front, the link key
    pub items: Vec<String>,
}

pub fn deck_files(module: &str, week: u32) -> Vec<PathBuf> {
    let rev = crate::pdf::uni_dir().join(module.to_lowercase()).join("revision");
    let file = rev.join(format!("week-{week:02}-flashcards.md"));
    if file.exists() {
        return vec![file];
    }
    let dir = rev.join(format!("week-{week:02}"));
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md") && p.file_name().is_some_and(|n| n.to_string_lossy().contains("flashcard")))
        .collect();
    v.sort();
    v
}

fn strip_bold(s: &str) -> Option<String> {
    let t = s.trim();
    (t.starts_with("**") && t.ends_with("**") && t.len() > 4).then(|| t[2..t.len() - 2].to_owned())
}

pub fn parse(path: &Path) -> Vec<DeckCard> {
    let Ok(src) = std::fs::read_to_string(path) else { return vec![] };
    let mut out: Vec<DeckCard> = vec![];
    let mut section = String::new();
    let mut parent_card: Option<(usize, usize)> = None; // (indent, index in out) of a >>> card
    for raw in src.lines() {
        let indent = raw.len() - raw.trim_start().len();
        let Some(body) = raw.trim_start().strip_prefix("- ") else { continue };
        let body = body.replace("$$", "$");
        if let Some(h) = strip_bold(&body) {
            section = h;
            parent_card = None;
            continue;
        }
        if let Some((pi, idx)) = parent_card {
            if indent > pi {
                out[idx].items.push(body.clone());
                continue;
            }
            parent_card = None;
        }
        let line = rich::text_to_line(&body, &|_| None, &rich::Side::default(), &mut || "c".into());
        let is_card = !matches!(line.kind, Kind::Plain | Kind::Heading(_)) || body.contains("{{");
        if !is_card {
            continue;
        }
        if line.kind == Kind::MultiLine {
            parent_card = Some((indent, out.len()));
        }
        let key = norm(&rich::plain(&line.front, &|_| String::new()));
        out.push(DeckCard { section: section.clone(), text: body, line, key, items: vec![] });
    }
    out
}

/// Lowercased words (letters/digits, latex commands kept as words).
pub fn norm(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() || c == '\\' {
            out.extend(c.to_lowercase());
        } else {
            out.push(' ');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn words(s: &str) -> HashSet<String> {
    norm(s).split(' ').filter(|w| w.len() > 1 || w.chars().all(|c| c.is_alphabetic())).map(str::to_owned).collect()
}

/// Similarity of two card fronts (Jaccard over words, plus a bonus for the back).
pub fn similarity(a_front: &str, a_back: &str, b_front: &str, b_back: &str) -> f32 {
    let j = |x: &HashSet<String>, y: &HashSet<String>| {
        if x.is_empty() || y.is_empty() {
            return 0.0;
        }
        x.intersection(y).count() as f32 / x.union(y).count() as f32
    };
    let f = j(&words(a_front), &words(b_front));
    let b = j(&words(a_back), &words(b_back));
    f * 0.75 + b * 0.25
}

/// Zain's cards in a week: every rem under it that makes a flashcard.
pub fn my_cards(kb: &Kb, week: &str) -> Vec<String> {
    let mut out = vec![];
    let mut stack = vec![week.to_owned()];
    while let Some(i) = stack.pop() {
        for k in kb.children(Some(&i)).iter().rev() {
            stack.push(k.clone());
        }
        if i == week {
            continue;
        }
        let l = kb.line_of(&i);
        let cloze = l.front.iter().chain(&l.back).any(|s| matches!(s, rich::Span::Text { cloze: Some(_), .. } | rich::Span::Latex { cloze: Some(_), .. }));
        if !matches!(l.kind, Kind::Plain | Kind::Heading(_) | Kind::Code) || cloze {
            out.push(i);
        }
    }
    // stack order above is depth-first; keep document order
    out
}

pub fn links_path() -> PathBuf {
    crate::kb::home().join(".local/share/m3m0/links.json")
}

/// rem id -> Claude card key
pub fn load_links() -> HashMap<String, String> {
    std::fs::read_to_string(links_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save_links(l: &HashMap<String, String>) {
    let p = links_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(s) = serde_json::to_string_pretty(l) {
        let _ = std::fs::write(p, s);
    }
}

pub struct Match {
    /// for each of my cards: index of the Claude card it pairs with, and whether it's an explicit link
    pub mine: Vec<(String, Option<(usize, bool)>)>,
    /// Claude cards nobody matched
    pub theirs_free: HashSet<usize>,
}

/// Pair my cards with Claude's: explicit links first, then best similarity above a threshold.
pub fn pair(kb: &Kb, mine: &[String], theirs: &[DeckCard], links: &HashMap<String, String>) -> Match {
    let name = |id: &str| kb.name(id);
    let mut taken: HashSet<usize> = HashSet::new();
    let mut res: Vec<(String, Option<(usize, bool)>)> = mine.iter().map(|m| (m.clone(), None)).collect();
    for (m, slot) in res.iter_mut() {
        if let Some(k) = links.get(m) {
            if let Some(i) = theirs.iter().position(|c| &c.key == k) {
                *slot = Some((i, true));
                taken.insert(i);
            }
        }
    }
    let mut cands: Vec<(f32, usize, usize)> = vec![];
    for (mi, (m, slot)) in res.iter().enumerate() {
        if slot.is_some() {
            continue;
        }
        let l = kb.line_of(m);
        let (mf, mb) = (rich::plain(&l.front, &name), rich::plain(&l.back, &name));
        for (ti, t) in theirs.iter().enumerate() {
            if taken.contains(&ti) {
                continue;
            }
            let s = similarity(&mf, &mb, &rich::plain(&t.line.front, &|_| String::new()), &rich::plain(&t.line.back, &|_| String::new()));
            if s >= 0.35 {
                cands.push((s, mi, ti));
            }
        }
    }
    cands.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (_, mi, ti) in cands {
        if res[mi].1.is_none() && !taken.contains(&ti) {
            res[mi].1 = Some((ti, false));
            taken.insert(ti);
        }
    }
    let theirs_free = (0..theirs.len()).filter(|i| !taken.contains(i)).collect();
    Match { mine: res, theirs_free }
}
