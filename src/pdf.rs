//! Week PDFs: find them in uni-git, render pages with hayro on a worker
//! thread, show them as a continuous scrolling column.
//!
//! uni-git layout: KEATS downloads land in `<module>/sources/...` and get
//! sorted into teaching-week folders (`sources/slides/week-05/week7_*.pdf` is
//! teaching week 5). A RemNote module rem "FC2" maps to `uni-git/fc2`, and
//! "Week 5 - ..." to any `week-05` folder. Module-wide PDFs (build/,
//! past-papers/, coursework/) are listed under the week's files.

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

pub fn uni_dir() -> PathBuf {
    std::env::var("M3M0_UNI").map(PathBuf::from).unwrap_or_else(|_| crate::kb::home().join("uni-git"))
}

/// "Week 5 - Approximation" -> 5
pub fn week_number(name: &str) -> Option<u32> {
    let rest = name.trim().strip_prefix("Week")?.trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
            continue;
        }
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// (this week's files, module-wide PDFs) for a module + week. A week file is
/// anything under a `week-NN` folder or named `week-NN*` (quiz JSON, answers,
/// decks, slide text...), plus that week's lgt/sgt tutorial sheets.
pub fn week_files(module: &str, week: u32) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let root = uni_dir().join(module.to_lowercase());
    let mut all = vec![];
    walk(&root, &mut all);
    let tag = format!("week-{week:02}");
    let tut = [format!("lgt-{week:02}"), format!("sgt-{week:02}")];
    let mut wk = vec![];
    let mut module_wide = vec![];
    for p in all {
        let rel = p.strip_prefix(&root).unwrap_or(&p).to_path_buf();
        let in_week = rel.components().any(|c| c.as_os_str() == tag.as_str());
        let name = p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        let named = name == tag || name.starts_with(&format!("{tag}-")) || name.starts_with(&format!("{tag}_"));
        let is_tut = tut.iter().any(|t| name.starts_with(t.as_str()));
        let any_week = rel.to_string_lossy().contains("week-");
        let pdf = crate::files::kind(&p) == crate::files::FileKind::Pdf;
        if in_week || named || is_tut {
            wk.push(p);
        } else if pdf && !any_week && !rel.to_string_lossy().contains("tutorials") {
            module_wide.push(p);
        }
    }
    // the week's own notes first, then quiz, slides, tutorials, decks, the rest
    let rank = |p: &PathBuf| -> (u8, PathBuf) {
        let r = p.strip_prefix(&root).unwrap_or(p).to_string_lossy().to_string();
        let k = if r.starts_with("weeks/") {
            0
        } else if r.contains("quizzes") {
            1
        } else if r.contains("slides") {
            2
        } else if r.contains("tutorials") {
            3
        } else if r.starts_with("revision") {
            4
        } else {
            5
        };
        (k, p.clone())
    };
    wk.sort_by_key(rank);
    module_wide.sort();
    (wk, module_wide)
}

enum Req {
    Open(PathBuf),
    Page(PathBuf, usize, u32),
}

enum Resp {
    Opened(PathBuf, Result<Vec<(f32, f32)>, String>),
    Page(PathBuf, usize, u32, ColorImage),
}

fn worker(rx: Receiver<Req>, tx: Sender<Resp>, repaint: impl Fn()) {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::Pdf;
    use hayro::vello_cpu::color::palette::css::WHITE;
    use hayro::{render, PixmapSettings, RenderCache, RenderSettings};
    let mut docs: HashMap<PathBuf, Pdf> = HashMap::new();
    let settings = InterpreterSettings::default();
    while let Ok(mut req) = rx.recv() {
        // skip stale page requests: keep only the newest few
        let mut queue = vec![];
        loop {
            queue.push(req);
            match rx.try_recv() {
                Ok(r) => req = r,
                Err(_) => break,
            }
        }
        let n = queue.len();
        for (i, req) in queue.into_iter().enumerate() {
            match req {
                Req::Open(p) => {
                    let r = std::fs::read(&p).map_err(|e| e.to_string()).and_then(|b| Pdf::new(b).map_err(|e| format!("{e:?}")));
                    let res = r.map(|pdf| {
                        let sizes = pdf.pages().iter().map(|pg| pg.render_dimensions()).collect();
                        docs.insert(p.clone(), pdf);
                        sizes
                    });
                    let _ = tx.send(Resp::Opened(p, res));
                }
                Req::Page(p, idx, w) => {
                    if n - i > 12 {
                        continue; // scrolled past
                    }
                    let Some(pdf) = docs.get(&p) else { continue };
                    let pages = pdf.pages();
                    let Some(page) = pages.get(idx) else { continue };
                    let (pw, _) = page.render_dimensions();
                    let scale = w as f32 / pw.max(1.0);
                    let cache = RenderCache::new();
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        render(page, &cache, &settings, &RenderSettings::default(), &PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE })
                    }));
                    if let Ok(px) = r {
                        let img = ColorImage::from_rgba_premultiplied([px.width() as usize, px.height() as usize], px.data_as_u8_slice());
                        let _ = tx.send(Resp::Page(p, idx, w, img));
                    }
                }
            }
            repaint();
        }
    }
}

pub struct Pdfs {
    tx: Sender<Req>,
    rx: Receiver<Resp>,
    pub files: Vec<PathBuf>,
    pub n_week: usize,
    pub cur: usize,
    pub sizes: HashMap<PathBuf, Result<Vec<(f32, f32)>, String>>,
    tex: HashMap<(PathBuf, usize), (u32, TextureHandle)>,
    asked: HashSet<(PathBuf, usize, u32)>,
    pub scroll: f32,
    pub zoom: f32,
    pub key: Option<(String, String)>, // (module, week) the list was built for
    /// non-PDF files: cached text, images, scroll, quiz answers shown
    texts: HashMap<PathBuf, String>,
    images: HashMap<PathBuf, Option<TextureHandle>>,
    pub text_scroll: f32,
    pub set_text_scroll: bool,
    pub show_answers: bool,
}

impl Pdfs {
    pub fn new(repaint: impl Fn() + Send + 'static) -> Pdfs {
        let (tx, wrx) = channel();
        let (wtx, rx) = channel();
        std::thread::spawn(move || worker(wrx, wtx, repaint));
        Pdfs { tx, rx, files: vec![], n_week: 0, cur: 0, sizes: HashMap::new(), tex: HashMap::new(), asked: HashSet::new(), scroll: 0.0, zoom: 1.0, key: None, texts: HashMap::new(), images: HashMap::new(), text_scroll: 0.0, set_text_scroll: true, show_answers: false }
    }

    pub fn set_week(&mut self, module: &str, week_name: &str) {
        let key = (module.to_owned(), week_name.to_owned());
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.key = Some(key);
        let (w, m) = week_number(week_name).map(|n| week_files(module, n)).unwrap_or_default();
        self.n_week = w.len();
        self.files = w.into_iter().chain(m).collect();
        self.cur = 0;
        self.scroll = 0.0;
        self.open_cur();
    }

    pub fn current_kind(&self) -> Option<crate::files::FileKind> {
        self.files.get(self.cur).map(|p| crate::files::kind(p))
    }

    /// Scroll a non-PDF file by `by` points (keys).
    pub fn scroll_text(&mut self, by: f32) {
        self.text_scroll = (self.text_scroll + by).max(0.0);
        self.set_text_scroll = true;
    }

    pub fn open_cur(&mut self) {
        self.text_scroll = 0.0;
        self.set_text_scroll = true;
        self.show_answers = false;
        if let Some(p) = self.files.get(self.cur) {
            if crate::files::kind(p) != crate::files::FileKind::Pdf {
                self.texts.remove(p); // re-read: Claude may have edited it
                return;
            }
            if !self.sizes.contains_key(p) {
                let _ = self.tx.send(Req::Open(p.clone()));
            }
        }
        self.scroll = 0.0;
    }

    pub fn step(&mut self, by: isize) {
        if self.files.is_empty() {
            return;
        }
        self.cur = (self.cur as isize + by).rem_euclid(self.files.len() as isize) as usize;
        self.open_cur();
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(r) = self.rx.try_recv() {
            match r {
                Resp::Opened(p, s) => {
                    self.sizes.insert(p, s);
                }
                Resp::Page(p, i, w, img) => {
                    let t = ctx.load_texture(format!("pdf:{}:{i}", p.display()), img, TextureOptions::LINEAR);
                    self.tex.insert((p, i), (w, t));
                }
            }
        }
    }

    /// Draw the current file: PDFs as pages, everything else with its renderer.
    pub fn show(&mut self, ui: &mut egui::Ui, kb: &crate::kb::Kb, math: &mut crate::math::Math) {
        use crate::files::{self, FileKind};
        let Some(path) = self.files.get(self.cur).cloned() else {
            ui.label(egui::RichText::new("no files for this week in uni-git").color(crate::theme::COMMENT));
            return;
        };
        let k = files::kind(&path);
        if k == FileKind::Pdf {
            return self.show_pdf(ui);
        }
        let mut sa = egui::ScrollArea::vertical().id_salt("file-scroll").auto_shrink(false);
        if self.set_text_scroll {
            sa = sa.vertical_scroll_offset(self.text_scroll);
            self.set_text_scroll = false;
        }
        let (texts, images, show_answers) = (&mut self.texts, &mut self.images, self.show_answers);
        let out = sa.show(ui, |ui| {
            if k == FileKind::Image {
                files::image(ui, images, &path);
                return;
            }
            let src = texts.entry(path.clone()).or_insert_with(|| std::fs::read_to_string(&path).unwrap_or_else(|e| format!("can't read: {e}"))).clone();
            match k {
                FileKind::Quiz => files::quiz(ui, kb, math, &path, &src, show_answers),
                FileKind::Markdown => files::markdown(ui, kb, math, &src),
                FileKind::Json => files::json(ui, &src),
                _ => files::text(ui, &src),
            }
        });
        // mouse-wheel scrolling feeds back so keys continue from there
        self.text_scroll = out.state.offset.y;
        let max = (out.content_size.y - out.inner_rect.height()).max(0.0);
        if self.text_scroll > max {
            self.text_scroll = max;
        }
    }

    fn show_pdf(&mut self, ui: &mut egui::Ui) {
        use crate::theme::*;
        use egui::{Rect, Sense, Vec2};
        self.poll(ui.ctx());
        let Some(path) = self.files.get(self.cur).cloned() else {
            ui.label(egui::RichText::new("no PDFs for this week in uni-git").color(COMMENT));
            return;
        };
        let avail = ui.available_rect_before_wrap();
        let width = (avail.width() * self.zoom).max(100.0);
        let ppp = ui.ctx().pixels_per_point();
        let wpx = ((width * ppp) as u32 / 64 + 1) * 64; // bucket so resizing doesn't re-render every frame
        let sizes = match self.sizes.get(&path) {
            Some(Ok(s)) => s.clone(),
            Some(Err(e)) => {
                ui.colored_label(RED_BRIGHT, format!("can't open: {e}"));
                return;
            }
            None => {
                ui.label(egui::RichText::new("opening…").color(COMMENT));
                return;
            }
        };
        let gap = 10.0;
        let heights: Vec<f32> = sizes.iter().map(|(w, h)| width * h / w.max(1.0)).collect();
        let total: f32 = heights.iter().sum::<f32>() + gap * heights.len() as f32;
        self.scroll = self.scroll.clamp(0.0, (total - avail.height()).max(0.0));
        let (rect, resp) = ui.allocate_exact_size(avail.size(), Sense::click_and_drag());
        if resp.hovered() {
            let d = ui.input(|i| i.smooth_scroll_delta);
            self.scroll = (self.scroll - d.y).clamp(0.0, (total - avail.height()).max(0.0));
        }
        if resp.dragged() {
            self.scroll = (self.scroll - resp.drag_delta().y).clamp(0.0, (total - avail.height()).max(0.0));
        }
        let p = ui.painter_at(rect);
        let x0 = rect.left() + ((rect.width() - width) / 2.0).max(0.0);
        let mut y = rect.top() - self.scroll;
        for (i, h) in heights.iter().enumerate() {
            let pr = Rect::from_min_size(egui::pos2(x0, y), Vec2::new(width, *h));
            if pr.bottom() >= rect.top() - avail.height() && pr.top() <= rect.bottom() + avail.height() {
                match self.tex.get(&(path.clone(), i)) {
                    Some((w, t)) => {
                        p.image(t.id(), pr, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                        if *w != wpx && self.asked.insert((path.clone(), i, wpx)) {
                            let _ = self.tx.send(Req::Page(path.clone(), i, wpx));
                        }
                    }
                    None => {
                        p.rect_filled(pr, 0.0, GREY0);
                        if self.asked.insert((path.clone(), i, wpx)) {
                            let _ = self.tx.send(Req::Page(path.clone(), i, wpx));
                        }
                    }
                }
            }
            y += h + gap;
        }
        // page indicator
        let mut acc = 0.0;
        let mut page = 0;
        for (i, h) in heights.iter().enumerate() {
            if acc + h / 2.0 > self.scroll {
                page = i;
                break;
            }
            acc += h + gap;
        }
        let label = format!(" {}/{} ", page + 1, heights.len());
        let at = rect.right_bottom() - Vec2::new(8.0, 8.0);
        let g = p.text(at, egui::Align2::RIGHT_BOTTOM, &label, egui::FontId::monospace(12.0), FG);
        p.rect_filled(g.expand(2.0), 0.0, BG_ALT);
        p.text(at, egui::Align2::RIGHT_BOTTOM, &label, egui::FontId::monospace(12.0), FG);
    }

    pub fn page_heights(&self, width: f32) -> Vec<f32> {
        match self.files.get(self.cur).and_then(|p| self.sizes.get(p)) {
            Some(Ok(s)) => s.iter().map(|(w, h)| width * h / w.max(1.0)).collect(),
            _ => vec![],
        }
    }
}
