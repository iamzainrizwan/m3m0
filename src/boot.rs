//! Boot animation: a drifting note graph collapses into the dot-matrix logo,
//! a red scan sweeps it into solid blocks, the boot log types out real stats,
//! then the logo flies into the top bar. Any key or click skips ahead.

use crate::theme::*;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Vec2};

/// "m3m0", 5x7 cells per glyph. '#' = dot. Digits are drawn red.
pub const GLYPHS: [[&str; 7]; 4] = [
    [".....", ".....", "####.", "#.#.#", "#.#.#", "#.#.#", "#.#.#"],
    ["####.", "....#", "....#", ".###.", "....#", "....#", "####."],
    [".....", ".....", "####.", "#.#.#", "#.#.#", "#.#.#", "#.#.#"],
    [".###.", "#...#", "#..##", "#.#.#", "##..#", "#...#", ".###."],
];
pub const LOGO_COLS: f32 = 23.0; // 4 glyphs * 5 + 3 gaps
pub const LOGO_ROWS: f32 = 7.0;

/// (col, row, red) for every dot of the logo.
pub fn logo_dots() -> Vec<(f32, f32, bool)> {
    let mut v = vec![];
    for (g, glyph) in GLYPHS.iter().enumerate() {
        for (r, row) in glyph.iter().enumerate() {
            for (c, ch) in row.chars().enumerate() {
                if ch == '#' {
                    v.push(((g * 6 + c) as f32, r as f32, g % 2 == 1));
                }
            }
        }
    }
    v
}

/// Draw the finished logo with its top-left at `tl` and dot pitch `pitch`.
pub fn paint_logo(p: &egui::Painter, tl: Pos2, pitch: f32, alpha: f32) {
    for (c, r, red) in logo_dots() {
        let ctr = tl + Vec2::new((c + 0.5) * pitch, (r + 0.5) * pitch);
        let col = if red { RED } else { FG_BRIGHT };
        p.rect_filled(Rect::from_center_size(ctr, Vec2::splat(pitch * 0.86)), 0.0, col.gamma_multiply(alpha));
    }
}

struct Particle {
    home: Vec2, // unit square
    ph: f32,
    speed: f32,
    target: Option<(f32, f32, bool)>,
    delay: f32,
}

pub struct Boot {
    t0: Option<f64>,
    skip_at: Option<f32>,
    parts: Vec<Particle>,
    pub finished: bool,
}

fn rng(seed: &mut u64) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed >> 40) as f32 / (1u64 << 24) as f32
}

fn hash(a: u32, b: u32) -> f32 {
    let mut s = (a as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (b as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) | 1;
    rng(&mut s)
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

// timeline (seconds)
const CONVERGE: f32 = 0.55;
const MORPH: f32 = 1.45;
const LOG: f32 = 1.95;
const LINE_GAP: f32 = 0.11;
const FLY: f32 = 0.45;

impl Boot {
    pub fn new() -> Boot {
        let mut seed = 0x6d336d30u64;
        let mut parts = vec![];
        for d in logo_dots() {
            parts.push(Particle {
                home: Vec2::new(rng(&mut seed), rng(&mut seed)),
                ph: rng(&mut seed) * std::f32::consts::TAU,
                speed: 0.4 + rng(&mut seed),
                target: Some(d),
                delay: d.0 / LOGO_COLS * 0.35 + rng(&mut seed) * 0.15,
            });
        }
        for _ in 0..70 {
            parts.push(Particle {
                home: Vec2::new(rng(&mut seed), rng(&mut seed)),
                ph: rng(&mut seed) * std::f32::consts::TAU,
                speed: 0.4 + rng(&mut seed),
                target: None,
                delay: 0.0,
            });
        }
        Boot { t0: None, skip_at: None, parts, finished: false }
    }

    /// Paint the boot overlay. `log` is None until the snapshot has loaded.
    /// `bar_logo` is where the top-bar logo sits (top-left, pitch).
    pub fn show(&mut self, ctx: &egui::Context, log: Option<&[(bool, String)]>, bar_logo: (Pos2, f32)) {
        if self.finished {
            return;
        }
        let now = ctx.input(|i| i.time);
        let t = (now - *self.t0.get_or_insert(now)) as f32;
        let n_lines = log.map_or(1, |l| l.len()) as f32;
        let natural_end = LOG + n_lines * LINE_GAP + 0.55;
        if ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. } | egui::Event::PointerButton { pressed: true, .. }))) {
            self.skip_at.get_or_insert(t);
        }
        // fly-out starts at the natural end (or on skip), but never before the data is in
        let fly_start = match (log, self.skip_at) {
            (None, _) => f32::INFINITY,
            (Some(_), Some(s)) => s.max(MORPH + 0.3).min(natural_end),
            (Some(_), None) => natural_end,
        };
        let fly = ease_out((t - fly_start) / FLY);
        if t > fly_start + FLY {
            self.finished = true;
            return;
        }
        ctx.request_repaint();

        let screen = ctx.screen_rect();
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("boot")));
        p.rect_filled(screen, 0.0, BG.gamma_multiply(1.0 - smooth((t - fly_start) / (FLY * 0.9))));

        let pitch = (screen.width() / 42.0).clamp(8.0, 18.0);
        let logo_size = Vec2::new(LOGO_COLS * pitch, LOGO_ROWS * pitch);
        let tl = screen.center() - logo_size / 2.0 - Vec2::new(0.0, screen.height() * 0.12);
        let (bar_tl, bar_pitch) = bar_logo;
        // map a point from the big logo frame into the top bar logo frame
        let to_bar = |q: Pos2| -> Pos2 {
            let fq = bar_tl + (q - tl) * (bar_pitch / pitch);
            q + (fq - q) * fly
        };
        let scale = 1.0 + (bar_pitch / pitch - 1.0) * fly;

        // particles
        let pos: Vec<Pos2> = self
            .parts
            .iter()
            .map(|pt| {
                let drift = Vec2::new(
                    (t * pt.speed + pt.ph).sin() * 28.0,
                    (t * pt.speed * 0.8 + pt.ph * 1.3).cos() * 22.0,
                );
                let start = screen.min + pt.home * screen.size() + drift;
                match pt.target {
                    Some((c, r, _)) => {
                        let k = ease_out((t - CONVERGE - pt.delay) / 0.8);
                        let mut goal = tl + Vec2::new((c + 0.5) * pitch, (r + 0.5) * pitch);
                        // glitch: a few rows jump sideways for a frame or two
                        if (MORPH + 0.25..MORPH + 0.55).contains(&t) && hash(r as u32, (t * 30.0) as u32) < 0.22 {
                            goal.x += pitch * if hash(r as u32 + 9, (t * 30.0) as u32) < 0.5 { -0.5 } else { 0.5 };
                        }
                        to_bar(start + (goal - start) * k)
                    }
                    None => start,
                }
            })
            .collect();

        // edges between nearby nodes, fading as the logo forms
        let edge_fade = 1.0 - smooth((t - 0.9) / 0.6);
        let reach = 110.0f32;
        if edge_fade > 0.0 {
            for i in 0..pos.len() {
                for j in i + 1..pos.len() {
                    let d = pos[i].distance(pos[j]);
                    if d < reach {
                        let a = (1.0 - d / reach) * edge_fade * smooth(t / 0.3);
                        p.line_segment([pos[i], pos[j]], (1.0, GREY1.gamma_multiply(a)));
                    }
                }
            }
            // synapse pulses: red sparks running along edges
            for k in 0..14u32 {
                let i = (hash(k, 1) * pos.len() as f32) as usize % pos.len();
                let Some(j) = (0..pos.len())
                    .filter(|&j| j != i)
                    .min_by(|&a, &b| pos[i].distance(pos[a]).total_cmp(&pos[i].distance(pos[b])))
                else {
                    continue;
                };
                if pos[i].distance(pos[j]) > reach {
                    continue;
                }
                let u = (t * (1.2 + hash(k, 2)) + hash(k, 3)).fract();
                let q = pos[i] + (pos[j] - pos[i]) * u;
                p.circle_filled(q, 2.0, RED_BRIGHT.gamma_multiply(edge_fade));
            }
        }

        // nodes
        let morph = smooth((t - MORPH) / 0.3);
        let scan_x = tl.x + (t - MORPH) / 0.4 * logo_size.x;
        for (pt, &q) in self.parts.iter().zip(&pos) {
            match pt.target {
                None => {
                    let a = 1.0 - smooth((t - 0.7) / 0.6);
                    if a > 0.0 {
                        p.circle_filled(q, 2.2, FG_DIM.gamma_multiply(a * smooth(t / 0.3)));
                    }
                }
                Some((c, _, red)) => {
                    let r0 = 2.6 * scale;
                    let side = (r0 * 2.0 + (pitch * 0.86 * scale - r0 * 2.0) * morph).max(1.0);
                    let rounding = r0 * (1.0 - morph);
                    let settled = if red { RED } else { FG_BRIGHT };
                    let mut col = lerp(FG_DIM, settled, morph);
                    // the scan bar flashes each column bright red as it passes
                    let x = tl.x + (c + 0.5) * pitch;
                    let flash = 1.0 - ((scan_x - x).abs() / (pitch * 2.5)).min(1.0);
                    if flash > 0.0 && t < MORPH + 0.6 {
                        col = lerp(col, RED_BRIGHT, flash);
                    }
                    p.rect_filled(Rect::from_center_size(q, Vec2::splat(side)), CornerRadius::same(rounding as u8), col.gamma_multiply(smooth(t / 0.3)));
                }
            }
        }

        let text_a = 1.0 - smooth((t - fly_start) / (FLY * 0.5));
        if text_a <= 0.0 {
            return;
        }
        // blinking block cursor after the logo
        if t > MORPH + 0.3 {
            let on = ((t - MORPH) * 2.0) as i32 % 2 == 0;
            if on {
                let cx = tl.x + (LOGO_COLS + 1.6) * pitch;
                p.rect_filled(
                    Rect::from_min_size(Pos2::new(cx, tl.y + 2.0 * pitch), Vec2::new(pitch * 0.9, 5.0 * pitch)),
                    0.0,
                    RED_CURSOR.gamma_multiply(text_a),
                );
            }
        }
        // tagline, typed
        let tag = "native remnote · v0.1";
        let n = (((t - MORPH - 0.35) / 0.025).max(0.0) as usize).min(tag.chars().count());
        let tag_y = tl.y + logo_size.y + pitch * 1.6;
        p.text(Pos2::new(tl.x, tag_y), Align2::LEFT_TOP, tag.chars().take(n).collect::<String>(), FontId::monospace(13.0), FG_DIM.gamma_multiply(text_a));

        // boot log
        let font = FontId::monospace(12.5);
        let mut y = tag_y + 34.0;
        let pending = [(false, "snapshot remnote.db".to_owned())];
        let lines = log.unwrap_or(&pending);
        for (k, (ok, line)) in lines.iter().enumerate() {
            let at = LOG + k as f32 * LINE_GAP;
            if t < at {
                break;
            }
            let a = smooth((t - at) / 0.08) * text_a;
            let (tag, tag_col) = if *ok {
                (" ok ", FG)
            } else {
                (["·   ", " ·  ", "  · ", "   ·"][((t * 10.0) as usize) % 4], RED_BRIGHT)
            };
            let x = tl.x;
            let r = p.text(Pos2::new(x, y), Align2::LEFT_TOP, "[", font.clone(), COMMENT.gamma_multiply(a));
            let r = p.text(r.right_top(), Align2::LEFT_TOP, tag, font.clone(), tag_col.gamma_multiply(a));
            let r = p.text(r.right_top(), Align2::LEFT_TOP, "] ", font.clone(), COMMENT.gamma_multiply(a));
            p.text(r.right_top(), Align2::LEFT_TOP, line, font.clone(), FG_DIM.gamma_multiply(a));
            y += 18.0;
        }
        if self.skip_at.is_none() && t > 1.0 {
            p.text(
                Pos2::new(screen.center().x, screen.max.y - 28.0),
                Align2::CENTER_CENTER,
                "any key to skip",
                FontId::monospace(11.0),
                COMMENT.gamma_multiply(text_a * smooth((t - 1.0) / 0.4)),
            );
        }
    }
}

fn lerp(a: Color32, b: Color32, k: f32) -> Color32 {
    let k = k.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k) as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}
