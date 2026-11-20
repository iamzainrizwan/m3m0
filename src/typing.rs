//! What typing does in insert mode, as plain functions over the buffer (so it
//! can be tested without a UI). Only keys he types come through here: paste,
//! `:stage`, Claude cards and text read from RemNote never do. Nothing fires
//! inside `` `code` `` or `$latex$`, except stepping over their own closer.
//!
//! - auto-pairs: `( [ { " `` ` `` $` insert their closer, the closer steps over
//!   it, backspace between a fresh pair deletes both (`'` never pairs; `$`
//!   only after a space or at line start). `[[` -> `[[]]`, `{{` -> `{{}}`.
//! - `==` -> ` >> `, a third `=` -> ` >>> `; typed markers (`>>` `>>>` `::`
//!   `;;` `<>` `<<`) get one space each side; `>> A)` closes up to `>>A)`.
//!   Backspace straight after any of these puts back exactly what was typed.
//! - format keys wrap the selection or the word at the caret (toggle).

/// Buffer + selection, in char indices.
#[derive(Clone, Debug, PartialEq)]
pub struct Buf {
    pub text: String,
    pub anchor: usize,
    pub caret: usize,
}

/// The last automatic change, so backspace right after can put back the literal.
#[derive(Clone, Debug)]
pub struct Undo {
    literal: Buf,
    converted: Buf,
}

impl Buf {
    #[cfg(test)]
    pub fn new(text: &str, caret: usize) -> Buf {
        Buf { text: text.to_owned(), anchor: caret, caret }
    }
    fn chars(&self) -> Vec<char> {
        self.text.chars().collect()
    }
    fn sel(&self) -> (usize, usize) {
        (self.anchor.min(self.caret), self.anchor.max(self.caret))
    }
    /// Replace chars a..b with `s`, caret after it.
    fn splice(&mut self, a: usize, b: usize, s: &str) {
        let cs = self.chars();
        let mut t: String = cs[..a].iter().collect();
        t.push_str(s);
        t.extend(&cs[b.min(cs.len())..]);
        self.text = t;
        self.caret = a + s.chars().count();
        self.anchor = self.caret;
    }
    fn delete_sel(&mut self) -> bool {
        let (a, b) = self.sel();
        if a == b {
            return false;
        }
        self.splice(a, b, "");
        true
    }
    fn before(&self) -> String {
        self.chars()[..self.caret].iter().collect()
    }
    fn prev(&self) -> Option<char> {
        self.caret.checked_sub(1).and_then(|i| self.chars().get(i).copied())
    }
    fn next(&self) -> Option<char> {
        self.chars().get(self.caret).copied()
    }
}

/// Is the caret inside `code` or $latex$ (counting unescaped delimiters before it)?
pub fn protected_at(text: &str, caret: usize) -> Option<char> {
    let mut open: Option<char> = None;
    let mut esc = false;
    for c in text.chars().take(caret) {
        if esc {
            esc = false;
            continue;
        }
        match (c, open) {
            ('\\', _) => esc = true,
            ('`', None) | ('$', None) => open = Some(c),
            (c, Some(o)) if c == o => open = None,
            _ => {}
        }
    }
    open
}

fn closer(c: char) -> Option<char> {
    Some(match c {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '"' => '"',
        '`' => '`',
        '$' => '$',
        _ => return None,
    })
}

/// A spot where a pair may open: end of text, whitespace or a closer follows.
fn free_after(n: Option<char>) -> bool {
    n.is_none_or(|n| n.is_whitespace() || ")]}\"`$,.;:".contains(n))
}

fn free_before(p: Option<char>) -> bool {
    p.is_none_or(|p| p.is_whitespace() || "([{".contains(p))
}

/// Type one char.
pub fn type_char(b: &mut Buf, c: char, undo: &mut Option<Undo>) {
    let last = undo.take().filter(|u| u.converted == *b);
    b.delete_sel();
    let (p, n) = (b.prev(), b.next());
    let at = b.caret;
    if let Some(open) = protected_at(&b.text, at) {
        // inside code / latex: only its own closer steps over
        if c == open && n == Some(c) {
            b.caret += 1;
            b.anchor = b.caret;
        } else {
            b.splice(at, at, &c.to_string());
        }
        return;
    }
    // a space straight after an auto-spaced marker is already there
    if c == ' ' && last.is_some() && p == Some(' ') {
        *undo = last;
        return;
    }
    // closers step over the one already there
    if ")]}\"".contains(c) && n == Some(c) {
        b.caret += 1;
        b.anchor = b.caret;
        return;
    }
    let before = b.before();
    let literal = |b: &Buf, s: &str| {
        let mut l = b.clone();
        l.splice(at, at, s);
        l
    };
    // == -> >>, === -> >>>
    if c == '=' {
        if let Some(l) = last.as_ref().filter(|_| before.ends_with(" >> ")) {
            if l.literal.before().ends_with("==") {
                let mut lit = l.literal.clone();
                lit.splice(lit.caret, lit.caret, "=");
                return marker(b, at - 4, at, ">>>", lit, undo);
            }
        }
        if p == Some('=') && !before.ends_with("==") {
            return marker(b, at - 1, at, ">>", literal(b, "="), undo);
        }
    }
    // typed markers get one space each side; >> grows into >>> and >>A)
    if c == '>' && last.is_some() && before.ends_with(" >> ") {
        return marker(b, at - 4, at, ">>>", literal(b, ">"), undo);
    }
    if c == ')' && before.ends_with(" >> A") {
        let mut lit = last.map(|l| l.literal).unwrap_or_else(|| b.clone());
        lit.splice(lit.caret, lit.caret, ")");
        let mut conv = b.clone();
        conv.splice(at - 5, at, " >>A)");
        *b = conv.clone();
        *undo = Some(Undo { literal: lit, converted: conv });
        return;
    }
    if let Some(p) = p {
        let two: String = [p, c].iter().collect();
        let before2 = before.chars().rev().nth(1);
        if ["::", ";;", "<>", "<<", ">>"].contains(&two.as_str()) && before2 != Some(p) && before2 != Some(c) {
            return marker(b, at - 1, at, &two, literal(b, &c.to_string()), undo);
        }
    }
    // openers
    if let Some(cl) = closer(c) {
        let pair = match c {
            '(' | '[' | '{' => free_after(n),
            '"' => free_before(p) && free_after(n),
            '`' => p != Some('`') && free_before(p) && free_after(n),
            '$' => p.is_none_or(char::is_whitespace) && free_after(n),
            _ => false,
        };
        if pair {
            b.splice(at, at, &format!("{c}{cl}"));
            b.caret -= 1;
            b.anchor = b.caret;
            return;
        }
    }
    b.splice(at, at, &c.to_string());
}

/// Replace chars a..at (what came before the marker's last char) with ` m `.
fn marker(b: &mut Buf, a: usize, at: usize, m: &str, literal: Buf, undo: &mut Option<Undo>) {
    let cs = b.chars();
    // eat spaces already typed before the marker
    let mut a = a;
    while a > 0 && cs[a - 1] == ' ' {
        a -= 1;
    }
    let lead = if a == 0 { "" } else { " " };
    let trail = if cs.get(at) == Some(&' ') { "" } else { " " };
    b.splice(a, at, &format!("{lead}{m}{trail}"));
    if trail.is_empty() {
        b.caret += 1;
        b.anchor = b.caret;
    }
    *undo = Some(Undo { literal, converted: b.clone() });
}

/// Backspace with text in the buffer. Puts back what was literally typed
/// right after an automatic change, and deletes a fresh pair together.
pub fn backspace(b: &mut Buf, undo: &mut Option<Undo>) {
    if let Some(u) = undo.take().filter(|u| u.converted == *b) {
        *b = u.literal;
        return;
    }
    if b.delete_sel() || b.caret == 0 {
        return;
    }
    let at = b.caret;
    if let (Some(p), Some(n)) = (b.prev(), b.next()) {
        if closer(p) == Some(n) {
            b.splice(at - 1, at + 1, "");
            return;
        }
    }
    b.splice(at - 1, at, "");
}

/// Wrap the selection (or the word at the caret) in `m`, or unwrap it if it's
/// wrapped already. No word: insert an empty pair with the caret inside.
pub fn wrap(b: &mut Buf, m: &str) {
    let cs = b.chars();
    let (mut a, mut z) = b.sel();
    if a == z {
        let word = |c: &char| c.is_alphanumeric() || *c == '_';
        while a > 0 && word(&cs[a - 1]) {
            a -= 1;
        }
        while z < cs.len() && word(&cs[z]) {
            z += 1;
        }
    }
    let ml = m.chars().count();
    let s: String = cs[a..z].iter().collect();
    let pre: String = cs[a.saturating_sub(ml)..a].iter().collect();
    let post: String = cs[z..(z + ml).min(cs.len())].iter().collect();
    if a == z {
        b.splice(a, a, &format!("{m}{m}"));
        b.caret = a + ml;
        b.anchor = b.caret;
    } else if pre == m && post == m && a >= ml {
        b.splice(a - ml, z + ml, &s);
    } else {
        b.splice(a, z, &format!("{m}{s}{m}"));
    }
}

/// `/` commands: (name, what it does).
pub const SLASH: &[(&str, &str)] = &[
    ("h1", "heading 1"),
    ("h2", "heading 2"),
    ("h3", "heading 3"),
    ("ref", "[[reference]]"),
    ("cloze", "{{cloze}}"),
    ("code", "`inline code`"),
    ("math", "$latex$"),
    ("basic", "front >> back"),
    ("concept", "front :: back"),
    ("descriptor", "front ;; back"),
    ("both", "front <> back"),
    ("multi", "front >>> (children are the answer)"),
    ("mcq", "front >>A) (children are the options)"),
    ("bold", "**bold**"),
    ("italic", "*italic*"),
];

pub fn slash_matches(q: &str) -> Vec<usize> {
    let q = q.to_lowercase();
    let mut v: Vec<(bool, usize)> = SLASH.iter().enumerate().filter(|(_, (n, d))| n.contains(&q) || d.contains(&q)).map(|(i, (n, _))| (!n.starts_with(&q), i)).collect();
    v.sort();
    v.into_iter().map(|x| x.1).collect()
}

/// Run slash command `i`, which was typed as chars `start..b.caret` (`/query`).
pub fn slash_apply(b: &mut Buf, start: usize, i: usize) {
    b.splice(start, b.caret, "");
    let set_kind = |b: &mut Buf, m: &str| {
        // drop any marker already there, then put this one at the end
        let t = b.text.trim_end().to_owned();
        b.text = t;
        let n = b.chars().len();
        b.caret = n;
        b.anchor = n;
        let lead = if n == 0 { "" } else { " " };
        b.splice(n, n, &format!("{lead}{m} "));
    };
    match SLASH[i].0 {
        h @ ("h1" | "h2" | "h3") => {
            let lvl = (h.as_bytes()[1] - b'0') as usize;
            let caret = b.caret;
            let cs = b.chars();
            let old = cs.iter().take_while(|&&c| c == '#').count();
            let old = if old > 0 && cs.get(old) == Some(&' ') { old + 1 } else { 0 };
            b.splice(0, old, &format!("{} ", "#".repeat(lvl)));
            b.caret = (caret + lvl + 1).saturating_sub(old);
            b.anchor = b.caret;
        }
        "ref" => pair_in(b, "[[", "]]"),
        "cloze" => pair_in(b, "{{", "}}"),
        "code" => pair_in(b, "`", "`"),
        "math" => pair_in(b, "$", "$"),
        "bold" => wrap(b, "**"),
        "italic" => wrap(b, "*"),
        "basic" => set_kind(b, ">>"),
        "concept" => set_kind(b, "::"),
        "descriptor" => set_kind(b, ";;"),
        "both" => set_kind(b, "<>"),
        "multi" => {
            set_kind(b, ">>>");
            trim_end(b);
        }
        "mcq" => {
            set_kind(b, ">>A)");
            trim_end(b);
        }
        _ => {}
    }
}

fn pair_in(b: &mut Buf, o: &str, c: &str) {
    let at = b.caret;
    b.splice(at, at, &format!("{o}{c}"));
    b.caret = at + o.chars().count();
    b.anchor = b.caret;
}

fn trim_end(b: &mut Buf) {
    let t = b.text.trim_end().to_owned();
    b.text = t;
    b.caret = b.chars().len();
    b.anchor = b.caret;
}


// --- vim motions inside a line (char indices) --------------------------------

fn class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

/// `w`: start of the next word.
pub fn word_fwd(cs: &[char], i: usize) -> usize {
    let n = cs.len();
    if i >= n {
        return n;
    }
    let c0 = class(cs[i]);
    let mut j = i;
    while j < n && c0 != 0 && class(cs[j]) == c0 {
        j += 1;
    }
    while j < n && class(cs[j]) == 0 {
        j += 1;
    }
    j
}

/// `b`: start of this / the previous word.
pub fn word_back(cs: &[char], i: usize) -> usize {
    let mut j = i.min(cs.len());
    while j > 0 && class(cs[j - 1]) == 0 {
        j -= 1;
    }
    if j == 0 {
        return 0;
    }
    let c = class(cs[j - 1]);
    while j > 0 && class(cs[j - 1]) == c {
        j -= 1;
    }
    j
}

/// `e`: end of this / the next word (moves at least one char).
pub fn word_end(cs: &[char], i: usize) -> usize {
    let n = cs.len();
    let mut j = i + 1;
    while j < n && class(cs[j]) == 0 {
        j += 1;
    }
    if j >= n {
        return n.saturating_sub(1);
    }
    let c = class(cs[j]);
    while j + 1 < n && class(cs[j + 1]) == c {
        j += 1;
    }
    j
}

/// `iw`: the word (or run of spaces / punctuation) under the cursor.
pub fn inner_word(cs: &[char], i: usize) -> (usize, usize) {
    let n = cs.len();
    if n == 0 {
        return (0, 0);
    }
    let i = i.min(n - 1);
    let c = class(cs[i]);
    let (mut a, mut z) = (i, i + 1);
    while a > 0 && class(cs[a - 1]) == c {
        a -= 1;
    }
    while z < n && class(cs[z]) == c {
        z += 1;
    }
    (a, z)
}

/// `f t F T` + a char.
pub fn find(cs: &[char], i: usize, kind: char, ch: char) -> Option<usize> {
    match kind {
        'f' => (i + 1..cs.len()).find(|&j| cs[j] == ch),
        't' => (i + 2..cs.len()).find(|&j| cs[j] == ch).map(|j| j - 1),
        'F' => (0..i).rev().find(|&j| cs[j] == ch),
        'T' => (0..i.saturating_sub(1)).rev().find(|&j| cs[j] == ch).map(|j| j + 1),
        _ => None,
    }
}

/// First non-blank (`^`, `I`).
pub fn first_non_blank(cs: &[char]) -> usize {
    cs.iter().position(|c| !c.is_whitespace()).unwrap_or(0)
}

// --- vim commands inside a line: [count] [op [count]] motion / text object ---

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// h l w b e W B E 0 ^ $
    Motion(char),
    /// f t F T + char
    Find(char, char),
    /// i / a + w W ( ) b [ ] { } B < > " ' ` $
    Obj(bool, char),
    /// dd cc yy: the whole line
    Line,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Parsed {
    /// wait for more keys
    More,
    Bad,
    Move { count: usize, op: Option<char>, target: Target },
    /// anything else (x p i a v D C r ~ u ...), with its count and argument
    Other { count: usize, key: char, arg: Option<char> },
}

const MOTIONS: &str = "hlwbeWBE0^$ ";

pub fn parse(s: &str, visual: bool) -> Parsed {
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    let num = |i: &mut usize| {
        let st = *i;
        while *i < cs.len() && cs[*i].is_ascii_digit() && !(*i == st && cs[*i] == '0') {
            *i += 1;
        }
        cs[st..*i].iter().collect::<String>().parse::<usize>().ok()
    };
    let c1 = num(&mut i);
    let Some(&c) = cs.get(i) else { return Parsed::More };
    i += 1;
    let mut count = c1.unwrap_or(1);
    let target = |i: usize, op: bool| -> Option<Option<Target>> {
        let Some(&t) = cs.get(i) else { return Some(None) };
        Some(Some(match t {
            'f' | 't' | 'F' | 'T' => match cs.get(i + 1) {
                Some(&x) => Target::Find(t, x),
                None => return Some(None),
            },
            'i' | 'a' if op || visual => match cs.get(i + 1) {
                Some(&x) if "wW()b[]{}B<>\"'`$".contains(x) => Target::Obj(t == 'i', x),
                Some(_) => return None,
                None => return Some(None),
            },
            m if MOTIONS.contains(m) => Target::Motion(m),
            _ => return None,
        }))
    };
    if "dcy".contains(c) && !visual {
        let c2 = num(&mut i);
        count *= c2.unwrap_or(1);
        if cs.get(i) == Some(&c) {
            return Parsed::Move { count, op: Some(c), target: Target::Line };
        }
        return match target(i, true) {
            None => Parsed::Bad,
            Some(None) => Parsed::More,
            Some(Some(t)) => Parsed::Move { count, op: Some(c), target: t },
        };
    }
    if "fFtT".contains(c) || MOTIONS.contains(c) || (visual && "ia".contains(c)) {
        return match target(i - 1, false) {
            None => Parsed::Bad,
            Some(None) => Parsed::More,
            Some(Some(t)) => Parsed::Move { count, op: None, target: t },
        };
    }
    if c == 'r' {
        return match cs.get(i) {
            Some(&x) => Parsed::Other { count, key: 'r', arg: Some(x) },
            None => Parsed::More,
        };
    }
    Parsed::Other { count, key: c, arg: None }
}

fn big(c: char) -> u8 {
    if c.is_whitespace() { 0 } else { 1 }
}

/// W / B / E: like w b e over runs of non-blanks.
fn word_big(cs: &[char], i: usize, m: char) -> usize {
    let n = cs.len();
    match m {
        'W' => {
            let mut j = i;
            while j < n && big(cs[j]) == 1 {
                j += 1;
            }
            while j < n && big(cs[j]) == 0 {
                j += 1;
            }
            j
        }
        'B' => {
            let mut j = i.min(n);
            while j > 0 && big(cs[j - 1]) == 0 {
                j -= 1;
            }
            while j > 0 && big(cs[j - 1]) == 1 {
                j -= 1;
            }
            j
        }
        _ => {
            let mut j = i + 1;
            while j < n && big(cs[j]) == 0 {
                j += 1;
            }
            while j + 1 < n && big(cs[j + 1]) == 1 {
                j += 1;
            }
            j.min(n.saturating_sub(1))
        }
    }
}

/// Where a motion lands from `i`, `count` times.
pub fn motion(cs: &[char], i: usize, m: char, count: usize) -> usize {
    let n = cs.len();
    let mut j = i;
    for _ in 0..count.max(1) {
        j = match m {
            'h' => j.saturating_sub(1),
            'l' | ' ' => (j + 1).min(n),
            'w' => word_fwd(cs, j),
            'b' => word_back(cs, j),
            'e' => word_end(cs, j),
            'W' | 'B' | 'E' => word_big(cs, j, m),
            '0' => 0,
            '^' => first_non_blank(cs),
            '$' => n,
            _ => j,
        };
    }
    j
}

/// `f t F T` repeated `count` times.
pub fn find_n(cs: &[char], i: usize, kind: char, ch: char, count: usize) -> Option<usize> {
    let mut j = i;
    for k in 0..count.max(1) {
        // t / T: step past the char we stopped next to, or they'd stick
        let from = match (kind, k) {
            ('t', k) if k > 0 => j + 1,
            ('T', k) if k > 0 => j.saturating_sub(1),
            _ => j,
        };
        j = find(cs, from, kind, ch)?;
    }
    Some(j)
}

/// Text object `i`/`a` + `w W ( b [ { B < " ' `` ` `` $` around `i`: (start, end).
pub fn object(cs: &[char], i: usize, inner: bool, o: char) -> Option<(usize, usize)> {
    let n = cs.len();
    if n == 0 {
        return None;
    }
    let i = i.min(n - 1);
    match o {
        'w' | 'W' => {
            let (a, z) = if o == 'w' {
                inner_word(cs, i)
            } else {
                let c = big(cs[i]);
                let (mut a, mut z) = (i, i + 1);
                while a > 0 && big(cs[a - 1]) == c {
                    a -= 1;
                }
                while z < n && big(cs[z]) == c {
                    z += 1;
                }
                (a, z)
            };
            if inner {
                return Some((a, z));
            }
            // aw: the trailing blanks too, else the leading ones
            let mut z2 = z;
            while z2 < n && cs[z2].is_whitespace() {
                z2 += 1;
            }
            if z2 > z {
                return Some((a, z2));
            }
            let mut a2 = a;
            while a2 > 0 && cs[a2 - 1].is_whitespace() {
                a2 -= 1;
            }
            Some((a2, z))
        }
        '(' | ')' | 'b' | '[' | ']' | '{' | '}' | 'B' | '<' | '>' => {
            let (op, cl) = match o {
                '(' | ')' | 'b' => ('(', ')'),
                '[' | ']' => ('[', ']'),
                '<' | '>' => ('<', '>'),
                _ => ('{', '}'),
            };
            // the nearest unmatched opener at or before the cursor
            let mut depth = 0;
            let mut a = None;
            for j in (0..=i).rev() {
                if cs[j] == cl && j != i {
                    depth += 1;
                } else if cs[j] == op {
                    if depth == 0 {
                        a = Some(j);
                        break;
                    }
                    depth -= 1;
                }
            }
            let a = a?;
            let mut depth = 0;
            let z = (a + 1..n).find(|&j| {
                if cs[j] == op {
                    depth += 1;
                } else if cs[j] == cl {
                    if depth == 0 {
                        return true;
                    }
                    depth -= 1;
                }
                false
            })?;
            Some(if inner { (a + 1, z) } else { (a, z + 1) })
        }
        '"' | '\'' | '`' | '$' => {
            let qs: Vec<usize> = (0..n).filter(|&j| cs[j] == o && (j == 0 || cs[j - 1] != '\\')).collect();
            let pair = qs.chunks(2).filter(|p| p.len() == 2).find(|p| p[0] <= i && i <= p[1]).or_else(|| qs.chunks(2).filter(|p| p.len() == 2).find(|p| p[0] > i))?;
            let (a, z) = (pair[0], pair[1]);
            Some(if inner { (a + 1, z) } else { (a, z + 1) })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> Buf {
        let mut b = Buf::new("", 0);
        let mut u = None;
        for c in s.chars() {
            if c == '\u{8}' {
                backspace(&mut b, &mut u);
            } else {
                type_char(&mut b, c, &mut u);
            }
        }
        b
    }

    fn shown(b: &Buf) -> String {
        let cs: Vec<char> = b.text.chars().collect();
        let mut s: String = cs[..b.caret].iter().collect();
        s.push('|');
        s.extend(&cs[b.caret..]);
        s
    }

    #[test]
    fn pairs() {
        assert_eq!(shown(&typed("f(x")), "f(x|)");
        assert_eq!(shown(&typed("f(x)")), "f(x)|");
        assert_eq!(shown(&typed("[[")), "[[|]]");
        assert_eq!(shown(&typed("{{a")), "{{a|}}");
        assert_eq!(shown(&typed("(\u{8}")), "|");
        assert_eq!(shown(&typed("it's")), "it's|");
        assert_eq!(shown(&typed("a $x")), "a $x|$");
        assert_eq!(shown(&typed("a$")), "a$|");
        assert_eq!(shown(&typed("```")), "```|");
        assert_eq!(shown(&typed("say \"hi")), "say \"hi|\"");
    }

    #[test]
    fn markers() {
        assert_eq!(shown(&typed("a==")), "a >> |");
        assert_eq!(shown(&typed("a==b")), "a >> b|");
        assert_eq!(shown(&typed("a===")), "a >>> |");
        assert_eq!(shown(&typed("a==\u{8}")), "a==|");
        assert_eq!(shown(&typed("a===\u{8}")), "a===|");
        assert_eq!(shown(&typed("a::b")), "a :: b|");
        assert_eq!(shown(&typed("a :: b")), "a :: b|");
        assert_eq!(shown(&typed("a>>>")), "a >>> |");
        assert_eq!(shown(&typed("q>>A)")), "q >>A)|");
        assert_eq!(shown(&typed("std::\u{8}")), "std::|");
        assert_eq!(shown(&typed("x<>y")), "x <> y|");
        // nothing inside code or latex
        assert_eq!(shown(&typed("`a::b")), "`a::b|`");
        assert_eq!(shown(&typed("a $x==y")), "a $x==y|$");
    }

    #[test]
    fn wrapping() {
        let mut b = Buf::new("make this bold", 7);
        wrap(&mut b, "**");
        assert_eq!(b.text, "make **this** bold");
        let mut b = Buf::new("make **this** bold", 9);
        wrap(&mut b, "**");
        assert_eq!(b.text, "make this bold");
        let mut b = Buf::new("x ", 2);
        wrap(&mut b, "`");
        assert_eq!(shown(&b), "x `|`");
    }

    #[test]
    fn slash() {
        let mut b = Buf::new("/h2", 3);
        slash_apply(&mut b, 0, slash_matches("h2")[0]);
        assert_eq!(shown(&b), "## |");
        let mut b = Buf::new("Question /mcq", 13);
        slash_apply(&mut b, 9, slash_matches("mcq")[0]);
        assert_eq!(shown(&b), "Question >>A)|");
        let mut b = Buf::new("see /ref", 8);
        slash_apply(&mut b, 4, slash_matches("ref")[0]);
        assert_eq!(shown(&b), "see [[|]]");
    }

    #[test]
    fn motions() {
        let cs: Vec<char> = "foo bar.baz  qux".chars().collect();
        assert_eq!(word_fwd(&cs, 0), 4);
        assert_eq!(word_fwd(&cs, 4), 7);
        assert_eq!(word_fwd(&cs, 7), 8);
        assert_eq!(word_fwd(&cs, 8), 13);
        assert_eq!(word_back(&cs, 13), 8);
        assert_eq!(word_back(&cs, 5), 4);
        assert_eq!(word_end(&cs, 0), 2);
        assert_eq!(word_end(&cs, 2), 6);
        assert_eq!(inner_word(&cs, 5), (4, 7));
        assert_eq!(find(&cs, 0, 'f', 'b'), Some(4));
        assert_eq!(find(&cs, 0, 't', 'b'), Some(3));
        assert_eq!(find(&cs, 10, 'F', 'b'), Some(8));
        assert_eq!(find(&cs, 10, 'T', 'b'), Some(9));
    }

    #[test]
    fn vim_parse() {
        assert_eq!(parse("3", false), Parsed::More);
        assert_eq!(parse("3w", false), Parsed::Move { count: 3, op: None, target: Target::Motion('w') });
        assert_eq!(parse("2d3w", false), Parsed::Move { count: 6, op: Some('d'), target: Target::Motion('w') });
        assert_eq!(parse("d", false), Parsed::More);
        assert_eq!(parse("di", false), Parsed::More);
        assert_eq!(parse("ci(", false), Parsed::Move { count: 1, op: Some('c'), target: Target::Obj(true, '(') });
        assert_eq!(parse("da\"", false), Parsed::Move { count: 1, op: Some('d'), target: Target::Obj(false, '"') });
        assert_eq!(parse("dd", false), Parsed::Move { count: 1, op: Some('d'), target: Target::Line });
        assert_eq!(parse("2fx", false), Parsed::Move { count: 2, op: None, target: Target::Find('f', 'x') });
        assert_eq!(parse("0", false), Parsed::Move { count: 1, op: None, target: Target::Motion('0') });
        assert_eq!(parse("10l", false), Parsed::Move { count: 10, op: None, target: Target::Motion('l') });
        assert_eq!(parse("3x", false), Parsed::Other { count: 3, key: 'x', arg: None });
        assert_eq!(parse("r", false), Parsed::More);
        assert_eq!(parse("rq", false), Parsed::Other { count: 1, key: 'r', arg: Some('q') });
        assert_eq!(parse("dz", false), Parsed::Bad);
        assert_eq!(parse("iw", true), Parsed::Move { count: 1, op: None, target: Target::Obj(true, 'w') });
    }

    #[test]
    fn objects() {
        let cs: Vec<char> = "f(a, (b)) say \"hi there\" $x^2$".chars().collect();
        assert_eq!(object(&cs, 2, true, '('), Some((2, 8)));
        assert_eq!(object(&cs, 6, true, 'b'), Some((6, 7)));
        assert_eq!(object(&cs, 6, false, '('), Some((5, 8)));
        assert_eq!(object(&cs, 17, true, '"'), Some((15, 23)));
        assert_eq!(object(&cs, 10, true, '"'), Some((15, 23)));
        assert_eq!(object(&cs, 26, true, '$'), Some((26, 29)));
        assert_eq!(object(&cs, 10, false, 'w'), Some((10, 14)));
        assert_eq!(motion(&cs, 0, 'w', 2), 2);
        assert_eq!(motion(&cs, 0, 'W', 2), 10);
        assert_eq!(find_n(&cs, 0, 'f', '(', 2), Some(5));
    }
}
