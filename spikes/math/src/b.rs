// Candidate B: MathJax (boa JS engine) -> SVG -> resvg PNG
use mathjax_svg::{render_tex, Options};
use std::time::Instant;

fn main() {
    let out = std::env::args().nth(1).unwrap_or("out".into());
    std::fs::create_dir_all(&out).unwrap();
    let s: Vec<String> = serde_json::from_str(&std::fs::read_to_string("samples.json").unwrap()).unwrap();
    let opts = Options { font_size: 20.0, ..Default::default() };
    let mut total = 0.0;
    let mut n = 0;
    for (i, t) in s.iter().enumerate() {
        let t0 = Instant::now();
        let r = render_tex(t, &opts);
        let dt = t0.elapsed().as_secs_f64() * 1000.0;
        match r {
            Ok(svg) => {
                let svg = svg.replace("currentColor", "#e0e0e0");
                std::fs::write(format!("{out}/{i:02}.svg"), &svg).unwrap();
                let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default());
                match tree {
                    Ok(tree) => {
                        let sz = tree.size();
                        let (w, h) = ((sz.width() * 2.0).ceil() as u32 + 8, (sz.height() * 2.0).ceil() as u32 + 8);
                        let mut pm = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1)).unwrap();
                        resvg::render(&tree, resvg::tiny_skia::Transform::from_translate(4.0, 4.0).pre_scale(2.0, 2.0), &mut pm.as_mut());
                        pm.save_png(format!("{out}/{i:02}.png")).unwrap();
                        if i > 0 { total += dt; n += 1; }
                        println!("OK   {i:02} {dt:.2}ms");
                    }
                    Err(e) => println!("FAIL(rasterise) {i:02} {e}"),
                }
            }
            Err(e) => println!("FAIL {i:02} {} :: {}", e.lines().next().unwrap_or(""), t.replace('\n', " ")),
        }
    }
    println!("MEAN (excl. first) {:.2} ms over {n}", total / n as f64);
}
