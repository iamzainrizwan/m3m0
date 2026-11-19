//! Rich text and rem lines.
//!
//! RemNote rich text (docs/remnote-storage.md) <-> `Span`s <-> the editable
//! deck syntax a bullet shows while you type in it:
//!
//!   `# heading`   `front :: back`   `front ;; back`   `front >> back`
//!   `front <> back`   `front >>>` (children are the items)   `front >>A)` (mcq)
//!   inline: `**bold**` `*italic*` `` `code` `` `$latex$` `{{cloze}}` `[[ref]]`
//!   anything we don't model (images, plugin objects) shows as `![image](N)` and
//!   comes back verbatim, so editing a line never loses data.

use serde_json::{json, Map, Value};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Span {
    Text { t: String, b: bool, i: bool, u: bool, code: bool, cloze: Option<String>, extra: Map<String, Value> },
    Latex { tex: String, cloze: Option<String>, extra: Map<String, Value> },
    Ref { id: String, cloze: Option<String>, extra: Map<String, Value> },
    /// images and anything else: kept as the original JSON
    Opaque(Value),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Heading(u8),
    Concept,
    Descriptor,
    Basic,
    Both,
    MultiLine,
    Mcq,
    /// code block rem (apu.cd); text is raw, no markup or delimiters
    Code,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub kind: Kind,
    pub front: Vec<Span>,
    pub back: Vec<Span>,
}

const KNOWN_M: [&str; 8] = ["i", "text", "b", "l", "u", "q", "cId", "cloze"];

fn cloze_of(o: &Map<String, Value>) -> Option<String> {
    o.get("cId").or_else(|| o.get("cloze")).and_then(Value::as_str).map(str::to_owned)
}

fn extra(o: &Map<String, Value>, known: &[&str]) -> Map<String, Value> {
    o.iter().filter(|(k, _)| !known.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect()
}

pub fn parse_rt(rt: Option<&Value>) -> Vec<Span> {
    let mut out = vec![];
    let Some(rt) = rt else { return out };
    if let Some(s) = rt.as_str() {
        out.push(Span::Text { t: s.into(), b: false, i: false, u: false, code: false, cloze: None, extra: Map::new() });
        return out;
    }
    for p in rt.as_array().into_iter().flatten() {
        if let Some(s) = p.as_str() {
            if !s.is_empty() {
                out.push(Span::Text { t: s.into(), b: false, i: false, u: false, code: false, cloze: None, extra: Map::new() });
            }
            continue;
        }
        let Some(o) = p.as_object() else { continue };
        let flag = |k: &str| o.get(k).is_some_and(|v| v.as_bool() != Some(false) && !v.is_null());
        match o.get("i").and_then(Value::as_str) {
            Some("m") => out.push(Span::Text {
                t: o.get("text").and_then(Value::as_str).unwrap_or("").into(),
                b: flag("b"),
                i: flag("l"),
                u: flag("u"),
                code: flag("q"),
                cloze: cloze_of(o),
                extra: extra(o, &KNOWN_M),
            }),
            Some("x") => out.push(Span::Latex {
                tex: o.get("text").and_then(Value::as_str).unwrap_or("").into(),
                cloze: cloze_of(o),
                extra: extra(o, &["i", "text", "cId", "cloze"]),
            }),
            Some("q") => out.push(Span::Ref {
                id: o.get("_id").and_then(Value::as_str).unwrap_or("").into(),
                cloze: cloze_of(o),
                extra: extra(o, &["i", "_id", "cId", "cloze"]),
            }),
            _ => out.push(Span::Opaque(p.clone())),
        }
    }
    out
}

pub fn to_rt(spans: &[Span]) -> Value {
    let mut out = vec![];
    for s in spans {
        match s {
            Span::Text { t, b, i, u, code, cloze, extra } => {
                if !b && !i && !u && !code && cloze.is_none() && extra.is_empty() {
                    if t.is_empty() {
                        continue;
                    }
                    // merge plain runs, like RemNote does
                    if let Some(Value::String(prev)) = out.last_mut() {
                        prev.push_str(t);
                        continue;
                    }
                    out.push(json!(t));
                    continue;
                }
                let mut o = extra.clone();
                o.insert("i".into(), json!("m"));
                o.insert("text".into(), json!(t));
                for (k, on) in [("b", b), ("l", i), ("u", u), ("q", code)] {
                    if *on {
                        o.insert(k.into(), json!(true));
                    }
                }
                if let Some(c) = cloze {
                    o.insert("cId".into(), json!(c));
                }
                out.push(Value::Object(o));
            }
            Span::Latex { tex, cloze, extra } => {
                let mut o = extra.clone();
                o.insert("i".into(), json!("x"));
                o.insert("text".into(), json!(tex));
                if let Some(c) = cloze {
                    o.insert("cId".into(), json!(c));
                }
                out.push(Value::Object(o));
            }
            Span::Ref { id, cloze, extra } => {
                let mut o = extra.clone();
                o.insert("i".into(), json!("q"));
                o.insert("_id".into(), json!(id));
                if let Some(c) = cloze {
                    o.insert("cId".into(), json!(c));
                }
                out.push(Value::Object(o));
            }
            Span::Opaque(v) => out.push(v.clone()),
        }
    }
    Value::Array(out)
}

pub fn plain(spans: &[Span], name: &dyn Fn(&str) -> String) -> String {
    spans
        .iter()
        .map(|s| match s {
            Span::Text { t, .. } => t.clone(),
            Span::Latex { tex, .. } => tex.clone(),
            Span::Ref { id, .. } => name(id),
            Span::Opaque(_) => String::new(),
        })
        .collect()
}

// --- deck syntax --------------------------------------------------------------

/// What a line's text needs from outside: ref names, and the bits that can't be
/// typed (opaque objects, existing cloze ids) so a round trip is lossless.
#[derive(Default, Clone, Debug)]
pub struct Side {
    pub opaque: Vec<Value>,
    pub refs: HashMap<String, String>,   // name -> id, from the original line
    pub clozes: HashMap<String, String>, // cloze text -> id, from the original line
    /// extra fields on text we don't model (sup, sub, colours): `⟪N:text⟫` -> styled[N]
    pub styled: Vec<Map<String, Value>>,
    /// extra fields on latex / refs (block, latexClozes, aliasId ...), by tex / id
    pub latex_extras: HashMap<String, Map<String, Value>>,
    pub ref_extras: HashMap<String, Map<String, Value>>,
}

fn esc(t: &str) -> String {
    // escape markup characters; brackets/braces only when doubled ([[ / {{)
    let cs: Vec<char> = t.chars().collect();
    let mut s = String::new();
    for (i, &c) in cs.iter().enumerate() {
        // doubled, or last in the run (a [[ref]] token may follow)
        let doubled = cs.get(i + 1).is_none_or(|n| *n == c);
        if matches!(c, '$' | '`' | '*' | '\\' | '⟪' | '⟫') || (matches!(c, '[' | '{') && doubled) {
            s.push('\\');
        }
        s.push(c);
    }
    s
}

pub fn spans_to_text(spans: &[Span], name: &dyn Fn(&str) -> String, side: &mut Side) -> String {
    let mut s = String::new();
    for sp in spans {
        let (body, cloze) = match sp {
            Span::Text { t, b, i, u, code, cloze, extra } => {
                let mut x = if *code { format!("`{}`", t.replace('`', "")) } else { esc(t) };
                if *i {
                    x = format!("*{x}*");
                }
                if *b {
                    x = format!("**{x}**");
                }
                let mut extra = extra.clone();
                if *u {
                    extra.insert("u".into(), json!(true));
                }
                if !extra.is_empty() {
                    side.styled.push(extra);
                    x = format!("⟪{}:{x}⟫", side.styled.len() - 1);
                }
                (x, cloze)
            }
            Span::Latex { tex, cloze, extra } => {
                if !extra.is_empty() {
                    side.latex_extras.insert(tex.clone(), extra.clone());
                }
                (format!("${tex}$"), cloze)
            }
            Span::Ref { id, cloze, extra } => {
                // names can themselves contain [[refs]]; strip brackets so the token parses
                let n: String = name(id).replace(['[', ']'], "").replace('\n', " ");
                side.refs.insert(n.clone(), id.clone());
                if !extra.is_empty() {
                    side.ref_extras.insert(id.clone(), extra.clone());
                }
                (format!("[[{n}]]"), cloze)
            }
            Span::Opaque(v) => {
                side.opaque.push(v.clone());
                (format!("![image]({})", side.opaque.len() - 1), &None)
            }
        };
        match cloze {
            Some(c) => {
                side.clozes.insert(body.clone(), c.clone());
                s.push_str(&format!("{{{{{body}}}}}"));
            }
            None => s.push_str(&body),
        }
    }
    s
}

pub fn line_to_text(l: &Line, name: &dyn Fn(&str) -> String, side: &mut Side) -> String {
    let f = spans_to_text(&l.front, name, side);
    let b = spans_to_text(&l.back, name, side);
    let join = |d: &str| if b.is_empty() { format!("{f} {d}") } else { format!("{f} {d} {b}") };
    match l.kind {
        Kind::Plain => f,
        // code block, optionally a card: ```<raw>``` >> back
        Kind::Code if b.is_empty() => format!("```{}", code_text(&l.front, name, side)),
        Kind::Code => format!("```{}``` >> {b}", code_text(&l.front, name, side)),
        Kind::Heading(n) => format!("{} {f}", "#".repeat(n.clamp(1, 5) as usize)),
        Kind::Concept => join("::"),
        Kind::Descriptor => join(";;"),
        Kind::Basic => join(">>"),
        Kind::Both => join("<>"),
        Kind::MultiLine => join(">>>"),
        Kind::Mcq => join(">>A)"),
    }
}

/// Code block text: raw, except refs (RemNote auto-links words in code) and
/// images, which become the same tokens as elsewhere.
fn code_text(spans: &[Span], name: &dyn Fn(&str) -> String, side: &mut Side) -> String {
    let mut s = String::new();
    for sp in spans {
        match sp {
            Span::Ref { .. } | Span::Opaque(_) => s.push_str(&spans_to_text(std::slice::from_ref(sp), name, side)),
            Span::Text { t, .. } => s.push_str(t),
            Span::Latex { tex, .. } => s.push_str(tex),
        }
    }
    s
}

fn code_spans(raw: &str, side: &Side) -> Vec<Span> {
    let plain = |t: &str| Span::Text { t: t.into(), b: false, i: false, u: false, code: false, cloze: None, extra: Map::new() };
    let mut out = vec![];
    let mut rest = raw;
    let mut buf = String::new();
    while !rest.is_empty() {
        let mut hit = None;
        if let Some(inner) = rest.strip_prefix("[[") {
            if let Some(end) = inner.find("]]") {
                if let Some(id) = side.refs.get(&inner[..end]) {
                    let extra = side.ref_extras.get(id).cloned().unwrap_or_default();
                    hit = Some((Span::Ref { id: id.clone(), cloze: None, extra }, 2 + end + 2));
                }
            }
        } else if let Some(inner) = rest.strip_prefix("![image](") {
            if let Some(end) = inner.find(')') {
                if let Some(v) = inner[..end].parse::<usize>().ok().and_then(|k| side.opaque.get(k)) {
                    hit = Some((Span::Opaque(v.clone()), 9 + end + 1));
                }
            }
        }
        match hit {
            Some((sp, n)) => {
                if !buf.is_empty() {
                    out.push(plain(&buf));
                    buf.clear();
                }
                out.push(sp);
                rest = &rest[n..];
            }
            None => {
                let c = rest.chars().next().unwrap();
                buf.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    if !buf.is_empty() {
        out.push(plain(&buf));
    }
    out
}

/// Byte ranges of `code` and $latex$, where delimiters and markup don't count.
fn protected(s: &str) -> Vec<(usize, usize)> {
    let b = s.as_bytes();
    let mut v = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            i += 2;
            continue;
        }
        if b[i] == b'`' || b[i] == b'$' {
            let q = b[i];
            if let Some(j) = b[i + 1..].iter().position(|&c| c == q) {
                v.push((i, i + 1 + j + 1));
                i += j + 2;
                continue;
            }
        }
        i += 1;
    }
    v
}

pub fn text_to_line(s: &str, resolve: &dyn Fn(&str) -> Option<String>, side: &Side, new_cloze: &mut dyn FnMut() -> String) -> Line {
    if let Some(raw) = s.strip_prefix("```") {
        let (raw, back) = match raw.rfind("``` >> ") {
            Some(k) => (&raw[..k], inline(&raw[k + 7..], resolve, side, new_cloze)),
            None => (raw, vec![]),
        };
        return Line { kind: Kind::Code, front: code_spans(raw, side), back };
    }
    let mut s = s.to_owned();
    let mut kind = Kind::Plain;
    let heading = s.chars().take_while(|&c| c == '#').count();
    if (1..=5).contains(&heading) && s[heading..].starts_with(' ') {
        kind = Kind::Heading(heading as u8);
        s = s[heading + 1..].to_owned();
    }
    let mut back = String::new();
    if kind == Kind::Plain {
        let prot = protected(&s);
        let free = |i: usize| !prot.iter().any(|&(a, b)| i >= a && i < b);
        // the first delimiter outside code/latex; at the same spot the longest wins
        {
            let mut best: Option<(usize, &str, Kind)> = None;
            for (d, k) in [(">>A)", Kind::Mcq), (">>>", Kind::MultiLine), ("::", Kind::Concept), (";;", Kind::Descriptor), ("<>", Kind::Both), (">>", Kind::Basic)] {
                let mut from = 0;
                while let Some(p) = s[from..].find(d) {
                    let at = from + p;
                    if free(at) {
                        if best.is_none_or(|(bp, bd, _)| at < bp || (at == bp && d.len() > bd.len())) {
                            best = Some((at, d, k));
                        }
                        break;
                    }
                    from = at + d.len();
                }
            }
            if let Some((at, d, k)) = best {
                kind = k;
                let b = &s[at + d.len()..];
                back = b.strip_prefix(' ').unwrap_or(b).to_owned();
                let f = &s[..at];
                s = f.strip_suffix(' ').unwrap_or(f).to_owned();
            }
        }
    }
    Line {
        kind,
        front: inline(&s, resolve, side, new_cloze),
        back: inline(&back, resolve, side, new_cloze),
    }
}

fn text_span(t: String, b: bool, i: bool, code: bool, cloze: Option<String>, _side: &Side) -> Span {
    Span::Text { t, b, i, u: false, code, cloze, extra: Map::new() }
}

/// Inline markup -> spans.
pub fn inline(s: &str, resolve: &dyn Fn(&str) -> Option<String>, side: &Side, new_cloze: &mut dyn FnMut() -> String) -> Vec<Span> {
    let mut out = vec![];
    let mut rest = s;
    while !rest.is_empty() {
        if let Some(inner) = rest.strip_prefix("{{") {
            if let Some(end) = inner.find("}}") {
                let body = &inner[..end];
                let id = side.clozes.get(body).cloned().unwrap_or_else(&mut *new_cloze);
                for mut sp in inline(body, resolve, side, new_cloze) {
                    match &mut sp {
                        Span::Text { cloze, .. } | Span::Latex { cloze, .. } | Span::Ref { cloze, .. } => *cloze = Some(id.clone()),
                        Span::Opaque(_) => {}
                    }
                    out.push(sp);
                }
                rest = &inner[end + 2..];
                continue;
            }
        }
        if let Some(inner) = rest.strip_prefix('⟪') {
            if let (Some(colon), Some(end)) = (inner.find(':'), inner.find('⟫')) {
                if let Some(ex) = inner[..colon].parse::<usize>().ok().and_then(|k| side.styled.get(k)).filter(|_| colon < end) {
                    for sp in inline(&inner[colon + 1..end], resolve, side, new_cloze) {
                        out.push(match sp {
                            Span::Text { t, b, i, code, cloze, .. } => {
                                let mut extra = ex.clone();
                                let u = extra.remove("u").is_some();
                                Span::Text { t, b, i, u, code, cloze, extra }
                            }
                            o => o,
                        });
                    }
                    rest = &inner[end + '⟫'.len_utf8()..];
                    continue;
                }
            }
        }
        if let Some(inner) = rest.strip_prefix("[[") {
            if let Some(end) = inner.find("]]") {
                let n = &inner[..end];
                match side.refs.get(n).cloned().or_else(|| resolve(n)) {
                    Some(id) => {
                        let extra = side.ref_extras.get(&id).cloned().unwrap_or_default();
                        out.push(Span::Ref { id, cloze: None, extra })
                    }
                    None => out.push(text_span(format!("[[{n}]]"), false, false, false, None, side)),
                }
                rest = &inner[end + 2..];
                continue;
            }
        }
        if let Some(inner) = rest.strip_prefix("![image](") {
            if let Some(end) = inner.find(')') {
                if let Some(v) = inner[..end].parse::<usize>().ok().and_then(|k| side.opaque.get(k)) {
                    out.push(Span::Opaque(v.clone()));
                    rest = &inner[end + 1..];
                    continue;
                }
            }
        }
        if let Some(inner) = rest.strip_prefix('$') {
            if let Some(end) = inner.find('$') {
                let tex: String = inner[..end].into();
                let extra = side.latex_extras.get(&tex).cloned().unwrap_or_default();
                out.push(Span::Latex { tex, cloze: None, extra });
                rest = &inner[end + 1..];
                continue;
            }
        }
        if let Some(inner) = rest.strip_prefix('`') {
            if let Some(end) = inner.find('`') {
                out.push(text_span(inner[..end].into(), false, false, true, None, side));
                rest = &inner[end + 1..];
                continue;
            }
        }
        if let Some(inner) = rest.strip_prefix("**") {
            if let Some(end) = inner.find("**") {
                for sp in inline(&inner[..end], resolve, side, new_cloze) {
                    out.push(match sp {
                        Span::Text { t, i, code, cloze, extra, .. } => Span::Text { t, b: true, i, u: false, code, cloze, extra },
                        o => o,
                    });
                }
                rest = &inner[end + 2..];
                continue;
            }
        }
        if let Some(inner) = rest.strip_prefix('*') {
            if let Some(end) = inner.find('*') {
                for sp in inline(&inner[..end], resolve, side, new_cloze) {
                    out.push(match sp {
                        Span::Text { t, b, code, cloze, extra, .. } => Span::Text { t, b, i: true, u: false, code, cloze, extra },
                        o => o,
                    });
                }
                rest = &inner[end + 1..];
                continue;
            }
        }
        // plain run up to the next markup char; backslash escapes one char
        let mut t = String::new();
        let mut it = rest.char_indices().peekable();
        let mut stop = rest.len();
        while let Some((k, c)) = it.next() {
            if c == '\\' {
                if let Some((_, n)) = it.next() {
                    t.push(n);
                }
                continue;
            }
            let r = &rest[k..];
            if k > 0 && (r.starts_with("{{") || r.starts_with("[[") || r.starts_with("![image](") || matches!(c, '$' | '`' | '*' | '⟪')) {
                stop = k;
                break;
            }
            if k == 0 && matches!(c, '$' | '`' | '*' | '{' | '[' | '!' | '⟪') {
                // unmatched markup char at the start: take it literally
                t.push(c);
                continue;
            }
            t.push(c);
        }
        out.push(text_span(t, false, false, false, None, side));
        rest = &rest[stop..];
    }
    // merge adjacent plain runs
    let mut merged: Vec<Span> = vec![];
    for sp in out {
        if let (Some(Span::Text { t: pt, b: false, i: false, u: false, code: false, cloze: None, extra: pe }), Span::Text { t, b: false, i: false, u: false, code: false, cloze: None, extra }) =
            (merged.last_mut(), &sp)
        {
            if pe.is_empty() && extra.is_empty() {
                pt.push_str(t);
                continue;
            }
        }
        merged.push(sp);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(s: &str) -> Vec<Span> {
        parse_rt(Some(&serde_json::from_str(s).unwrap()))
    }

    fn round(l: &Line) -> Line {
        let names = |id: &str| format!("name-{id}");
        let mut side = Side::default();
        let text = line_to_text(l, &names, &mut side);
        let mut n = 0;
        let mut nc = || {
            n += 1;
            format!("new{n}")
        };
        text_to_line(&text, &|_| None, &side, &mut nc)
    }

    #[test]
    fn roundtrips_rich_text() {
        let front = rt(r#"["latex ", {"i":"x","text":"\\frac{a}{b}"}, " code ", {"i":"m","text":"std::vector","q":true}, " ", {"i":"m","text":"strong","b":true}, " ref ", {"i":"q","_id":"abc"}, " ", {"i":"i","url":"%LOCAL_FILE%x.png"}]"#);
        let back = rt(r#"["the answer is ", {"i":"m","text":"42","cId":"777"}, " and $5"]"#);
        for kind in [Kind::Plain, Kind::Concept, Kind::Descriptor, Kind::Basic, Kind::Both, Kind::MultiLine, Kind::Mcq, Kind::Heading(2)] {
            let l = Line { kind, front: front.clone(), back: if matches!(kind, Kind::Plain | Kind::Heading(_)) { vec![] } else { back.clone() } };
            let r = round(&l);
            assert_eq!(r.kind, l.kind, "{kind:?}");
            assert_eq!(to_rt(&r.front), to_rt(&l.front), "{kind:?} front");
            assert_eq!(to_rt(&r.back), to_rt(&l.back), "{kind:?} back");
        }
    }

    #[test]
    fn delimiters_inside_code_dont_count() {
        let side = Side::default();
        let l = text_to_line("`a::b` and $x >> y$ :: def", &|_| None, &side, &mut || "c".into());
        assert_eq!(l.kind, Kind::Concept);
        assert_eq!(to_rt(&l.back), json!(["def"]));
    }

    #[test]
    fn typed_markup() {
        let side = Side::default();
        let mut n = 0;
        let l = text_to_line("**bold** *it* {{hidden}} [[Thing]] >> back", &|n| (n == "Thing").then(|| "id1".into()), &side, &mut || {
            n += 1;
            format!("c{n}")
        });
        assert_eq!(l.kind, Kind::Basic);
        assert_eq!(
            to_rt(&l.front),
            json!([{"i":"m","text":"bold","b":true}, " ", {"i":"m","text":"it","l":true}, " ", {"i":"m","text":"hidden","cId":"c1"}, " ", {"i":"q","_id":"id1"}])
        );
    }
}
