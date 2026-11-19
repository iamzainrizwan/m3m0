//! Renderers for the non-PDF files in the week pane: quiz JSON (with answers
//! hidden until `a`), markdown (deck bullets show card markers), other JSON
//! (pretty), images, and plain text / code.

use crate::kb::Kb;
use crate::math::Math;
use crate::rich;
use crate::theme::*;
use crate::NOTE_TEXT;
use eframe::egui::{self, FontFamily, RichText, TextureHandle, Vec2};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq)]
pub enum FileKind {
    Pdf,
    Quiz,
    Json,
    Markdown,
    Image,
    Text,
}

pub fn kind(p: &Path) -> FileKind {
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "pdf" => FileKind::Pdf,
        "md" | "markdown" => FileKind::Markdown,
        "png" | "jpg" | "jpeg" => FileKind::Image,
        "json" => {
            if p.components().any(|c| c.as_os_str() == "quizzes") {
                FileKind::Quiz
            } else {
                FileKind::Json
            }
        }
        _ => FileKind::Text,
    }
}

/// Short fixed-width tag for a file kind (text, not glyphs: the nerd-font icons
/// come from different sets and never lined up).
pub fn icon(k: FileKind) -> &'static str {
    match k {
        FileKind::Pdf => "pdf ",
        FileKind::Quiz => "quiz",
        FileKind::Json => "json",
        FileKind::Markdown => "md  ",
        FileKind::Image => "img ",
        FileKind::Text => "txt ",
    }
}

/// Quiz answers for a quiz file: question id -> (source, answer object), from
/// `<module>/data/answers/<source>/week-NN.json`.
fn answers_for(quiz: &Path) -> HashMap<String, Vec<(String, Value)>> {
    let mut out: HashMap<String, Vec<(String, Value)>> = HashMap::new();
    let Some(name) = quiz.file_name() else { return out };
    let Some(data) = quiz.parent().and_then(|p| p.parent()) else { return out };
    let Ok(rd) = std::fs::read_dir(data.join("answers")) else { return out };
    for e in rd.flatten() {
        let src = e.file_name().to_string_lossy().into_owned();
        let f = e.path().join(name);
        let Some(v) = std::fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) else { continue };
        for a in v["answers"].as_array().into_iter().flatten() {
            if let Some(q) = a["question"].as_str() {
                out.entry(q.to_owned()).or_default().push((src.clone(), a.clone()));
            }
        }
    }
    out
}

/// LaTeX text mode (quiz files) -> our inline markup, leaving $...$ alone:
/// \texttt{x} -> `x`, \textbf{x} -> **x**, \emph/\textit{x} -> *x*, \, ~ -> space,
/// \% \& \_ \# \$ -> the character, \ldots -> …, -- / --- -> dashes, quotes.
pub fn latex_text(s: &str) -> String {
    let mut out = String::new();
    let mut in_math = false;
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    let arg = |i: usize| -> Option<(String, usize)> {
        if cs.get(i) != Some(&'{') {
            return None;
        }
        let mut depth = 0;
        for (j, c) in cs.iter().enumerate().skip(i) {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((cs[i + 1..j].iter().collect(), j + 1));
                    }
                }
                _ => {}
            }
        }
        None
    };
    while i < cs.len() {
        let c = cs[i];
        if c == '$' {
            in_math = !in_math;
            out.push(c);
            i += 1;
            continue;
        }
        if in_math {
            out.push(c);
            i += 1;
            continue;
        }
        if c == '\\' {
            let name: String = cs[i + 1..].iter().take_while(|c| c.is_ascii_alphabetic()).collect();
            let after = i + 1 + name.len();
            let wrap = |pre: &str, post: &str, out: &mut String| -> Option<usize> {
                let (a, end) = arg(after)?;
                out.push_str(pre);
                out.push_str(&latex_text(&a));
                out.push_str(post);
                Some(end)
            };
            let next = match name.as_str() {
                "texttt" | "verb" => wrap("`", "`", &mut out),
                "textbf" => wrap("**", "**", &mut out),
                "emph" | "textit" => wrap("*", "*", &mut out),
                "text" | "textrm" | "textsf" | "mbox" | "url" => wrap("", "", &mut out),
                "ldots" | "dots" => {
                    out.push('…');
                    Some(after)
                }
                "" => {
                    // \, \; \% \& \_ \# \$ \{ \}
                    let n = cs.get(i + 1).copied().unwrap_or(' ');
                    match n {
                        ',' | ';' | ' ' | '!' => out.push(' '),
                        '%' | '&' | '_' | '#' | '{' | '}' => out.push(n),
                        '$' => out.push_str("\\$"),
                        _ => {
                            out.push('\\');
                            out.push(n);
                        }
                    }
                    Some(i + 2)
                }
                _ => None,
            };
            match next {
                Some(j) => i = j,
                None => {
                    // unknown command: keep its name readable, drop the backslash
                    out.push_str(&name);
                    i = after;
                }
            }
            continue;
        }
        if c == '~' {
            out.push(' ');
            i += 1;
            continue;
        }
        if c == '-' && cs.get(i + 1) == Some(&'-') {
            let three = cs.get(i + 2) == Some(&'-');
            out.push(if three { '—' } else { '–' });
            i += if three { 3 } else { 2 };
            continue;
        }
        if (c == '`' && cs.get(i + 1) == Some(&'`')) || (c == '\'' && cs.get(i + 1) == Some(&'\'')) {
            out.push(if c == '`' { '“' } else { '”' });
            i += 2;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn tex_line(ui: &mut egui::Ui, kb: &Kb, math: &mut Math, s: &str, size: f32, color: egui::Color32, bold: bool, bullet: Option<(&'static str, egui::Color32)>) {
    let s = &latex_text(s);
    let spans = rich::inline(s, &|_| None, &rich::Side::default(), &mut || "c".into());
    let l = rich::Line { kind: rich::Kind::Plain, front: spans, back: vec![] };
    crate::render::line(ui, kb, math, &l, &crate::render::Style { size, color, bold, bullet });
}

pub fn quiz(ui: &mut egui::Ui, kb: &Kb, math: &mut Math, path: &Path, src: &str, show_answers: bool) {
    let Ok(v) = serde_json::from_str::<Value>(src) else {
        ui.colored_label(RED_BRIGHT, "not valid JSON");
        return;
    };
    let answers = if show_answers { answers_for(path) } else { HashMap::new() };
    let size = NOTE_TEXT - 1.0;
    ui.label(RichText::new(v["title"].as_str().unwrap_or("quiz")).color(FG_BRIGHT).family(FontFamily::Name("bold".into())).size(17.0));
    ui.label(RichText::new(format!("{} · {}", v["week_title"].as_str().unwrap_or(""), v["source"].as_str().unwrap_or(""))).color(COMMENT).size(12.0));
    ui.label(RichText::new(if show_answers { "a hides answers" } else { "a shows answers" }).color(RED).size(12.0));
    ui.add_space(10.0);
    for q in v["questions"].as_array().into_iter().flatten() {
        let qid = q["id"].as_str().unwrap_or("");
        let flags: Vec<&str> = q["flags"].as_array().into_iter().flatten().filter_map(|f| f.as_str()).collect();
        let head = format!(
            "Q{} · {} · {} mark{}{}",
            q["number"].as_u64().unwrap_or(0),
            q["type"].as_str().unwrap_or(""),
            q["marks"],
            if q["marks"].as_f64() == Some(1.0) { "" } else { "s" },
            if flags.is_empty() { String::new() } else { format!(" · {}", flags.join(", ")) }
        );
        ui.label(RichText::new(head).color(RED).size(12.0).family(FontFamily::Name("bold".into())));
        tex_line(ui, kb, math, q["stem"].as_str().unwrap_or(""), size, FG_BRIGHT, false, None);
        if let Some(fig) = q["figure"].as_str() {
            match math.get(ui.ctx(), fig, size * crate::math::KATEX_SCALE, FG) {
                Some(t) => {
                    let (r, _) = ui.allocate_exact_size(t.size, egui::Sense::hover());
                    ui.painter().image(t.tex.id(), r, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)), egui::Color32::WHITE);
                }
                None => {
                    ui.label(RichText::new(fig).monospace().color(FG_DIM).size(size - 2.0));
                }
            }
        }
        let picked: Vec<String> = answers
            .get(qid)
            .into_iter()
            .flatten()
            .flat_map(|(_, a)| a["options"].as_array().into_iter().flatten().filter_map(|o| o.as_str().map(str::to_owned)))
            .collect();
        for o in q["options"].as_array().into_iter().flatten() {
            let id = o["id"].as_str().unwrap_or("?");
            let right = picked.iter().any(|p| p == id);
            ui.horizontal_top(|ui| {
                ui.add_space(12.0);
                let col = if right { RED_BRIGHT } else { FG_DIM };
                ui.label(RichText::new(format!("{id})")).color(col).size(size));
                ui.add_space(4.0);
                tex_line(ui, kb, math, o["text"].as_str().unwrap_or(""), size, if right { FG_BRIGHT } else { FG }, right, None);
            });
        }
        if show_answers {
            match answers.get(qid) {
                Some(list) => {
                    for (src, a) in list {
                        let mut what = vec![];
                        if let Some(os) = a["options"].as_array() {
                            what.push(os.iter().filter_map(|o| o.as_str()).collect::<Vec<_>>().join(", "));
                        }
                        for k in ["value", "answer", "text"] {
                            if let Some(x) = a.get(k).filter(|x| !x.is_null()) {
                                what.push(x.as_str().map(str::to_owned).unwrap_or_else(|| x.to_string()));
                            }
                        }
                        let conf = a["confidence"].as_str().map(|c| format!(" · {c} confidence")).unwrap_or_default();
                        ui.label(RichText::new(format!("answer ({src}{conf}): {}", what.join(" · "))).color(RED_BRIGHT).size(12.5));
                        if let Some(ex) = a["explanation"].as_str() {
                            tex_line(ui, kb, math, ex, size - 1.0, FG_DIM, false, None);
                        }
                    }
                }
                None => {
                    ui.label(RichText::new("no answer on file").color(COMMENT).size(12.0));
                }
            }
        }
        if let Some(n) = q["source_notes"].as_str().filter(|_| show_answers) {
            ui.label(RichText::new(n).color(COMMENT).size(11.5).italics());
        }
        ui.add_space(14.0);
    }
}

/// `[text](target)` -> link spans (Ref with id "link:<target>" and a label),
/// the rest through the normal inline parser.
fn md_spans(s: &str) -> Vec<rich::Span> {
    let mut out = vec![];
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let (Some(close), true) = (after.find("]("), true) else { break };
        let Some(end) = after[close + 2..].find(')') else { break };
        let label = &after[..close];
        let target = &after[close + 2..close + 2 + end];
        out.extend(rich::inline(&rest[..open], &|_| None, &rich::Side::default(), &mut || "c".into()));
        let mut extra = serde_json::Map::new();
        extra.insert("label".into(), Value::String(label.to_owned()));
        out.push(rich::Span::Ref { id: format!("link:{target}"), cloze: None, extra });
        rest = &after[close + 2 + end + 1..];
    }
    out.extend(rich::inline(rest, &|_| None, &rich::Side::default(), &mut || "c".into()));
    out
}

/// Markdown. Returns the on-screen rects of links with their targets.
pub fn markdown(ui: &mut egui::Ui, kb: &Kb, math: &mut Math, src: &str) -> Vec<(egui::Rect, String)> {
    let mut links: Vec<(egui::Rect, String)> = vec![];
    let keep = |v: Vec<(egui::Rect, String)>, links: &mut Vec<(egui::Rect, String)>| {
        links.extend(v.into_iter().filter_map(|(r, id)| id.strip_prefix("link:").map(|t| (r, t.to_owned()))));
    };
    let mut code: Option<String> = None;
    for raw in src.lines() {
        if raw.trim_start().starts_with("```") {
            match code.take() {
                Some(c) => {
                    egui::Frame::new().fill(BG_ALT).stroke((1.0, GREY0)).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(RichText::new(c.trim_end()).monospace().size(NOTE_TEXT - 2.5).color(FG)).wrap());
                    });
                }
                None => code = Some(String::new()),
            }
            continue;
        }
        if let Some(c) = &mut code {
            c.push_str(raw);
            c.push('\n');
            continue;
        }
        if raw.trim_start().starts_with("<!--") {
            continue; // html comments (generator notes)
        }
        let line = raw.replace("$$", "$");
        let t = line.trim_start();
        if t.is_empty() {
            ui.add_space(6.0);
            continue;
        }
        let depth = (line.len() - t.len()) / 2;
        if let Some(h) = t.strip_prefix("# ").or(t.strip_prefix("## ")).or(t.strip_prefix("### ")).or(t.strip_prefix("#### ")) {
            let size = match t.chars().take_while(|c| *c == '#').count() {
                1 => 20.0,
                2 => 17.5,
                _ => 15.5,
            };
            ui.add_space(6.0);
            let l = rich::Line { kind: rich::Kind::Plain, front: md_spans(h), back: vec![] };
            let r = crate::render::line(ui, kb, math, &l, &crate::render::Style { size, color: FG_BRIGHT, bold: true, bullet: None });
            keep(r, &mut links);
            continue;
        }
        if t.starts_with("|") {
            ui.label(RichText::new(t).monospace().size(NOTE_TEXT - 3.0).color(FG_DIM));
            continue;
        }
        let bullet = t.strip_prefix("- ").or(t.strip_prefix("* "));
        let has_link = t.contains("](");
        ui.horizontal_top(|ui| {
            ui.add_space(depth as f32 * 16.0);
            match bullet {
                Some(b) if has_link => {
                    let l = rich::Line { kind: rich::Kind::Plain, front: md_spans(b), back: vec![] };
                    let r = crate::render::line(ui, kb, math, &l, &crate::render::Style { size: NOTE_TEXT - 1.0, color: FG, bold: false, bullet: Some(("•", if depth == 0 { RED } else { COMMENT })) });
                    keep(r, &mut links);
                }
                // deck syntax bullets (Claude's decks) show their card markers
                Some(b) => {
                    let mut l = rich::text_to_line(b, &|_| None, &rich::Side::default(), &mut || "c".into());
                    let bold = b.starts_with("**") && b.trim_end().ends_with("**") && l.kind == rich::Kind::Plain;
                    if bold {
                        l = rich::Line { kind: rich::Kind::Plain, front: rich::inline(b.trim().trim_matches('*'), &|_| None, &rich::Side::default(), &mut || "c".into()), back: vec![] };
                    }
                    crate::render::line(ui, kb, math, &l, &crate::render::Style { size: NOTE_TEXT - 1.0, color: FG, bold, bullet: Some(("•", if depth == 0 { RED } else { COMMENT })) });
                }
                None if has_link => {
                    let l = rich::Line { kind: rich::Kind::Plain, front: md_spans(t), back: vec![] };
                    let r = crate::render::line(ui, kb, math, &l, &crate::render::Style { size: NOTE_TEXT - 1.0, color: FG, bold: false, bullet: None });
                    keep(r, &mut links);
                }
                None => tex_line(ui, kb, math, t, NOTE_TEXT - 1.0, FG, false, None),
            }
        });
    }
    links
}

pub fn json(ui: &mut egui::Ui, src: &str) {
    let pretty = serde_json::from_str::<Value>(src).ok().and_then(|v| serde_json::to_string_pretty(&v).ok()).unwrap_or_else(|| src.to_owned());
    ui.add(egui::Label::new(RichText::new(pretty).monospace().size(NOTE_TEXT - 3.0).color(FG)).wrap());
}

pub fn text(ui: &mut egui::Ui, src: &str) {
    ui.add(egui::Label::new(RichText::new(src).monospace().size(NOTE_TEXT - 2.5).color(FG)).wrap());
}

pub fn image(ui: &mut egui::Ui, cache: &mut HashMap<PathBuf, Option<TextureHandle>>, path: &Path) {
    let t = cache
        .entry(path.to_owned())
        .or_insert_with(|| {
            let img = image::open(path).ok()?.to_rgba8();
            let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
            Some(ui.ctx().load_texture(format!("img:{}", path.display()), ci, egui::TextureOptions::LINEAR))
        })
        .clone();
    match t {
        Some(t) => {
            let s = t.size_vec2();
            let w = ui.available_width().min(s.x);
            ui.image((t.id(), Vec2::new(w, s.y * w / s.x.max(1.0))));
        }
        None => {
            ui.colored_label(RED_BRIGHT, "can't open image");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::latex_text;

    #[test]
    fn latex_text_mode() {
        assert_eq!(latex_text(r"the address \texttt{127.0.0.1}?"), "the address `127.0.0.1`?");
        assert_eq!(latex_text(r"26.855\,ms and 50\% --- ok"), "26.855 ms and 50% — ok");
        assert_eq!(latex_text(r"``quoted'' and $\frac{1}{2}\,x$"), "“quoted” and $\\frac{1}{2}\\,x$");
        assert_eq!(latex_text(r"\emph{this} \textbf{bold}"), "*this* **bold**");
    }
}
