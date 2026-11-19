//! Cards view (ctrl-f): Zain's cards for the open week next to Claude's deck,
//! paired by explicit links then similarity. Gaps on either side are marked.
//! Keys: h/l column, j/k move, enter open / jump to pair, gl link (picker),
//! gL unlink, s stage a Claude card into the notes, esc / ctrl-f back.

use super::*;
use crate::deck::{self, DeckCard};
use crate::edit::{LineJson, Op};

pub struct CardsView {
    pub week: String,
    pub deck: Vec<DeckCard>,
    pub files: Vec<std::path::PathBuf>,
    pub m: deck::Match,
    pub side: usize, // 0 mine, 1 claude
    pub cur: [usize; 2],
    pub scroll: [bool; 2],
}

pub struct LinkPick {
    pub rem: String,
    pub items: Vec<(usize, f32)>,
    pub cur: usize,
    /// what's being linked (your card's front), and each item's label
    /// (claude's front · section), kept here since the notes-side `gl` closes cards
    pub title: String,
    pub labels: Vec<String>,
}

impl App {
    pub(super) fn open_cards(&mut self, d: &Loaded) {
        let Some(w) = self.week.clone() else { return };
        let Some(module) = d.kb.q.get(&w).and_then(|x| x.get("parent")).and_then(|p| p.as_str()).filter(|p| d.mods.iter().any(|m| m == p)).map(|m| d.kb.name(m)) else {
            self.msg = Some("open a week first".into());
            return;
        };
        let n = pdf::week_number(&d.kb.name(&w)).unwrap_or(0);
        let files = deck::deck_files(&module, n);
        let deck: Vec<DeckCard> = files.iter().flat_map(|f| deck::parse(f)).collect();
        let mine = deck::my_cards(&d.kb, &w);
        let m = deck::pair(&d.kb, &mine, &deck, &self.links);
        let keep = self.cards.as_ref().filter(|c| c.week == w).map(|c| (c.side, c.cur));
        let (side, cur) = keep.unwrap_or((0, [0, 0]));
        self.cards = Some(CardsView { week: w, deck, files, m, side, cur, scroll: [true, true] });
        if self.cards.as_ref().is_some_and(|c| c.files.is_empty()) {
            self.msg = Some(format!("no Claude deck for this week (looked for uni-git/{}/revision/week-{n:02}-flashcards.md)", module.to_lowercase()));
        }
    }

    pub(super) fn link_picker(&mut self, d: &Loaded, rem: &str) {
        let Some(c) = &self.cards else { return };
        let l = d.kb.line_of(rem);
        let name = |id: &str| d.kb.name(id);
        let (f, b) = (rich::plain(&l.front, &name), rich::plain(&l.back, &name));
        let mut items: Vec<(usize, f32)> = c
            .deck
            .iter()
            .enumerate()
            .map(|(i, t)| (i, deck::similarity(&f, &b, &rich::plain(&t.line.front, &|_| String::new()), &rich::plain(&t.line.back, &|_| String::new()))))
            .collect();
        items.sort_by(|a, b| b.1.total_cmp(&a.1));
        let labels = items.iter().map(|&(i, _)| format!("{}   · {}", rich::plain(&c.deck[i].line.front, &|_| String::new()), c.deck[i].section)).collect();
        self.overlay = Overlay::Link(LinkPick { rem: rem.to_owned(), items, cur: 0, title: f, labels });
    }

    pub(super) fn do_link(&mut self, d: &Loaded, rem: &str, card: usize) {
        if let Some(c) = &self.cards {
            if let Some(t) = c.deck.get(card) {
                self.links.insert(rem.to_owned(), t.key.clone());
                deck::save_links(&self.links);
                self.msg = Some("linked".into());
            }
        }
        self.open_cards(d);
    }

    /// Stage a Claude card into the notes, under the heading that best matches its section.
    fn stage_card(&mut self, d: &mut Loaded, card: usize) {
        let Some(c) = &self.cards else { return };
        let Some(t) = c.deck.get(card).cloned() else { return };
        let week = c.week.clone();
        let mut best = (0.3f32, week.clone());
        let mut stack = vec![week.clone()];
        while let Some(i) = stack.pop() {
            for k in d.kb.children(Some(&i)) {
                if d.kb.heading(k).is_some() {
                    let s = deck::similarity(&d.kb.name(k), "", &t.section, "");
                    if s > best.0 {
                        best = (s, k.clone());
                    }
                }
                stack.push(k.clone());
            }
        }
        let parent = best.1;
        let pos = d.kb.children(Some(&parent)).len();
        let id = self.changes.tmp_id();
        self.changes.apply(&mut d.kb, Op::Create { id: id.clone(), parent, pos, line: LineJson::of(&t.line) });
        for (k, it) in t.items.iter().enumerate() {
            let cid = self.changes.tmp_id();
            let line = rich::text_to_line(it, &|_| None, &rich::Side::default(), &mut || "c".into());
            self.changes.apply(&mut d.kb, Op::Create { id: cid, parent: id.clone(), pos: k, line: LineJson::of(&line) });
        }
        if let Some(key) = Some(t.key.clone()) {
            self.links.insert(id.clone(), key);
            deck::save_links(&self.links);
        }
        self.msg = Some("staged into your notes (:w to commit)".into());
        self.open_cards(d);
    }

    pub(super) fn cards_key(&mut self, d: &mut Loaded, prefix: &str, k: K, n: usize) -> bool {
        let Some(c) = self.cards.as_mut() else { return false };
        let len = [c.m.mine.len(), c.deck.len()];
        let s = c.side;
        match (prefix, k) {
            ("g", K::Ch('g')) => {
                c.cur[s] = 0;
                c.scroll[s] = true;
            }
            ("g", K::Ch('l')) if s == 0 => {
                if let Some((rem, _)) = c.m.mine.get(c.cur[0]).cloned() {
                    self.link_picker(d, &rem);
                }
            }
            ("g", K::Ch('L')) if s == 0 => {
                if let Some((rem, _)) = c.m.mine.get(c.cur[0]).cloned() {
                    self.links.remove(&rem);
                    deck::save_links(&self.links);
                    self.open_cards(d);
                    self.msg = Some("unlinked".into());
                }
            }
            (_, K::Ch('g')) => self.pending.push('g'),
            (_, K::Ch('G')) => {
                c.cur[s] = len[s].saturating_sub(1);
                c.scroll[s] = true;
            }
            (_, K::Ch('h') | K::Left) => c.side = 0,
            (_, K::Ch('l') | K::Right) => c.side = 1,
            (_, K::Ch('j') | K::Down) => {
                c.cur[s] = (c.cur[s] + n).min(len[s].saturating_sub(1));
                c.scroll[s] = true;
            }
            (_, K::Ch('k') | K::Up) => {
                c.cur[s] = c.cur[s].saturating_sub(n);
                c.scroll[s] = true;
            }
            (_, K::Ctrl('d')) => {
                c.cur[s] = (c.cur[s] + 10 * n).min(len[s].saturating_sub(1));
                c.scroll[s] = true;
            }
            (_, K::Ctrl('u')) => {
                c.cur[s] = c.cur[s].saturating_sub(10 * n);
                c.scroll[s] = true;
            }
            (_, K::Ch(']')) => {
                // next gap on this side
                let gap = |i: usize| if s == 0 { c.m.mine[i].1.is_none() } else { c.m.theirs_free.contains(&i) };
                if let Some(i) = (c.cur[s] + 1..len[s]).find(|&i| gap(i)) {
                    c.cur[s] = i;
                    c.scroll[s] = true;
                }
            }
            (_, K::Ch('[')) => {
                let gap = |i: usize| if s == 0 { c.m.mine[i].1.is_none() } else { c.m.theirs_free.contains(&i) };
                if let Some(i) = (0..c.cur[s]).rev().find(|&i| gap(i)) {
                    c.cur[s] = i;
                    c.scroll[s] = true;
                }
            }
            (_, K::Enter) => {
                if s == 0 {
                    if let Some((rem, _)) = c.m.mine.get(c.cur[0]).cloned() {
                        self.cards = None;
                        self.goto(d, &rem);
                    }
                } else if let Some(i) = c.m.mine.iter().position(|(_, p)| p.is_some_and(|(t, _)| t == c.cur[1])) {
                    c.cur[0] = i;
                    c.side = 0;
                    c.scroll[0] = true;
                }
            }
            (_, K::Ch('s')) if s == 1 => {
                let i = c.cur[1];
                if c.m.theirs_free.contains(&i) {
                    self.stage_card(d, i);
                } else {
                    self.msg = Some("you already have this card (enter jumps to it)".into());
                }
            }
            (_, K::Esc | K::Ctrl('f')) => self.cards = None,
            _ => return false,
        }
        true
    }

    pub(super) fn cards_ui(&mut self, ctx: &egui::Context, d: &Loaded) {
        let focused = self.focus == Focus::Notes;
        let math = &mut self.math;
        let Some(c) = self.cards.as_mut() else { return };
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin { left: 16, right: 16, top: 12, bottom: 0 })).show(ctx, |ui| {
            let mine_gaps = c.m.mine.iter().filter(|m| m.1.is_none()).count();
            let theirs_gaps = c.m.theirs_free.len();
            ui.horizontal(|ui| {
                ui.label(RichText::new(d.kb.name(&c.week)).size(18.0).color(FG_BRIGHT).family(bold()));
                ui.label(RichText::new(format!("   {} yours · {} claude's · {} only yours · {} missing from your notes", c.m.mine.len(), c.deck.len(), mine_gaps, theirs_gaps)).size(12.5).color(COMMENT));
            });
            ui.add_space(8.0);
            let w = (ui.available_width() - 16.0) / 2.0;
            let h = ui.available_height() - 26.0;
            ui.horizontal_top(|ui| {
                for side in 0..2 {
                    ui.allocate_ui_with_layout(Vec2::new(w, h + 24.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_max_width(w);
                        let title = if side == 0 { "your cards" } else { "claude's deck" };
                        let active = focused && c.side == side;
                        ui.label(RichText::new(title).size(13.0).color(if active { RED_BRIGHT } else { FG_DIM }).family(bold()));
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical().id_salt(("cards", side)).max_height(h).max_width(w).auto_shrink(false).show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            let n = if side == 0 { c.m.mine.len() } else { c.deck.len() };
                            for i in 0..n {
                                let sel = c.cur[side] == i;
                                // state tag: only what's worth reading (auto-matched cards get none)
                                let (line, kids, state, section) = if side == 0 {
                                    let (rem, p) = &c.m.mine[i];
                                    let st = match p {
                                        Some((_, true)) => ("\u{f0c1} linked", RED),
                                        Some((_, false)) => ("", FG_DIM),
                                        None => ("only yours", COMMENT),
                                    };
                                    // a >>> card's answer is its children
                                    let kids: Vec<rich::Line> = if d.kb.is_multiline(rem) { d.kb.children(Some(rem)).iter().map(|k| d.kb.line_of(k)).collect() } else { vec![] };
                                    (d.kb.line_of(rem), kids, st, None)
                                } else {
                                    let t = &c.deck[i];
                                    let st = if c.m.theirs_free.contains(&i) { ("+ missing", RED_BRIGHT) } else { ("", FG_DIM) };
                                    let first_in_section = i == 0 || c.deck[i - 1].section != t.section;
                                    (t.line.clone(), t.item_lines.clone(), st, first_in_section.then(|| t.section.clone()))
                                };
                                if let Some(sec) = section {
                                    ui.add_space(6.0);
                                    ui.label(RichText::new(sec).size(13.0).color(FG_BRIGHT).family(bold()));
                                }
                                let gap = (side == 0 && c.m.mine[i].1.is_none()) || (side == 1 && c.m.theirs_free.contains(&i));
                                let resp = row(ui, ("card", side, i), sel, active, 0.0, |ui| {
                                    ui.vertical(|ui| {
                                        let st = render::Style { size: NOTE_TEXT - 1.0, color: if gap && side == 1 { FG_BRIGHT } else { FG }, bold: false, bullet: None };
                                        render::line(ui, &d.kb, math, &line, &st);
                                        for k in &kids {
                                            ui.horizontal_top(|ui| {
                                                ui.add_space(18.0);
                                                ui.vertical(|ui| {
                                                    let ks = render::Style { size: NOTE_TEXT - 2.0, color: FG, bold: false, bullet: Some(("•", RED)) };
                                                    render::line(ui, &d.kb, math, k, &ks);
                                                });
                                            });
                                        }
                                        if !state.0.is_empty() {
                                            ui.label(RichText::new(state.0).size(11.0).color(state.1));
                                        }
                                    });
                                });
                                if sel && c.scroll[side] {
                                    resp.scroll_to_me(None);
                                }
                                if resp.clicked() {
                                    c.side = side;
                                    c.cur[side] = i;
                                }
                            }
                        });
                    });
                    if side == 0 {
                        ui.add_space(16.0);
                    }
                }
            });
            c.scroll = [false, false];
            ui.label(RichText::new("h/l column · j/k move · ]/[ next/prev gap · enter open / jump to pair · gl link · gL unlink · s stage claude card · esc back").size(12.0).color(COMMENT));
        });
    }
}
