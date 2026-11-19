//! App icon: a dot-matrix "m" with a red block cursor after it (vim, keyboard-first),
//! white on black, same dot style as the logo.

/// 7-row m (the logo's m is x-height only; at icon size the full height reads better)
const M: [&str; 7] = ["####.", "#.#.#", "#.#.#", "#.#.#", "#.#.#", "#.#.#", "#.#.#"];

pub fn rgba(size: u32) -> Vec<u8> {
    let mut px = vec![0u8; (size * size * 4) as usize];
    for a in px.chunks_mut(4) {
        a[3] = 255;
    }
    let grid = 13.0; // 5-wide m + gap + 2-wide cursor, centred in a 13x13 grid
    let cell = size as f32 / grid;
    let mut dots: Vec<(f32, f32, [u8; 3])> = vec![];
    for (r, row) in M.iter().enumerate() {
        for (c, ch) in row.chars().enumerate() {
            if ch == '#' {
                dots.push((2.0 + c as f32, 3.0 + r as f32, [0xff, 0xff, 0xff]));
            }
        }
    }
    for x in [8.0, 9.0] {
        for y in 3..10 {
            dots.push((x, y as f32, [0xcc, 0x22, 0x22]));
        }
    }
    for (gx, gy, col) in dots {
        let x0 = ((gx + 0.07) * cell) as u32;
        let y0 = ((gy + 0.07) * cell) as u32;
        let s = (cell * 0.86).max(1.0) as u32;
        for y in y0..(y0 + s).min(size) {
            for x in x0..(x0 + s).min(size) {
                let i = ((y * size + x) * 4) as usize;
                px[i..i + 3].copy_from_slice(&col);
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
