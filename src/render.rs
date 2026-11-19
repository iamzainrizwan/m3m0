//! Drawing a rem line: styled spans, inline KaTeX, refs, card markers.

use crate::kb::Kb;
use crate::math::Math;
use crate::rich::{Kind, Line, Span};
use crate::theme::*;
use crate::math::KATEX_SCALE;
use eframe::egui::{self, Align, Color32, FontFamily, Layout, Rect, RichText, Sense, Vec2, WidgetText};

pub struct Style {
    pub size: f32,
    pub color: Color32,
    pub bold: bool,
    /// bullet glyph drawn on the first line's baseline
    pub bullet: Option<(&'static str, Color32)>,
}

/// Draw `line` into a wrapping row. Returns the rects of [[refs]] (for clicks).
pub fn line(ui: &mut egui::Ui, kb: &Kb, math: &mut Math, l: &Line, st: &Style) -> Vec<(Rect, String)> {
    let mut refs = vec![];
    if l.kind == Kind::Code {
        code_block(ui, kb, l, st, &mut refs);
        return refs;
    }
    let m = metrics(ui, math, l, st);
    // bullet in its own column (on the first line's baseline) so wrapped lines
    // indent under the text, not under the bullet
    if let Some((b, c)) = st.bullet {
        ui.spacing_mut().item_spacing.x = 0.0;
        word(ui, RichText::new(format!("{b}  ")).size(st.size - 2.0).color(c), &m);
    }
    ui.with_layout(Layout::left_to_right(Align::Min).with_main_wrap(true), |ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        spans(ui, kb, math, &l.front, st, &m, &mut refs);
        let marker = match l.kind {
            Kind::Concept => Some("::"),
            Kind::Descriptor => Some(";;"),
            Kind::Basic => Some(">>"),
            Kind::Both => Some("<>"),
            Kind::MultiLine => Some(">>>"),
            Kind::Mcq => Some(">>A)"),
            _ => None,
        };
        let m_ = m;
        if let Some(m) = marker {
            word(ui, RichText::new(format!(" {m} ")).size(st.size).color(RED), &m_);
            let back = Style { size: st.size, color: FG_DIM.lerp_to_gamma(FG, 0.6), bold: false, bullet: None };
            spans(ui, kb, math, &l.back, &back, &m_, &mut refs);
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

/// Vertical metrics shared by every item of one rem so text and formulas share a
/// baseline: each item is `height` tall with its baseline `baseline` from the top.
#[derive(Clone, Copy)]
struct Metrics {
    baseline: f32,
    height: f32,
}

fn metrics(ui: &egui::Ui, math: &mut Math, l: &Line, st: &Style) -> Metrics {
    let g = WidgetText::from(RichText::new("x").size(st.size)).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::FontSelection::Default);
    let text_base = g.rows.first().and_then(|r| r.glyphs.first()).map(|g| g.pos.y).unwrap_or(st.size);
    let text_desc = g.size().y - text_base;
    let (mut asc, mut desc) = (text_base, text_desc);
    for s in l.front.iter().chain(&l.back) {
        if let Span::Latex { tex, .. } = s {
            if let Some(t) = math.get(ui.ctx(), tex, st.size * KATEX_SCALE, st.color) {
                asc = asc.max(t.ascent);
                desc = desc.max(t.size.y - t.ascent);
            }
        }
    }
    Metrics { baseline: asc, height: asc + desc }
}

/// A text run placed on the shared baseline.
fn word(ui: &mut egui::Ui, t: RichText, m: &Metrics) -> egui::Response {
    let g = WidgetText::from(t).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::FontSelection::Default);
    let base = g.rows.first().and_then(|r| r.glyphs.first()).map(|g| g.pos.y).unwrap_or(0.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(g.size().x, m.height), Sense::hover());
    ui.painter().galley(rect.min + Vec2::new(0.0, m.baseline - base), g, FG);
    resp
}

fn spans(ui: &mut egui::Ui, kb: &Kb, math: &mut Math, sp: &[Span], st: &Style, m: &Metrics, refs: &mut Vec<(Rect, String)>) {
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
                    word(ui, r, m);
                }
            }
            Span::Latex { tex, cloze, .. } => {
                let col = if cloze.is_some() { FG_BRIGHT } else { st.color };
                match math.get(ui.ctx(), tex, st.size * KATEX_SCALE, col) {
                    Some(t) => {
                        let (slot, _) = ui.allocate_exact_size(Vec2::new(t.size.x, m.height), Sense::hover());
                        let rect = Rect::from_min_size(slot.min + Vec2::new(0.0, m.baseline - t.ascent), t.size);
                        if cloze.is_some() {
                            ui.painter().rect_filled(rect, 0.0, CRIMSON);
                        }
                        ui.painter().image(t.tex.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    None => {
                        word(ui, RichText::new(format!("${tex}$")).monospace().size(st.size - 1.0).color(RED_BRIGHT), m);
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
                let resp = word(ui, r, m);
                refs.push((resp.rect, id.clone()));
            }
            Span::Opaque(v) => {
                let what = if v.get("i").and_then(|x| x.as_str()) == Some("i") { "[image]" } else { "[object]" };
                word(ui, RichText::new(what).size(st.size - 1.0).color(COMMENT), m);
            }
        }
    }
}
