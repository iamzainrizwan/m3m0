# phase 0 spikes

## math rendering → RaTeX

`spikes/math/`, compare page `~/Documents/claude/m3m0/math-compare.html`
(browser KaTeX vs both candidates, 44 real snippets from the notes in
`spikes/math/samples.json`).

| | RaTeX 0.1.16 | mathjax-svg-rs 0.4 + resvg |
|---|---|---|
| failures | 0/44 | 11/44 (every `\mathsf` `\mathbb` `\mathcal` `\vec`: MathJax needs async font loading) |
| per expression | 0.30 ms incl. raster + png | 15.7 ms svg only, 484 ms first call |
| release size added | +6.8 MB (embedded KaTeX fonts) | +16 MB |

RaTeX is a Rust port of KaTeX's layout with KaTeX's own fonts, so it matches
the phone. Crates `ratex-parser`, `ratex-layout`, `ratex-types`,
`ratex-render` (`embed-fonts`), all `=0.1.16`. Pipeline: `parse` → `layout`
(`LayoutOptions::with_color`) → `validate_layout` → `to_display_list` →
`render_to_png` (decode to RGBA for an egui texture; cache per expression and
zoom). Wrap in `catch_unwind`: odd input shouldn't take the app down.

## pdf rendering → hayro

`spikes/pdf/`, renders in `~/Documents/claude/m3m0/pdf/`, sheet
`pdf-sheet.png`. Pure Rust (no libpdfium download), 1600 px wide pages:

| file | pages | open | page renders |
|---|---|---|---|
| pep cpp-week3 slides | 72 | 2.1 ms | 16 ms, 6 ms |
| fc2 week2-sat beamer | 22 | 1.0 ms | 2 ms, 3 ms |
| fc2 lgt-02 handout | 8 | 0.4 ms | 13 ms, 20 ms |
| seg team report form | 6 | 0.5 ms | 89 ms, 2 ms |
| fc2 past-paper compendium | 123 | 6.2 ms | 9 ms, 5 ms |

All rendered correctly (fonts, colours, diagrams, embedded images). Limits per
its README: no encrypted PDFs, no blending/knockout groups. hayro rasterises
only; PDF text search later needs a separate extractor. Fallback if hayro
breaks on something: pdfium-render with a prebuilt libpdfium.

API: `Pdf::new(bytes)`, `pdf.pages()`, `render(page, &RenderCache, &InterpreterSettings,
&RenderSettings, &PixmapSettings { x_scale, y_scale, bg_color })` → pixmap
(`into_png`, or its RGBA data for egui). Reuse one `RenderCache` per document.

## bridge

See `docs/bridge.md`. Plumbing verified with a fake Node client; the real test
needs RemNote.
