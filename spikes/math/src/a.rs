// Candidate A: RaTeX. LaTeX -> parse -> layout -> display list -> PNG (tiny-skia, embedded KaTeX fonts).
use ratex_layout::{layout, to_display_list, validate_layout, LayoutOptions};
use ratex_parser::parser::parse;
use ratex_render::{render_to_png, RenderOptions};
use ratex_types::color::Color;
use std::time::Instant;

fn render(tex: &str, fg: Color) -> Result<Vec<u8>, String> {
    let ast = parse(tex).map_err(|e| format!("parse: {e}"))?;
    let lopts = LayoutOptions::default().with_color(fg);
    let lbox = layout(&ast, &lopts);
    validate_layout(&lbox)?;
    let dl = to_display_list(&lbox);
    let ropts = RenderOptions {
        font_size: 20.0,
        padding: 4.0,
        background_color: Color::new(0.0, 0.0, 0.0, 0.0), // transparent
        font_dir: String::new(),                          // unused with embed-fonts
        device_pixel_ratio: 2.0,
    };
    render_to_png(&dl, &ropts)
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or("out".into());
    std::fs::create_dir_all(&out).unwrap();
    let s: Vec<String> = serde_json::from_str(&std::fs::read_to_string("samples.json").unwrap()).unwrap();
    let fg = Color::from_hex("#e0e0e0").unwrap();
    let mut total = 0.0;
    let mut n = 0;
    for (i, t) in s.iter().enumerate() {
        let t0 = Instant::now();
        // catch panics too: report as failure
        let r = std::panic::catch_unwind(|| render(t, fg)).unwrap_or_else(|_| Err("PANIC".into()));
        let dt = t0.elapsed().as_secs_f64() * 1000.0;
        match r {
            Ok(png) => {
                std::fs::write(format!("{out}/{i:02}.png"), png).unwrap();
                if i > 0 { total += dt; n += 1; }
                println!("OK   {i:02} {dt:.2}ms");
            }
            Err(e) => println!("FAIL {i:02} {e} :: {}", t.replace('\n', " ")),
        }
    }
    println!("MEAN (excl. first) {:.2} ms over {n}", total / n as f64);
}
