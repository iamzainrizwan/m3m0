//! App icon: the logo's dot-matrix glyphs stacked 2x2 ("m3" over "m0"),
//! white m's and red digits on black. Drawn in code so it can't drift from the logo.

use crate::boot::GLYPHS;

pub fn rgba(size: u32) -> Vec<u8> {
    let mut px = vec![0u8; (size * size * 4) as usize];
    for a in px.chunks_mut(4) {
        a[3] = 255;
    }
    let grid = 17.0; // 11x15 glyph block centred in a 17x17 grid
    let cell = size as f32 / grid;
    let (ox, oy) = (3.0, 1.0);
    let place = [(0usize, 0.0, 0.0), (1, 6.0, 0.0), (2, 0.0, 8.0), (3, 6.0, 8.0)];
    for (g, gx, gy) in place {
        let red = g % 2 == 1;
        for (r, row) in GLYPHS[g].iter().enumerate() {
            for (c, ch) in row.chars().enumerate() {
                if ch != '#' {
                    continue;
                }
                let x0 = ((ox + gx + c as f32 + 0.07) * cell) as u32;
                let y0 = ((oy + gy + r as f32 + 0.07) * cell) as u32;
                let s = (cell * 0.86).max(1.0) as u32;
                let col: [u8; 3] = if red { [0xcc, 0x22, 0x22] } else { [0xff, 0xff, 0xff] };
                for y in y0..(y0 + s).min(size) {
                    for x in x0..(x0 + s).min(size) {
                        let i = ((y * size + x) * 4) as usize;
                        px[i..i + 3].copy_from_slice(&col);
                    }
                }
            }
        }
    }
    px
}

/// `m3m0 --install`: icon + .desktop entry so GNOME shows the icon in the dock
/// (Wayland matches the window to the entry by app id).
pub fn install() -> std::io::Result<()> {
    let home = crate::kb::home();
    let icon = home.join(".local/share/icons/hicolor/256x256/apps/m3m0.png");
    std::fs::create_dir_all(icon.parent().unwrap())?;
    image::save_buffer(&icon, &rgba(256), 256, 256, image::ColorType::Rgba8).map_err(std::io::Error::other)?;
    let exe = std::env::current_exe()?;
    let desktop = home.join(".local/share/applications/m3m0.desktop");
    std::fs::write(
        &desktop,
        format!(
            "[Desktop Entry]\nType=Application\nName=m3m0\nComment=native remnote\nExec={}\nIcon=m3m0\nStartupWMClass=m3m0\nCategories=Education;Office;\nTerminal=false\n",
            exe.display()
        ),
    )?;
    println!("wrote {}\nwrote {}", icon.display(), desktop.display());
    Ok(())
}
