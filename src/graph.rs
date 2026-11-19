//! Graph view: how the notes connect.
//!
//! Model (built for m3m0, not the old Obsidian export):
//! - nodes: pages (the module weeks), terms (concept `::` rems) and anything
//!   that's [[referenced]];
//! - edges: a term -> the page it's defined on ("lives in"), and every [[ref]]
//!   from where it's used (the nearest node above the citing rem) -> its target
//!   ("uses"), weighted by how many times;
//! - look: dot-matrix squares like the logo. Pages are hollow white squares,
//!   terms grey squares sized by how often they're referenced, cross-module
//!   links red, module names dim over their clusters, selection red.
//!
//! Keys: hjkl select by direction, tab / s-tab walk the selection's
//! neighbours, enter open, / search, L local (2 hops), f module filter,
//! +/- zoom, = fit, c follow, r rebuild, esc back.

use crate::kb::Kb;
use crate::rich::Span;
use crate::theme::*;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Copy, PartialEq)]
pub enum NodeKind {
    Page,
    Term,
}

pub struct Node {
    pub id: String,
    pub label: String,
    pub kind: NodeKind,
    pub module: usize,
    pub page: Option<String>,
    pub pos: Vec2,
    vel: Vec2,
    pub refs_in: usize,
    /// where its module sits on the ring; gravity pulls here so modules form islands
    home: Vec2,
}

pub struct Edge {
    pub a: usize,
    pub b: usize,
    pub weight: usize,
    pub cross: bool,
}

pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub index: HashMap<String, usize>,
    pub modules: Vec<(String, String)>,
    adj: Vec<Vec<usize>>,
    pub sel: Option<usize>,
    walk: usize,
    pub center: Vec2,
    pub scale: f32,
    pub filter: Option<usize>,
    pub local: Option<usize>,
    visible: Vec<bool>,
    alpha: f32,
    pub fit: bool,
    pub follow: bool,
    t: f32,
}

impl Graph {
    pub fn build(kb: &Kb, mods: &[String]) -> Graph {
        let modules: Vec<(String, String)> = mods.iter().map(|m| (m.clone(), kb.name(m))).collect();
        let mod_ix: HashMap<&str, usize> = mods.iter().enumerate().map(|(i, m)| (m.as_str(), i)).collect();
        let mut page_of: HashMap<String, (usize, String)> = HashMap::new();
        for (mi, m) in mods.iter().enumerate() {
            for w in kb.children(Some(m)) {
                for r in crate::edit::subtree(kb, w) {
                    page_of.insert(r, (mi, w.clone()));
                }
            }
        }
        let mut cites: Vec<(String, String)> = vec![];
        let mut refd: HashMap<String, usize> = HashMap::new();
        for (i, d) in &kb.q {
            if !page_of.contains_key(i) {
                continue;
            }
            for f in ["key", "value"] {
                for s in crate::rich::parse_rt(d.get(f)) {
                    if let Span::Ref { id, .. } = s {
                        if id != *i {
                            *refd.entry(id.clone()).or_default() += 1;
                            cites.push((i.clone(), id));
                        }
                    }
                }
            }
        }
        let mut nodes: Vec<Node> = vec![];
        let mut index = HashMap::new();
        let add = |id: &str, kind: NodeKind, module: usize, page: Option<String>, nodes: &mut Vec<Node>, index: &mut HashMap<String, usize>| {
            if index.contains_key(id) {
                return;
            }
            let label: String = kb.name(id).replace(['[', ']', '$'], "").replace('\n', " ").chars().take(48).collect();
            if label.trim().is_empty() {
                return;
            }
            index.insert(id.to_owned(), nodes.len());
            nodes.push(Node { id: id.to_owned(), label, kind, module, page, pos: Vec2::ZERO, vel: Vec2::ZERO, refs_in: 0, home: Vec2::ZERO });
        };
        for (mi, m) in mods.iter().enumerate() {
            for w in kb.children(Some(m)) {
                if kb.name(w).starts_with("Week") {
                    add(w, NodeKind::Page, mi, None, &mut nodes, &mut index);
                }
            }
        }
        for (i, d) in &kb.q {
            let Some((mi, page)) = page_of.get(i) else { continue };
            let concept = d.get("type").and_then(|t| t.as_i64()) == Some(1);
            if concept || refd.contains_key(i) {
                add(i, NodeKind::Term, *mi, Some(page.clone()), &mut nodes, &mut index);
            }
        }
        for t in refd.keys() {
            if !index.contains_key(t) && kb.q.contains_key(t) {
                let mi = kb.module_of(t).and_then(|m| mod_ix.get(m.as_str()).copied()).unwrap_or(usize::MAX);
                add(t, NodeKind::Term, mi, None, &mut nodes, &mut index);
            }
        }
        for (id, n) in &refd {
            if let Some(&i) = index.get(id) {
                nodes[i].refs_in = *n;
            }
        }
        let owner = |start: &str| -> Option<usize> {
            let mut cur = start.to_owned();
            loop {
                if let Some(&n) = index.get(&cur) {
                    return Some(n);
                }
                cur = kb.q.get(&cur)?.get("parent")?.as_str()?.to_owned();
            }
        };
        let mut w: HashMap<(usize, usize), usize> = HashMap::new();
        for n in &nodes {
            if let (NodeKind::Term, Some(p)) = (n.kind, &n.page) {
                if let (Some(&a), Some(&b)) = (index.get(&n.id), index.get(p)) {
                    *w.entry((a.min(b), a.max(b))).or_default() += 1;
                }
            }
        }
        for (src, dst) in &cites {
            if let (Some(a), Some(&b)) = (owner(src), index.get(dst)) {
                if a != b {
                    *w.entry((a.min(b), a.max(b))).or_default() += 1;
                }
            }
        }
        let edges: Vec<Edge> = w.into_iter().map(|((a, b), weight)| Edge { a, b, weight, cross: nodes[a].module != nodes[b].module }).collect();
        let mut adj = vec![vec![]; nodes.len()];
        for e in &edges {
            adj[e.a].push(e.b);
            adj[e.b].push(e.a);
        }
        let mut seed = 0x6d336d30u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 40) as f32 / (1u64 << 24) as f32 - 0.5
        };
        let nm = mods.len().max(1) as f32;
        let home = |mi: usize| -> Vec2 {
            if mi == usize::MAX {
                return Vec2::ZERO;
            }
            let a = mi as f32 / nm * std::f32::consts::TAU;
            Vec2::new(a.cos(), a.sin()) * 1100.0
        };
        let page_pos: HashMap<String, Vec2> = nodes.iter().filter(|n| n.kind == NodeKind::Page).map(|n| (n.id.clone(), home(n.module) + Vec2::new(rnd(), rnd()) * 300.0)).collect();
        for n in nodes.iter_mut() {
            n.home = home(n.module);
            n.pos = match n.kind {
                NodeKind::Page => page_pos[&n.id],
                NodeKind::Term => n.page.as_ref().and_then(|p| page_pos.get(p)).copied().unwrap_or_else(|| home(n.module)) + Vec2::new(rnd(), rnd()) * 120.0,
            };
        }
        let n = nodes.len();
        let mut g = Graph {
            nodes,
            edges,
            index,
            modules,
            adj,
            sel: None,
            walk: 0,
            center: Vec2::ZERO,
            scale: 0.5,
            filter: None,
            local: None,
            visible: vec![true; n],
            alpha: 1.0,
            fit: true,
            follow: false,
            t: 0.0,
        };
        for _ in 0..150 {
            g.tick();
        }
        g
    }

    pub fn reheat(&mut self) {
        self.alpha = 0.8;
    }

    fn update_visible(&mut self) {
        let keep: Option<HashSet<usize>> = self.local.map(|c| {
            let mut seen = HashSet::from([c]);
            let mut q = VecDeque::from([(c, 0)]);
            while let Some((x, d)) = q.pop_front() {
                if d < 2 {
                    for &y in &self.adj[x] {
                        if seen.insert(y) {
                            q.push_back((y, d + 1));
                        }
                    }
                }
            }
            seen
        });
        for (i, n) in self.nodes.iter().enumerate() {
            let f = self.filter.is_none_or(|m| n.module == m);
            let l = keep.as_ref().is_none_or(|k| k.contains(&i));
            self.visible[i] = f && l;
        }
    }

    fn tick(&mut self) {
        if self.alpha < 0.003 {
            return;
        }
        let vis: Vec<usize> = (0..self.nodes.len()).filter(|&i| self.visible[i]).collect();
        let mut force = vec![Vec2::ZERO; self.nodes.len()];
        for (k, &a) in vis.iter().enumerate() {
            for &b in &vis[k + 1..] {
                let d = self.nodes[a].pos - self.nodes[b].pos;
                let l2 = d.length_sq().max(36.0);
                if l2 > 320.0 * 320.0 {
                    continue;
                }
                let f = d * (1100.0 / l2);
                force[a] += f;
                force[b] -= f;
            }
        }
        for e in &self.edges {
            if !self.visible[e.a] || !self.visible[e.b] {
                continue;
            }
            let d = self.nodes[e.b].pos - self.nodes[e.a].pos;
            let len = d.length().max(0.01);
            let rest = if e.cross { 140.0 } else { 55.0 };
            let k = if e.cross { 0.015 } else { 0.06 } * (e.weight as f32).sqrt().min(3.0);
            let f = d / len * (len - rest) * k;
            force[e.a] += f;
            force[e.b] -= f;
        }
        for &i in &vis {
            let n = &mut self.nodes[i];
            // filtered or local views pull to the middle; the full view to module islands
            let target = if self.filter.is_some() || self.local.is_some() { Vec2::ZERO } else { n.home };
            let g = (target - n.pos) * 0.02;
            n.vel = (n.vel + (force[i] + g) * self.alpha) * 0.55;
            n.pos += n.vel;
        }
        self.alpha *= 0.985;
    }

    pub fn select_id(&mut self, id: &str) {
        if let Some(&i) = self.index.get(id) {
            self.sel = Some(i);
            self.walk = 0;
        }
    }

    fn pick(&mut self, i: usize) {
        self.sel = Some(i);
        self.follow = true;
        self.fit = false;
    }

    pub fn step(&mut self, dir: Vec2) {
        let from = self.sel.map(|s| self.nodes[s].pos).unwrap_or(self.center);
        let mut best: Option<(f32, usize)> = None;
        for (i, n) in self.nodes.iter().enumerate() {
            if !self.visible[i] || Some(i) == self.sel {
                continue;
            }
            let d = n.pos - from;
            let len = d.length().max(0.01);
            let cos = d.dot(dir) / len;
            if cos < 0.45 {
                continue;
            }
            let score = len * (2.2 - cos);
            if best.is_none_or(|(s, _)| score < s) {
                best = Some((score, i));
            }
        }
        if let Some((_, i)) = best {
            self.walk = 0;
            self.pick(i);
        }
    }

    /// tab / s-tab: cycle through the selection's neighbours, strongest link
    /// first, staying anchored on the node where the walk started.
    pub fn walk_neighbours(&mut self, by: isize, anchor: &mut Option<usize>) {
        let Some(s) = self.sel else { return };
        let a = *anchor.get_or_insert(s);
        let mut nb: Vec<(usize, usize)> = self.edges.iter().filter(|e| (e.a == a || e.b == a) && self.visible[e.a] && self.visible[e.b]).map(|e| (if e.a == a { e.b } else { e.a }, e.weight)).collect();
        if nb.is_empty() {
            return;
        }
        nb.sort_by(|x, y| y.1.cmp(&x.1).then(self.nodes[x.0].label.cmp(&self.nodes[y.0].label)));
        if s != a {
            self.walk = (self.walk as isize + by).rem_euclid(nb.len() as isize) as usize;
        } else {
            self.walk = if by > 0 { 0 } else { nb.len() - 1 };
        }
        self.sel = Some(nb[self.walk].0);
        self.follow = true;
        self.fit = false;
    }

    pub fn search(&mut self, q: &str) -> bool {
        let ql = q.to_lowercase();
        let mut best: Option<(u8, usize, usize)> = None;
        for (i, n) in self.nodes.iter().enumerate() {
            let l = n.label.to_lowercase();
            let rank = if l.starts_with(&ql) {
                0
            } else if l.contains(&ql) {
                1
            } else {
                continue;
            };
            let key = (rank, n.label.len(), i);
            if best.is_none_or(|b| key < b) {
                best = Some(key);
            }
        }
        if let Some((_, _, i)) = best {
            if !self.visible[i] {
                self.filter = None;
                self.local = None;
            }
            self.pick(i);
            return true;
        }
        false
    }

    pub fn set_local(&mut self, on: bool) {
        self.local = if on { self.sel } else { None };
        self.fit = true;
        self.follow = false;
        self.reheat();
    }

    pub fn cycle_filter(&mut self) {
        let n = self.modules.len();
        self.filter = match self.filter {
            None if n > 0 => Some(0),
            Some(m) if m + 1 < n => Some(m + 1),
            _ => None,
        };
        self.fit = true;
        self.follow = false;
        self.reheat();
    }

    fn size(&self, n: &Node) -> f32 {
        match n.kind {
            NodeKind::Page => 9.0,
            NodeKind::Term => 3.5 + (n.refs_in as f32).sqrt() * 1.8,
        }
        .min(14.0)
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<String> {
        self.update_visible();
        self.t += ui.input(|i| i.stable_dt).min(0.1);
        let steps = if self.alpha > 0.3 { 6 } else { 2 };
        for _ in 0..steps {
            self.tick();
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
        let vis: Vec<usize> = (0..self.nodes.len()).filter(|&i| self.visible[i]).collect();
        if self.fit && !vis.is_empty() {
            let (mut lo, mut hi) = (self.nodes[vis[0]].pos, self.nodes[vis[0]].pos);
            for &i in &vis {
                lo = lo.min(self.nodes[i].pos);
                hi = hi.max(self.nodes[i].pos);
            }
            let size = (hi - lo).max(Vec2::splat(80.0));
            let target = (lo + hi) / 2.0;
            self.center += (target - self.center) * 0.25;
            let s = ((rect.width() - 120.0) / size.x).min((rect.height() - 120.0) / size.y).clamp(0.02, 3.0);
            self.scale += (s - self.scale) * 0.25;
        } else if let (true, Some(s)) = (self.follow, self.sel) {
            self.center += (self.nodes[s].pos - self.center) * 0.2;
        }
        ui.ctx().request_repaint();

        let to_screen = |p: Vec2| -> Pos2 { rect.center() + (p - self.center) * self.scale };
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, BG);
        let step = 48.0;
        let off = Vec2::new((-self.center.x * self.scale).rem_euclid(step), (-self.center.y * self.scale).rem_euclid(step));
        let mut y = rect.top() + off.y;
        while y < rect.bottom() {
            let mut x = rect.left() + off.x;
            while x < rect.right() {
                p.circle_filled(Pos2::new(x, y), 0.8, GREY0);
                x += step;
            }
            y += step;
        }
        let nb: HashSet<usize> = self.sel.map(|s| self.adj[s].iter().copied().collect()).unwrap_or_default();
        for e in &self.edges {
            if !self.visible[e.a] || !self.visible[e.b] {
                continue;
            }
            let hot = self.sel.is_some_and(|s| s == e.a || s == e.b);
            let (w, c) = match (hot, e.cross) {
                (true, _) => (1.6, RED_BRIGHT),
                (false, true) => (0.8, CRIMSON),
                (false, false) => (0.6, GREY1),
            };
            p.line_segment([to_screen(self.nodes[e.a].pos), to_screen(self.nodes[e.b].pos)], (w, c));
        }
        if self.local.is_none() {
            let mut acc: HashMap<usize, (Vec2, f32)> = HashMap::new();
            for &i in &vis {
                if self.nodes[i].module != usize::MAX {
                    let e = acc.entry(self.nodes[i].module).or_insert((Vec2::ZERO, 0.0));
                    e.0 += self.nodes[i].pos;
                    e.1 += 1.0;
                }
            }
            for (m, (sum, n)) in acc {
                if n >= 3.0 {
                    let c = to_screen(sum / n);
                    p.text(c, egui::Align2::CENTER_CENTER, self.modules[m].1.to_lowercase(), egui::FontId::new(28.0, egui::FontFamily::Name("bold".into())), Color32::from_white_alpha(22));
                }
            }
        }
        let hover = resp.hover_pos();
        let mut hovered: Option<usize> = None;
        let zoom_k = self.scale.sqrt().clamp(0.55, 1.8);
        for &i in &vis {
            let n = &self.nodes[i];
            let c = to_screen(n.pos);
            let r = self.size(n) * zoom_k;
            let sq = Rect::from_center_size(c, Vec2::splat(r * 2.0));
            if hover.is_some_and(|h| sq.expand(3.0).contains(h)) {
                hovered = Some(i);
            }
            let sel = Some(i) == self.sel;
            let near = nb.contains(&i);
            match n.kind {
                NodeKind::Page => {
                    p.rect_filled(sq, 0.0, BG);
                    p.rect_stroke(sq, 0.0, (1.6, if sel { RED_BRIGHT } else { FG_BRIGHT }), egui::StrokeKind::Inside);
                }
                NodeKind::Term => {
                    let col = if sel {
                        RED_BRIGHT
                    } else if near {
                        FG
                    } else if n.refs_in > 0 {
                        FG_DIM
                    } else {
                        COMMENT
                    };
                    p.rect_filled(sq, 0.0, col);
                }
            }
            if sel {
                let pulse = 3.0 + (self.t * 4.0).sin().abs() * 3.0;
                p.rect_stroke(sq.expand(pulse), 0.0, (1.0, RED), egui::StrokeKind::Outside);
            }
            let show_label = sel || (near && (self.scale > 0.8 || self.local.is_some())) || hovered == Some(i) || (n.kind == NodeKind::Page && self.scale > 0.25) || self.scale > 1.2 || (self.local.is_some() && self.scale > 0.4);
            if show_label {
                let col = if sel { FG_BRIGHT } else if near { FG } else { FG_DIM };
                let label = if n.kind == NodeKind::Page {
                    format!("{} {}", n.label.split(" - ").next().unwrap_or(&n.label), self.modules.get(n.module).map(|m| m.1.to_lowercase()).unwrap_or_default())
                } else {
                    n.label.clone()
                };
                p.text(c + Vec2::new(0.0, r + 3.0), egui::Align2::CENTER_TOP, label, egui::FontId::monospace(11.5), col);
            }
        }
        if let Some(s) = self.sel {
            let n = &self.nodes[s];
            let kind = match n.kind {
                NodeKind::Page => "page",
                NodeKind::Term => "term",
            };
            let module = self.modules.get(n.module).map(|m| m.1.clone()).unwrap_or_else(|| "outside modules".into());
            let at = rect.left_top() + Vec2::new(14.0, 14.0);
            let r = Rect::from_min_size(at, Vec2::new(440.0, 46.0));
            p.rect_filled(r, 0.0, BG_ALT);
            p.rect_filled(Rect::from_min_size(r.min, Vec2::new(2.0, r.height())), 0.0, RED_CURSOR);
            p.text(at + Vec2::new(10.0, 6.0), egui::Align2::LEFT_TOP, &n.label, egui::FontId::monospace(13.0), FG_BRIGHT);
            p.text(at + Vec2::new(10.0, 25.0), egui::Align2::LEFT_TOP, format!("{kind} · {module} · {} links · cited {}×", self.adj[s].len(), n.refs_in), egui::FontId::monospace(11.5), FG_DIM);
        }
        let mut info = format!("{} nodes · {} links", vis.len(), self.edges.iter().filter(|e| self.visible[e.a] && self.visible[e.b]).count());
        if let Some(m) = self.filter {
            info += &format!(" · {}", self.modules[m].1.to_lowercase());
        }
        if self.local.is_some() {
            info += " · local";
        }
        p.text(rect.left_bottom() + Vec2::new(12.0, -10.0), egui::Align2::LEFT_BOTTOM, info, egui::FontId::monospace(12.0), COMMENT);
        if resp.clicked() {
            if let Some(i) = hovered {
                if Some(i) == self.sel {
                    return Some(self.nodes[i].id.clone());
                }
                self.pick(i);
            }
        }
        if resp.double_clicked() {
            return hovered.map(|i| self.nodes[i].id.clone());
        }
        None
    }
}
