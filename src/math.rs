//! Inline LaTeX via RaTeX (KaTeX's layout and fonts, in Rust): tex -> png ->
//! egui texture, cached per (tex, size, colour, pixels-per-point).

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions, Vec2};
use ratex_layout::{layout, to_display_list, validate_layout, LayoutOptions};
use ratex_render::{render_to_png, RenderOptions};
use ratex_types::color::Color;
use std::collections::HashMap;

#[derive(Default)]
pub struct Math {
    cache: HashMap<(String, u32, [u8; 4], u32), Option<(TextureHandle, Vec2)>>,
}

fn render(tex: &str, size: f32, col: Color32, ppp: f32) -> Result<ColorImage, String> {
    let ast = ratex_parser::parser::parse(tex).map_err(|e| format!("{e}"))?;
    let c = Color::new(col.r() as f32 / 255.0, col.g() as f32 / 255.0, col.b() as f32 / 255.0, 1.0);
    let lbox = layout(&ast, &LayoutOptions::default().with_color(c));
    validate_layout(&lbox)?;
    let dl = to_display_list(&lbox);
    let png = render_to_png(
        &dl,
        &RenderOptions { font_size: size, padding: 1.0, background_color: Color::new(0.0, 0.0, 0.0, 0.0), font_dir: String::new(), device_pixel_ratio: ppp },
    )?;
    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png).map_err(|e| e.to_string())?.to_rgba8();
    Ok(ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
}

impl Math {
    /// Texture and its size in points, or None if RaTeX can't render it (show the source instead).
    pub fn get(&mut self, ctx: &egui::Context, tex: &str, size: f32, col: Color32) -> Option<(TextureHandle, Vec2)> {
        let ppp = ctx.pixels_per_point();
        let key = (tex.to_owned(), (size * 10.0) as u32, col.to_array(), (ppp * 100.0) as u32);
        if let Some(v) = self.cache.get(&key) {
            return v.clone();
        }
        // a panic inside the layout engine must not take the app down
        let r = std::panic::catch_unwind(|| render(tex, size, col, ppp)).unwrap_or_else(|_| Err("panic".into()));
        let v = r.ok().map(|img| {
            let sz = Vec2::new(img.size[0] as f32, img.size[1] as f32) / ppp;
            (ctx.load_texture(format!("tex:{tex}"), img, TextureOptions::LINEAR), sz)
        });
        self.cache.insert(key, v.clone());
        v
    }
}
