//! Drawing a rem line: styled spans, inline KaTeX, refs, card markers.
//!
//! Own line layout instead of egui's wrapping layout: every word and formula
//! is measured (width, ascent, descent), wrapped greedily, and each wrapped
//! row is as tall as its own tallest item, with everything on that row's
//! baseline. So one tall fraction only makes its own row taller.

use crate::kb::Kb;
use crate::math::{Math, Tex, KATEX_SCALE};
use crate::rich::{Kind, Line, Span};
use crate::theme::*;
use eframe::egui::{self, Color32, FontFamily, Galley, Pos2, Rect, RichText, Sense, Vec2, WidgetText};
use std::sync::Arc;

pub struct Style {
    pub size: f32,
    pub color: Color32,
    pub bold: bool,
    /// bullet glyph drawn on the first row's baseline; wrapped rows indent past it
    pub bullet: Option<(&'static str, Color32)>,
}

enum Look {
    Text(Arc<Galley>),
    Tex(Tex, Option<Color32>),
}

struct Piece {
    look: Look,
    w: f32,
    asc: f32,
    desc: f32,
    re: Option<String>,
}

fn text_piece(ui: &egui::Ui, t: RichText, re: Option<String>) -> Piece {
    let g = WidgetText::from(t).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::FontSelection::Default);
    let base = g.rows.first().and_then(|r| r.glyphs.first()).map(|g| g.pos.y).unwrap_or(g.size().y * 0.8);
    let s = g.size();
    Piece { w: s.x, asc: base, desc: s.y - base, look: Look::Text(g), re }
}

fn pieces(ui: &egui::Ui, kb: &Kb, math: &mut Math, sp: &[Span], st: &Style, out: &mut Vec<Piece>) {
    for s in sp {
        match s {
            Span::Text { t, b, i, u, code, cloze, extra } => {
                let small = extra.contains_key("sup") || extra.contains_key("sub");
                for w in t.split_inclusive(' ') {
                    let mut r = RichText::new(w).size(if small { st.size * 0.7 } else { st.size }).color(st.color);
                    if *b || st.bold {
                        r = r.family(FontFamily::Name("bold".into())).color(FG_BRIGHT);
                    }
                    if *i {
                        r = r.italics();
                    }
                    if *u {
                        r = r.underline();
                    }
                    if *code {
                        r = r.monospace().size(st.size - 1.0).color(FG_BRIGHT).background_color(GREY0);
                    }
                    if cloze.is_some() {
                        r = r.color(FG_BRIGHT).background_color(CRIMSON);
                    }
                    out.push(text_piece(ui, r, None));
                }
            }
            Span::Latex { tex, cloze, .. } => {
                let col = if cloze.is_some() { FG_BRIGHT } else { st.color };
                match math.get(ui.ctx(), tex, st.size * KATEX_SCALE, col) {
                    Some(t) => out.push(Piece { w: t.size.x, asc: t.ascent, desc: t.size.y - t.ascent, look: Look::Tex(t, cloze.as_ref().map(|_| CRIMSON)), re: None }),
                    None => out.push(text_piece(ui, RichText::new(format!("${tex}$")).monospace().size(st.size - 1.0).color(RED_BRIGHT), None)),
                }
            }
            Span::Ref { id, cloze, extra } => {
                // markdown links ride on Ref spans with their label in `extra`
                let name = extra.get("label").and_then(|l| l.as_str()).map(str::to_owned).unwrap_or_else(|| kb.name(id));
                let name = if name.is_empty() { "(empty)".to_owned() } else { name };
                for w in name.split_inclusive(' ') {
                    let mut r = RichText::new(w).size(st.size).color(RED_BRIGHT).underline();
                    if cloze.is_some() {
                        r = r.background_color(CRIMSON);
                    }
                    out.push(text_piece(ui, r, Some(id.clone())));
                }
            }
            Span::Opaque(v) => {
                let what = if v.get("i").and_then(|x| x.as_str()) == Some("i") { "[image] " } else { "[object] " };
                out.push(text_piece(ui, RichText::new(what).size(st.size - 1.0).color(COMMENT), None));
            }
        }
    }
}

/// Draw `line`. Returns the rects of [[refs]] (for clicks).
pub fn line(ui: &mut egui::Ui, kb: &Kb, math: &mut Math, l: &Line, st: &Style) -> Vec<(Rect, String)> {
    if l.kind == Kind::Code {
        code_block(ui, kb, l, st);
        return vec![];
    }
    let mut ps: Vec<Piece> = vec![];
    pieces(ui, kb, math, &l.front, st, &mut ps);
    let marker = match l.kind {
        Kind::Concept => Some("::"),
        Kind::Descriptor => Some(";;"),
        Kind::Basic => Some(">>"),
        Kind::Both => Some("<>"),
        Kind::MultiLine => Some(">>>"),
        Kind::Mcq => Some(">>A)"),
        _ => None,
    };
    if let Some(m) = marker {
        ps.push(text_piece(ui, RichText::new(format!(" {m} ")).size(st.size).color(RED), None));
        let back = Style { size: st.size, color: FG_DIM.lerp_to_gamma(FG, 0.6), bold: false, bullet: None };
        pieces(ui, kb, math, &l.back, &back, &mut ps);
    }
    if ps.is_empty() {
        ps.push(text_piece(ui, RichText::new(" ").size(st.size), None));
    }
    let bullet = st.bullet.map(|(b, c)| text_piece(ui, RichText::new(format!("{b}  ")).size(st.size - 2.0).color(c), None));
    let indent = bullet.as_ref().map_or(0.0, |b| b.w);
    let width = (ui.available_width() - indent).max(40.0);

    let mut rows: Vec<Vec<usize>> = vec![vec![]];
    let mut x = 0.0;
    for (i, p) in ps.iter().enumerate() {
        if x + p.w > width && !rows.last().unwrap().is_empty() {
            rows.push(vec![]);
            x = 0.0;
        }
        rows.last_mut().unwrap().push(i);
        x += p.w;
    }
    let gap = 2.0;
    let metrics: Vec<(f32, f32)> = rows
        .iter()
        .enumerate()
        .map(|(ri, r)| {
            let mut asc = r.iter().map(|&i| ps[i].asc).fold(0.0, f32::max);
            let mut desc = r.iter().map(|&i| ps[i].desc).fold(0.0, f32::max);
            if ri == 0 {
                if let Some(b) = &bullet {
                    asc = asc.max(b.asc);
                    desc = desc.max(b.desc);
                }
            }
            (asc, desc)
        })
        .collect();
    let total: f32 = metrics.iter().map(|(a, d)| a + d).sum::<f32>() + gap * (rows.len() as f32 - 1.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(indent + width, total), Sense::hover());
    let p = ui.painter();
    let mut refs = vec![];
    let mut y = rect.top();
    for (ri, r) in rows.iter().enumerate() {
        let (asc, desc) = metrics[ri];
        let base = y + asc;
        if ri == 0 {
            if let Some(b) = &bullet {
                if let Look::Text(g) = &b.look {
                    p.galley(Pos2::new(rect.left(), base - b.asc), g.clone(), FG);
                }
            }
        }
        let mut x = rect.left() + indent;
        for &i in r {
            let pc = &ps[i];
            let top = base - pc.asc;
            match &pc.look {
                Look::Text(g) => p.galley(Pos2::new(x, top), g.clone(), FG),
                Look::Tex(t, bg) => {
                    let tr = Rect::from_min_size(Pos2::new(x, top), t.size);
                    if let Some(c) = bg {
                        p.rect_filled(tr, 0.0, *c);
                    }
                    p.image(t.tex.id(), tr, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
            }
            if let Some(id) = &pc.re {
                refs.push((Rect::from_min_size(Pos2::new(x, top), Vec2::new(pc.w, pc.asc + pc.desc)), id.clone()));
            }
            x += pc.w;
        }
        y += asc + desc + gap;
    }
    refs
}

fn code_block(ui: &mut egui::Ui, kb: &Kb, l: &Line, st: &Style) {
    let text: String = l
        .front
        .iter()
        .map(|s| match s {
            Span::Text { t, .. } => t.clone(),
            Span::Ref { id, .. } => kb.name(id),
            Span::Latex { tex, .. } => tex.clone(),
            Span::Opaque(_) => "[image]".into(),
        })
        .collect();
    ui.vertical(|ui| {
        egui::Frame::new().fill(BG_ALT).stroke((1.0, GREY0)).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(text.trim_end()).monospace().size(st.size - 1.5).color(FG)).wrap().selectable(false));
        });
        if !l.back.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                ui.label(RichText::new(">> ").size(st.size).color(RED));
                let plain: String = crate::rich::plain(&l.back, &|id| kb.name(id));
                ui.label(RichText::new(plain).size(st.size).color(FG_DIM.lerp_to_gamma(FG, 0.6)));
            });
        }
    });
}
