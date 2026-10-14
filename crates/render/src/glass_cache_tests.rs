//! Glass renders the same however a render is cut: a band or a canvas patch whose edge runs
//! through a pane shows exactly the pixels of the whole render.

use figcraft_color::{Color, Paint};
use figcraft_doc::{Document, Effect, EffectKind, Glass, NodeId, NodeKind};
use figcraft_geom::Affine;
use tiny_skia::Pixmap;

use crate::{Options, View, render_view_single};

/// Stripes under a 600 × 360 pane at (300, 220) with corner `radius` and `g`: (document, page).
fn pane_over_stripes(g: Glass, radius: f64) -> (Document, NodeId) {
    let mut d = Document::new("glass");
    let p = d.first_page().unwrap();
    let colours = ["E03030", "2040E0", "20C060", "F0C830"].map(|h| Color::from_hex(h).unwrap());
    for i in 0..24u32 {
        let s = d.create(p, NodeKind::Rectangle, "Stripe", f64::from(i) * 50.0, 0.0, 50.0, 800.0).unwrap();
        d.get_mut(s).unwrap().fills = vec![Paint::solid(colours[i as usize % 4])];
    }
    let pane = d.create(p, NodeKind::Rectangle, "Glass", 0.0, 0.0, 600.0, 360.0).unwrap();
    let n = d.get_mut(pane).unwrap();
    n.transform = Affine::translate((300.0, 220.0));
    n.corners.radii = [radius; 4];
    n.fills = vec![Paint::solid(Color { a: 0.1, ..Color::WHITE })];
    n.effects = vec![Effect { kind: EffectKind::Glass(g), visible: true }];
    (d, p)
}

/// Rows `y0..` of `full` (as wide as `part`) equal `part`, byte for byte: the largest channel
/// difference and how many bytes differ.
fn diff_rows(full: &Pixmap, y0: u32, part: &Pixmap) -> (u8, usize) {
    let row = full.width() as usize * 4;
    let a = &full.data()[y0 as usize * row..][..part.data().len()];
    a.iter().zip(part.data()).fold((0, 0), |(m, n), (x, y)| (m.max(x.abs_diff(*y)), n + usize::from(x != y)))
}

/// A strong, wide light and nothing that blurs or bends: the light reaches furthest in, and the
/// backdrop effects read nothing past the layer, so a cut through the pane gets no margin.
fn wide_light() -> Glass {
    Glass { splay: 1.0, frost: 0.0, refraction: 0.0, light_intensity: 1.0, ..Glass::default() }
}

/// The largest channel difference and bytes differing between the whole render of `d` and renders
/// cut 50 to 80 px inside the pane's top edge (y = 440 px), as bands and canvas patches cut it:
/// rendered with the backdrop reach as a margin above and below, keeping 400 rows from the cut.
fn cut_diffs(d: &Document, p: NodeId) -> Vec<(u32, (u8, usize))> {
    let opts = Options::default();
    let full = render_view_single(d, p, &View { origin: (0.0, 0.0), scale: 2.0, width: 2400, height: 1600 }, &opts).unwrap();
    let margin = crate::backdrop_reach(crate::max_backdrop_blur(d), 2.0).ceil() as u32 + 1;
    [490u32, 505, 520]
        .into_iter()
        .map(|cut| {
            let top = cut - margin;
            let v = View { origin: (0.0, f64::from(top) / 2.0), scale: 2.0, width: 2400, height: 400 + 2 * margin };
            let part = render_view_single(d, p, &v, &opts).unwrap();
            let kept = part.clone_rect(tiny_skia::IntRect::from_xywh(0, margin as i32, 2400, 400).unwrap()).unwrap();
            (cut, diff_rows(&full, cut, &kept))
        })
        .collect()
}

#[test]
fn a_cut_through_the_pane_matches_the_whole_render() {
    // Square corners: straight edges on whole pixels raster the same in any cut.
    for g in [wide_light(), Glass::default(), Glass { splay: 0.5, frost: 0.0, refraction: 0.0, ..Glass::default() }] {
        let (d, p) = pane_over_stripes(g.clone(), 0.0);
        for (cut, diff) in cut_diffs(&d, p) {
            assert_eq!(diff, (0, 0), "cut at {cut} px with {g:?}");
        }
    }
}
