//! PDF spike: render a few uni PDFs with hayro (pure Rust), timing each step.
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{render, PixmapSettings, RenderCache, RenderSettings};
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap();
    for path in args {
        let name = std::path::Path::new(&path).file_stem().unwrap().to_string_lossy().into_owned();
        let t = Instant::now();
        let bytes = std::fs::read(&path).unwrap();
        let pdf = match Pdf::new(bytes) {
            Ok(p) => p,
            Err(e) => {
                println!("{name}: open failed {e:?}");
                continue;
            }
        };
        let pages = pdf.pages();
        let open_ms = t.elapsed().as_secs_f64() * 1e3;
        let cache = RenderCache::new();
        let settings = InterpreterSettings::default();
        let mut times = vec![];
        for idx in [0, pages.len() / 2] {
            let page = &pages[idx];
            let (w, _) = page.render_dimensions();
            let scale = 1600.0 / w; // 1600 px wide, like a page filling half the laptop screen at 1.5x
            let t = Instant::now();
            let px = render(page, &cache, &settings, &RenderSettings::default(), &PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE });
            let ms = t.elapsed().as_secs_f64() * 1e3;
            times.push(format!("p{idx} {ms:.0} ms ({}x{})", px.width(), px.height()));
            std::fs::write(format!("{out}/hayro-{name}-p{idx}.png"), px.into_png().unwrap()).unwrap();
        }
        println!("{name}: {} pages, open {open_ms:.1} ms, {}", pages.len(), times.join(", "));
    }
}
