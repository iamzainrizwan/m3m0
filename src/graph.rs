//! Graph view, modelled on uni-git/_tools/remnote_vault.py (what Zain used in
//! Obsidian): nodes are modules, weeks, headings, concepts and referenced rems
//! (plus year / semester hubs); edges link each node to its nearest node
//! descendants and to the [[refs]] in its body. One colour per module, a hue
//! family per semester. Force layout (d3-style: repulsion, springs, gravity,
//! cooling). Keyboard: hjkl moves the selection to the nearest node in that
//! direction, enter opens it, L local graph, f cycles module filter.

use crate::kb::Kb;
use crate::rich::Span;
use crate::theme::*;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};
use std::collections::{HashMap, HashSet, VecDeque};

const TERMS: [(&str, u8, u8, &str); 9] = [
    ("FC1", 1, 1, "#ff6b6b"),
    ("CS1", 1, 1, "#ffa94d"),
    ("ELA", 1, 1, "#ffd43b"),
    ("ISE", 1, 2, "#69db7c"),
    ("DBS", 1, 2, "#38d9a9"),
    ("DST", 1, 2, "#4dabf7"),
    ("FC2", 2, 1, "#b197fc"),
    ("INS", 2, 1, "#f783ac"),
    ("PEP", 2, 1, "#e599f7"),
];

fn hex(s: &str) -> Color32 {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap_or(0xffffff);
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

#[derive(Clone, Copy, PartialEq)]
pub enum NodeKind {
    Hub,
    Module,
    Week,
    Heading,
    Concept,
}

pub struct Node {
    pub id: String, // rem id, or "hub:..." for year/semester hubs
    pub label: String,
    pub color: Color32,
    pub kind: NodeKind,
    pub module: Option<String>,
    pub pos: Vec2,
    vel: Vec2,
    pub degree: usize,
}

pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<(usize, usize)>,
    pub index: HashMap<String, usize>,
    adj: Vec<Vec<usize>>,
    pub sel: Option<usize>,
    pub center: Vec2,
    pub scale: f32,
    pub filter: Option<String>, // module id
    pub local: Option<usize>,   // local graph around this node
    visible: Vec<bool>,
    alpha: f32,
    /// camera fits all visible nodes until the user pans/zooms
    pub fit: bool,
    /// camera follows the selected node
    pub follow: bool,
}

impl Graph {
    pub fn build(kb: &Kb, mods: &[String]) -> Graph {
        let modset: HashSet<&String> = mods.iter().collect();
        let module_of = |i: &str| -> Option<String> {
            let mut cur = i.to_owned();
            loop {
                if modset.contains(&cur) {
                    return Some(cur);
                }
                cur = kb.q.get(&cur)?.get("parent")?.as_str()?.to_owned();
            }
        };
        // referenced rems
        let mut refd: HashSet<String> = HashSet::new();
        for d in kb.q.values() {
            for f in ["key", "value"] {
                for s in crate::rich::parse_rt(d.get(f)) {
                    if let Span::Ref { id, .. } = s {
                        refd.insert(id);
                    }
                }
            }
        }
        let mut nodes: Vec<Node> = vec![];
        let mut index = HashMap::new();
        let colour = |m: &Option<String>| -> Color32 {
            m.as_ref().and_then(|m| TERMS.iter().find(|t| t.0 == kb.name(m))).map(|t| hex(t.3)).unwrap_or(FG_DIM)
        };
        let mut seed = 0x9e3779b97f4a7c15u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 40) as f32 / (1u64 << 24) as f32 - 0.5
        };
        for (i, d) in &kb.q {
            let Some(m) = module_of(i) else { continue };
            let name = kb.name(i);
            if name.is_empty() {
                continue;
            }
            let parent = d.get("parent").and_then(|p| p.as_str()).unwrap_or("");
            let kind = if modset.contains(i) {
                NodeKind::Module
            } else if modset.contains(&parent.to_owned()) {
                NodeKind::Week
            } else if kb.heading(i).is_some() {
                NodeKind::Heading
            } else if d.get("type").and_then(|t| t.as_i64()) == Some(1) || refd.contains(i) {
                NodeKind::Concept
            } else {
                continue;
            };
            let mo = Some(m);
            index.insert(i.clone(), nodes.len());
            let label: String = name.replace(['[', ']', '$'], "").chars().take(60).collect();
            nodes.push(Node { id: i.clone(), label, color: colour(&mo), kind, module: mo, pos: Vec2::new(rnd(), rnd()) * 800.0, vel: Vec2::ZERO, degree: 0 });
        }
        let mut edges: HashSet<(usize, usize)> = HashSet::new();
        // nearest node ancestor of every rem under a module
        let near = |start: &str| -> Option<usize> {
            let mut cur = kb.q.get(start)?.get("parent")?.as_str()?.to_owned();
            loop {
                if let Some(&n) = index.get(&cur) {
                    return Some(n);
                }
                cur = kb.q.get(&cur)?.get("parent")?.as_str()?.to_owned();
            }
        };
        for (i, d) in &kb.q {
            let owner = index.get(i).copied().or_else(|| near(i));
            let Some(owner) = owner else { continue };
            if let Some(&me) = index.get(i) {
                if let Some(p) = near(i) {
                    edges.insert((p.min(me), p.max(me)));
                }
            }
            for f in ["key", "value"] {
                for s in crate::rich::parse_rt(d.get(f)) {
                    if let Span::Ref { id, .. } = s {
                        if let Some(&t) = index.get(&id) {
                            if t != owner {
                                edges.insert((owner.min(t), owner.max(t)));
                            }
                        }
                    }
                }
            }
        }
        // year / semester hubs over the modules
        let mut hubs: HashMap<String, usize> = HashMap::new();
        for m in mods {
            let Some(&mi) = index.get(m) else { continue };
            let Some(t) = TERMS.iter().find(|t| t.0 == kb.name(m)) else { continue };
            let y = format!("Year {}", t.1);
            let s = format!("Year {} Semester {}", t.1, t.2);
            let mut hub = |label: &str, nodes: &mut Vec<Node>| -> usize {
                *hubs.entry(label.to_owned()).or_insert_with(|| {
                    nodes.push(Node { id: format!("hub:{label}"), label: label.to_owned(), color: FG_BRIGHT, kind: NodeKind::Hub, module: None, pos: Vec2::ZERO, vel: Vec2::ZERO, degree: 0 });
                    nodes.len() - 1
                })
            };
            let yi = hub(&y, &mut nodes);
            let si = hub(&s, &mut nodes);
            edges.insert((yi.min(si), yi.max(si)));
            edges.insert((si.min(mi), si.max(mi)));
        }
        let edges: Vec<(usize, usize)> = edges.into_iter().collect();
        let mut adj = vec![vec![]; nodes.len()];
        for &(a, b) in &edges {
            adj[a].push(b);
            adj[b].push(a);
            nodes[a].degree += 1;
            nodes[b].degree += 1;
        }
        // seed: modules on a ring (in term order), nodes near their module
        let order: Vec<String> = TERMS.iter().filter_map(|t| mods.iter().find(|m| kb.name(m) == t.0).cloned()).chain(mods.iter().filter(|m| !TERMS.iter().any(|t| t.0 == kb.name(m))).cloned()).collect();
        let home: HashMap<String, Vec2> = order
            .iter()
            .enumerate()
            .map(|(k, m)| {
                let a = k as f32 / order.len().max(1) as f32 * std::f32::consts::TAU;
                (m.clone(), Vec2::new(a.cos(), a.sin()) * 700.0)
            })
            .collect();
        for nd in nodes.iter_mut() {
            let h = nd.module.as_ref().and_then(|m| home.get(m)).copied().unwrap_or(Vec2::ZERO);
            nd.pos = h + nd.pos * 0.35;
        }
        let n = nodes.len();
        let mut g = Graph { nodes, edges, index, adj, sel: None, center: Vec2::ZERO, scale: 0.6, filter: None, local: None, visible: vec![true; n], alpha: 1.0, fit: true, follow: false };
        for _ in 0..120 {
            g.tick();
        }
        g
    }

    pub fn reheat(&mut self) {
        self.alpha = 1.0;
    }

    fn update_visible(&mut self) {
        let keep: Option<HashSet<usize>> = self.local.map(|c| {
            let mut seen = HashSet::from([c]);
            let mut q = VecDeque::from([(c, 0)]);
            while let Some((x, d)) = q.pop_front() {
                if d == 2 {
                    continue;
                }
                for &y in &self.adj[x] {
                    if seen.insert(y) {
                        q.push_back((y, d + 1));
                    }
                }
            }
            seen
        });
        for (i, n) in self.nodes.iter().enumerate() {
            let by_filter = self.filter.as_ref().is_none_or(|f| n.module.as_ref() == Some(f));
            let by_local = keep.as_ref().is_none_or(|k| k.contains(&i));
            self.visible[i] = by_filter && by_local;
        }
    }

    /// One simulation step over the visible nodes.
    fn tick(&mut self) {
        if self.alpha < 0.002 {
            return;
        }
        let vis: Vec<usize> = (0..self.nodes.len()).filter(|&i| self.visible[i]).collect();
        let mut force = vec![Vec2::ZERO; self.nodes.len()];
        // repulsion (all pairs; a few thousand nodes at most)
        for (a_i, &a) in vis.iter().enumerate() {
            for &b in &vis[a_i + 1..] {
                let d = self.nodes[a].pos - self.nodes[b].pos;
                let l2 = d.length_sq().max(25.0);
                if l2 > 350.0 * 350.0 {
                    continue; // local repulsion only: modules form clusters
                }
                let f = d * (900.0 / l2);
                force[a] += f;
                force[b] -= f;
            }
        }
        // springs
        for &(a, b) in &self.edges {
            if !self.visible[a] || !self.visible[b] {
                continue;
            }
            let d = self.nodes[b].pos - self.nodes[a].pos;
            let len = d.length().max(0.01);
            let rest = 60.0;
            let f = d / len * (len - rest) * 0.05;
            force[a] += f;
            force[b] -= f;
        }
        for &i in &vis {
            let n = &mut self.nodes[i];
            let g = -n.pos * 0.02; // gravity to the centre
            n.vel = (n.vel + (force[i] + g) * self.alpha) * 0.6;
            n.pos += n.vel;
        }
        self.alpha *= 0.985;
    }

    pub fn select_id(&mut self, id: &str) {
        if let Some(&i) = self.index.get(id) {
            self.sel = Some(i);
        }
    }

    /// Move the selection to the nearest visible node in direction `dir`.
    pub fn step(&mut self, dir: Vec2) {
        let from = match self.sel {
            Some(s) => self.nodes[s].pos,
            None => self.center,
        };
        let mut best: Option<(f32, usize)> = None;
        for (i, n) in self.nodes.iter().enumerate() {
            if !self.visible[i] || Some(i) == self.sel {
                continue;
            }
            let d = n.pos - from;
            let len = d.length().max(0.01);
            let cos = d.dot(dir) / len;
            if cos < 0.5 {
                continue;
            }
            let score = len * (2.0 - cos);
            if best.is_none_or(|(s, _)| score < s) {
                best = Some((score, i));
            }
        }
        if let Some((_, i)) = best {
            self.sel = Some(i);
            self.follow = true;
            self.fit = false;
        }
    }

    pub fn set_local(&mut self, on: bool) {
        self.local = if on { self.sel } else { None };
        self.fit = true;
        self.follow = false;
        self.reheat();
    }

    pub fn cycle_filter(&mut self, mods: &[String]) {
        let have: Vec<&String> = mods.iter().filter(|m| self.index.contains_key(*m)).collect();
        self.filter = match &self.filter {
            None => have.first().map(|s| (*s).clone()),
            Some(f) => have.iter().position(|m| *m == f).and_then(|i| have.get(i + 1)).map(|s| (*s).clone()),
        };
        self.fit = true;
        self.follow = false;
        self.reheat();
    }

    /// Draw and step. Returns a node the user clicked (to open).
    pub fn show(&mut self, ui: &mut egui::Ui, focused: bool) -> Option<String> {
        self.update_visible();
        let steps = if self.alpha > 0.3 { 8 } else { 3 };
        for _ in 0..steps {
            self.tick();
        }
        if self.alpha >= 0.002 {
            ui.ctx().request_repaint();
        }
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        if resp.dragged() {
            self.center -= resp.drag_delta() / self.scale;
            self.fit = false;
            self.follow = false;
        }
        if resp.hovered() {
            let z = ui.input(|i| i.smooth_scroll_delta.y);
            if z != 0.0 {
                self.scale = (self.scale * (1.0 + z * 0.002)).clamp(0.02, 6.0);
                self.fit = false;
            }
        }
        if self.fit {
            let pts: Vec<Vec2> = (0..self.nodes.len()).filter(|&i| self.visible[i]).map(|i| self.nodes[i].pos).collect();
            if !pts.is_empty() {
                let (mut lo, mut hi) = (pts[0], pts[0]);
                for p in &pts {
                    lo = lo.min(*p);
                    hi = hi.max(*p);
                }
                let size = (hi - lo).max(Vec2::splat(50.0));
                self.center = (lo + hi) / 2.0;
                self.scale = ((rect.width() - 80.0) / size.x).min((rect.height() - 80.0) / size.y).clamp(0.02, 3.0);
            }
        } else if let (true, Some(s)) = (self.follow, self.sel) {
            self.center += (self.nodes[s].pos - self.center) * 0.2;
            ui.ctx().request_repaint();
        }
        let to_screen = |p: Vec2| -> Pos2 { rect.center() + (p - self.center) * self.scale };
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, BG);
        for &(a, b) in &self.edges {
            if self.visible[a] && self.visible[b] {
                let hl = self.sel.is_some_and(|s| s == a || s == b);
                p.line_segment([to_screen(self.nodes[a].pos), to_screen(self.nodes[b].pos)], (if hl { 1.2 } else { 0.6 }, if hl { RED } else { GREY1 }));
            }
        }
        let hover = resp.hover_pos();
        let mut hovered: Option<usize> = None;
        for (i, n) in self.nodes.iter().enumerate() {
            if !self.visible[i] {
                continue;
            }
            let c = to_screen(n.pos);
            let r = ((n.degree as f32).sqrt() * 1.6 + 2.5).min(16.0) * self.scale.sqrt().clamp(0.5, 2.0);
            if hover.is_some_and(|h| h.distance(c) <= r + 3.0) {
                hovered = Some(i);
            }
            p.circle_filled(c, r, n.color);
            let sel = Some(i) == self.sel;
            if sel {
                p.circle_stroke(c, r + 3.0, (2.0, RED_BRIGHT));
            }
            let always = matches!(n.kind, NodeKind::Hub | NodeKind::Module) || (n.kind == NodeKind::Week && self.scale > 0.35);
            if always || sel || self.scale > 1.4 || hovered == Some(i) || (self.local.is_some() && self.scale > 0.6) {
                let col = if sel { FG_BRIGHT } else { FG_DIM };
                p.text(c + Vec2::new(0.0, r + 2.0), egui::Align2::CENTER_TOP, &n.label, egui::FontId::monospace(11.0), col);
            }
        }
        // legend / state
        let mut info = format!("{} nodes · {} edges", self.visible.iter().filter(|v| **v).count(), self.edges.len());
        if let Some(f) = &self.filter {
            if let Some(&i) = self.index.get(f) {
                info += &format!(" · {}", self.nodes[i].label);
            }
        }
        if self.local.is_some() {
            info += " · local";
        }
        p.text(rect.left_bottom() + Vec2::new(10.0, -10.0), egui::Align2::LEFT_BOTTOM, info, egui::FontId::monospace(12.0), COMMENT);
        let keys = if focused { "hjkl select · enter open · L local · f filter · +/- zoom · c follow · = fit · esc back" } else { "" };
        p.text(rect.right_bottom() + Vec2::new(-10.0, -10.0), egui::Align2::RIGHT_BOTTOM, keys, egui::FontId::monospace(12.0), COMMENT);
        if let (true, Some(i)) = (resp.clicked(), hovered) {
            if Some(i) == self.sel {
                return Some(self.nodes[i].id.clone());
            }
            self.sel = Some(i);
        }
        if resp.double_clicked() {
            return hovered.map(|i| self.nodes[i].id.clone());
        }
        let _ = Rect::NOTHING;
        None
    }
}
