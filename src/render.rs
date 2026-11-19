//! Drawing a rem line: styled spans, inline KaTeX, refs, card markers.

use crate::kb::Kb;
use crate::math::Math;
use crate::rich::{Kind, Line, Span};
use crate::theme::*;
use eframe::egui::{self, Color32, FontFamily, Rect, RichText, Sense, Vec2};

pub struct Style {
    pub size: f32,
    pub color: Color32,
    pub bold: bool,
}

/// Draw `line` into a wrapping row. Returns the rects of [[refs]] (for clicks).
pub fn line(ui: &mut egui::Ui, kb: &Kb, math: &mut Math, l: &Line, st: &Style) -> Vec<(Rect, String)> {
    let mut refs = vec![];
    if l.kind == Kind::Code {
        code_block(ui, kb, l, st, &mut refs);
        return refs;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        spans(ui, kb, math, &l.front, st, &mut refs);
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
            ui.label(RichText::new(format!(" {m} ")).size(st.size).color(RED));
            let back = Style { size: st.size, color: FG_DIM.lerp_to_gamma(FG, 0.6), bold: false };
            spans(ui, kb, math, &l.back, &back, &mut refs);
        }
    });
    refs
}

fn code_block(ui: &mut egui::Ui, kb: &Kb, l: &Line, st: &Style, refs: &mut Vec<(Rect, String)>) {
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
    let _ = refs;
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

fn spans(ui: &mut egui::Ui, kb: &Kb, math: &mut Math, sp: &[Span], st: &Style, refs: &mut Vec<(Rect, String)>) {
    for s in sp {
        match s {
            Span::Text { t, b, i, u, code, cloze, extra } => {
                let sup = extra.contains_key("sup");
                let sub = extra.contains_key("sub");
                // one label per word so lines wrap naturally
                for w in t.split_inclusive(' ') {
                    let mut r = RichText::new(w).size(if sup || sub { st.size * 0.7 } else { st.size }).color(st.color);
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
                    ui.add(egui::Label::new(r).selectable(false));
                }
            }
            Span::Latex { tex, cloze, .. } => {
                let col = if cloze.is_some() { FG_BRIGHT } else { st.color };
                match math.get(ui.ctx(), tex, st.size, col) {
                    Some((tex_h, size)) => {
                        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                        if cloze.is_some() {
                            ui.painter().rect_filled(rect, 0.0, CRIMSON);
                        }
                        ui.painter().image(tex_h.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    None => {
                        ui.add(egui::Label::new(RichText::new(format!("${tex}$")).monospace().size(st.size - 1.0).color(RED_BRIGHT)).selectable(false));
                    }
                }
            }
            Span::Ref { id, cloze, .. } => {
                let name = kb.name(id);
                let name = if name.is_empty() { "(empty)".to_owned() } else { name };
                let mut r = RichText::new(name).size(st.size).color(RED_BRIGHT).underline();
                if cloze.is_some() {
                    r = r.background_color(CRIMSON);
                }
                let resp = ui.add(egui::Label::new(r).selectable(false));
                refs.push((resp.rect, id.clone()));
            }
            Span::Opaque(v) => {
                let what = if v.get("i").and_then(|x| x.as_str()) == Some("i") { "[image]" } else { "[object]" };
                ui.add(egui::Label::new(RichText::new(what).size(st.size - 1.0).color(COMMENT)).selectable(false));
            }
        }
    }
}
