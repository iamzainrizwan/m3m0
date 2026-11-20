//! Review (gr this week · gR this module · :review all): RemNote's queue of due
//! cards, graded through the bridge with `card.updateCardRepetitionStatus`, so
//! RemNote's own default scheduler computes the next interval and the phone
//! stays in sync. Due = card doc `n` (next repetition time) <= now.
//! Keys: space/enter reveal, 1 again 2 hard 3 good 4 easy, s skip, esc stop.

use super::*;
use crate::rich::{Kind, Line, Span};
use serde_json::{json, Value};

pub struct Item {
    pub card: String,
    pub rem: String,
    pub dir: String, // "f", "b" or a cloze id
    pub level: String,
}

pub struct Review {
    pub scope: String,
    pub queue: Vec<Item>,
    pub i: usize,
    pub revealed: bool,
    pub graded: usize,
    pub failed: Vec<String>,
    pub inflight: Vec<mpsc::Receiver<(String, Result<Value, String>)>>,
}

pub const SCORES: [(&str, f64); 4] = [("again", 0.0), ("hard", 0.5), ("good", 1.0), ("easy", 1.5)];

impl App {
    /// scope: a rem (week or module) or None for everything.
    pub(super) fn start_review(&mut self, d: &Loaded, scope: Option<String>) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|t| t.as_millis() as f64).unwrap_or(0.0);
        let set: Option<HashSet<String>> = scope.as_ref().map(|s| edit::subtree(&d.kb, s).into_iter().collect());
        let mut q: Vec<(f64, Item)> = d
            .kb
            .card_docs
            .iter()
            .filter_map(|c| {
                let rem = c.get("rId")?.as_str()?.to_owned();
                if set.as_ref().is_some_and(|s| !s.contains(&rem)) || !d.kb.q.contains_key(&rem) {
                    return None;
                }
                let n = c.get("n").and_then(Value::as_f64).unwrap_or(0.0);
                if n > now {
                    return None;
                }
                let dir = c.get("k").and_then(Value::as_str).unwrap_or("").rsplit('.').next().unwrap_or("f").to_owned();
                Some((n, Item { card: c.get("_id")?.as_str()?.to_owned(), rem, dir, level: c.get("ml").and_then(Value::as_str).unwrap_or("").to_owned() }))
            })
            .collect();
        q.sort_by(|a, b| a.0.total_cmp(&b.0));
        let label = scope.as_ref().map(|s| d.kb.name(s)).unwrap_or_else(|| "everything".into());
        if q.is_empty() {
            self.msg = Some(format!("nothing due in {label}"));
            return;
        }
        self.cards = None;
        self.view_graph = false;
        self.focus = Focus::Notes;
        self.review = Some(Review { scope: label, queue: q.into_iter().map(|x| x.1).collect(), i: 0, revealed: false, graded: 0, failed: vec![], inflight: vec![] });
    }

    pub(super) fn review_key(&mut self, k: K) -> bool {
        let Some(r) = self.review.as_mut() else { return false };
        match k {
            K::Ch(' ') | K::Enter if !r.revealed => r.revealed = true,
            K::Ch(c @ '1'..='4') if r.revealed => {
                let Some(it) = r.queue.get(r.i) else { return true };
                if !self.bridge.is_online() {
                    if !bridge::remnote_running() {
                        let _ = bridge::launch_remnote();
                        self.msg = Some("starting RemNote (the sync engine): grade again once the bridge is online".into());
                    } else {
                        self.msg = Some("bridge offline: restart RemNote so it loads the plugin from m3m0".into());
                    }
                    return true;
                }
                let score = SCORES[(c as u8 - b'1') as usize].1;
                let (card, b) = (it.card.clone(), self.bridge.clone());
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    let res = b.call("review", json!({ "card": card, "score": score }));
                    let _ = tx.send((card, res));
                });
                r.inflight.push(rx);
                self.graded.insert(it.card.clone());
                r.graded += 1;
                r.i += 1;
                r.revealed = false;
            }
            K::Ch('s') => {
                r.i += 1;
                r.revealed = false;
            }
            K::Esc | K::Ch('q') => {
                let msg = format!("reviewed {} cards{}", r.graded, if r.failed.is_empty() { String::new() } else { format!(", {} failed to save", r.failed.len()) });
                self.review = None;
                self.msg = Some(msg);
            }
            _ => {}
        }
        true
    }

    pub(super) fn review_ui(&mut self, ctx: &egui::Context, d: &Loaded) {
        let math = &mut self.math;
        let Some(r) = self.review.as_mut() else { return };
        r.inflight.retain(|rx| match rx.try_recv() {
            Ok((card, res)) => {
                if res.is_err() {
                    r.failed.push(card);
                }
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(_) => false,
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(28, 18))).show(ctx, |ui| {
            let left = r.queue.len().saturating_sub(r.i);
            ui.horizontal(|ui| {
                ui.label(RichText::new("review").color(RED_BRIGHT).family(bold()).size(18.0));
                ui.label(RichText::new(format!("   {} · {} left · {} graded{}", r.scope, left, r.graded, if r.failed.is_empty() { String::new() } else { format!(" · {} not saved", r.failed.len()) })).color(COMMENT).size(13.0));
            });
            // progress bar, dot-matrix style
            let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
            let frac = r.i as f32 / r.queue.len().max(1) as f32;
            ui.painter().rect_filled(bar, 0.0, GREY0);
            ui.painter().rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * frac, bar.height())), 0.0, RED);
            ui.add_space(24.0);
            let Some(it) = r.queue.get(r.i) else {
                ui.centered_and_justified(|ui| ui.label(RichText::new(format!("done: {} cards graded · esc to go back", r.graded)).color(FG).size(16.0)));
                return;
            };
            let l = d.kb.line_of(&it.rem);
            // where the card lives
            let mut crumbs = vec![];
            let mut cur = it.rem.clone();
            while let Some(p) = d.kb.q.get(&cur).and_then(|x| x.get("parent")).and_then(|p| p.as_str()).map(str::to_owned) {
                crumbs.push(d.kb.name(&p));
                cur = p;
            }
            crumbs.reverse();
            let width = ui.available_width().min(MEASURE);
            let pad = ((ui.available_width() - width) / 2.0).max(0.0);
            ui.horizontal_top(|ui| {
                ui.add_space(pad);
                ui.vertical(|ui| {
                    ui.set_width(width);
                    ui.label(RichText::new(crumbs.join("  /  ")).color(COMMENT).size(12.0));
                    ui.label(RichText::new(format!("{} · {}", it.level.to_lowercase(), dir_label(&it.dir, l.kind))).color(FG_DIM).size(12.0));
                    ui.add_space(14.0);
                    let (q, a) = sides(&l, &it.dir);
                    let big = render::Style { size: NOTE_TEXT + 3.0, color: FG_BRIGHT, bold: false, bullet: None };
                    render::line(ui, &d.kb, math, &q, &big);
                    ui.add_space(18.0);
                    let (sep, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
                    ui.painter().rect_filled(sep, 0.0, GREY1);
                    ui.add_space(18.0);
                    if r.revealed {
                        render::line(ui, &d.kb, math, &a, &render::Style { size: NOTE_TEXT + 2.0, color: FG, bold: false, bullet: None });
                        if l.kind == Kind::MultiLine && it.dir == "f" {
                            for c in d.kb.children(Some(&it.rem)) {
                                let cl = d.kb.line_of(c);
                                render::line(ui, &d.kb, math, &cl, &render::Style { size: NOTE_TEXT + 1.0, color: FG, bold: false, bullet: Some(("•", RED)) });
                            }
                        }
                        ui.add_space(24.0);
                        ui.horizontal(|ui| {
                            for (i, (name, _)) in SCORES.iter().enumerate() {
                                let col = if i == 0 { RED_BRIGHT } else { FG };
                                ui.label(RichText::new(format!(" {} ", i + 1)).background_color(GREY0).color(col).size(14.0));
                                ui.label(RichText::new(format!(" {name}     ")).color(FG_DIM).size(14.0));
                            }
                        });
                    } else {
                        ui.label(RichText::new("space to reveal").color(COMMENT).size(14.0));
                    }
                });
            });
        });
    }
}

fn dir_label(dir: &str, k: Kind) -> &'static str {
    match (dir, k) {
        ("f", Kind::MultiLine) => "list",
        ("f", _) => "forward",
        ("b", _) => "backward",
        _ => "cloze",
    }
}

/// (question, answer) lines for a card direction.
fn sides(l: &Line, dir: &str) -> (Line, Line) {
    let plain = |s: Vec<Span>| Line { kind: Kind::Plain, front: s, back: vec![] };
    match dir {
        "f" => (plain(l.front.clone()), plain(l.back.clone())),
        "b" => (plain(l.back.clone()), plain(l.front.clone())),
        cid => {
            let hide = |spans: &[Span], reveal: bool| -> Vec<Span> {
                spans
                    .iter()
                    .map(|s| match s {
                        Span::Text { cloze: Some(c), .. } | Span::Latex { cloze: Some(c), .. } | Span::Ref { cloze: Some(c), .. } if c == cid && !reveal => {
                            Span::Text { t: "[ … ]".into(), b: true, i: false, u: false, code: false, cloze: Some(c.clone()), extra: Default::default() }
                        }
                        Span::Text { t, b, i, u, code, cloze, extra } => {
                            let keep = cloze.as_ref().filter(|c| *c == cid && reveal).cloned();
                            Span::Text { t: t.clone(), b: *b, i: *i, u: *u, code: *code, cloze: keep, extra: extra.clone() }
                        }
                        Span::Latex { tex, cloze, extra } => Span::Latex { tex: tex.clone(), cloze: cloze.as_ref().filter(|c| *c == cid && reveal).cloned(), extra: extra.clone() },
                        Span::Ref { id, cloze, extra } => Span::Ref { id: id.clone(), cloze: cloze.as_ref().filter(|c| *c == cid && reveal).cloned(), extra: extra.clone() },
                        o => o.clone(),
                    })
                    .collect()
            };
            let mut q = hide(&l.front, false);
            let mut a = hide(&l.front, true);
            if !l.back.is_empty() {
                let sep = Span::Text { t: "  →  ".into(), b: false, i: false, u: false, code: false, cloze: None, extra: Default::default() };
                q.push(sep.clone());
                q.extend(hide(&l.back, false));
                a.push(sep);
                a.extend(hide(&l.back, true));
            }
            (plain(q), plain(a))
        }
    }
}
