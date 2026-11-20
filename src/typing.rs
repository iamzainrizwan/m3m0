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
}
