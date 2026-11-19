//! rice palette (~/rice/palette.md) and BlexMono.

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily};

pub const BG: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
pub const BG_ALT: Color32 = Color32::from_rgb(0x0a, 0x0a, 0x0a);
pub const SELECTION: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
pub const FG: Color32 = Color32::from_rgb(0xe0, 0xe0, 0xe0);
pub const FG_BRIGHT: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
pub const FG_DIM: Color32 = Color32::from_rgb(0x66, 0x66, 0x66);
pub const COMMENT: Color32 = Color32::from_rgb(0x44, 0x44, 0x44);
pub const GREY0: Color32 = Color32::from_rgb(0x22, 0x22, 0x22);
pub const GREY1: Color32 = Color32::from_rgb(0x33, 0x33, 0x33);
pub const RED: Color32 = Color32::from_rgb(0xcc, 0x22, 0x22);
pub const RED_BRIGHT: Color32 = Color32::from_rgb(0xff, 0x44, 0x44);
pub const RED_CURSOR: Color32 = Color32::from_rgb(0xe0, 0x33, 0x33);
pub const CRIMSON: Color32 = Color32::from_rgb(0x6b, 0x0a, 0x1a);

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let dir = crate::kb::home().join(".fonts");
    for (name, file) in [("blex", "BlexMonoNerdFontMono-Regular.ttf"), ("blex-bold", "BlexMonoNerdFontMono-Bold.ttf")] {
        if let Ok(bytes) = std::fs::read(dir.join(file)) {
            fonts.font_data.insert(name.into(), FontData::from_owned(bytes).into());
        }
    }
    if fonts.font_data.contains_key("blex") {
        for fam in [FontFamily::Monospace, FontFamily::Proportional] {
            fonts.families.entry(fam).or_default().insert(0, "blex".into());
        }
    }
    if fonts.font_data.contains_key("blex-bold") {
        let mut fam = vec!["blex-bold".to_owned()];
        fam.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
        fonts.families.insert(FontFamily::Name("bold".into()), fam);
    } else {
        let p = fonts.families[&FontFamily::Proportional].clone();
        fonts.families.insert(FontFamily::Name("bold".into()), p);
    }
    ctx.set_fonts(fonts);

    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = BG_ALT;
    v.extreme_bg_color = BG;
    v.faint_bg_color = BG_ALT;
    v.override_text_color = Some(FG);
    v.selection.bg_fill = CRIMSON;
    v.selection.stroke.color = RED_BRIGHT;
    v.hyperlink_color = RED_BRIGHT;
    v.text_cursor.stroke.color = RED_CURSOR;
    v.window_corner_radius = 0.into();
    v.menu_corner_radius = 0.into();
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    v.window_stroke.color = GREY1;
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = 0.into();
    }
    v.widgets.noninteractive.bg_stroke.color = GREY0;
    v.widgets.inactive.bg_fill = BG_ALT;
    v.widgets.inactive.weak_bg_fill = BG_ALT;
    v.widgets.hovered.bg_fill = SELECTION;
    v.widgets.hovered.weak_bg_fill = SELECTION;
    v.widgets.hovered.bg_stroke.color = GREY1;
    v.widgets.active.bg_fill = CRIMSON;
    ctx.set_visuals(v);
}
