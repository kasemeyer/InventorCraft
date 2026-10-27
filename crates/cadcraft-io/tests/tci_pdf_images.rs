//! TCI-CAD (ours, not CADCraft's): the one vendored PDF patch, `pdf_sheets_with_images` (R20, as
//! amended by R60: page images only). A picture is drawn on its own page, over its rectangle, before
//! (under) the page's vector strokes; a picture that does not decode is left out.

use std::ops::Deref;

use cadcraft_doc::{Common, Drawing, EntityKind, Layout, Line, PageSetup, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::PdfOptions;
use cadcraft_io::pdf::{PageImage, pdf_sheets_with_images};
use craft_pdf::syntax::object::{Dict, Name, Stream};

#[path = "support/pdf_read.rs"]
mod pdf_read;

/// An inch drawing with layouts `SHEET1` and `SHEET2` (ANSI B landscape, 17 x 11 in), each with
/// one line from (1, 1) to (5, 1).
fn drawing() -> Drawing {
    let mut d = Drawing::new_imperial();
    for (i, name) in ["SHEET1", "SHEET2"].into_iter().enumerate() {
        let mut l = Layout::new(name, (i + 1) as _);
        l.page =
            PageSetup { paper: "ANSI B".into(), width_mm: 279.4, height_mm: 431.8, margins_mm: [0.0; 4], landscape: true, ..PageSetup::default() };
        d.layouts.push(l);
        let line = Line { a: Vec3::new(1.0, 1.0, 0.0), b: Vec3::new(5.0, 1.0, 0.0) };
        d.add(&Space::Paper(name.into()), Common::default(), EntityKind::Line(line)).unwrap();
    }
    d
}

fn spaces() -> Vec<Space> {
    vec![Space::Paper("SHEET1".into()), Space::Paper("SHEET2".into())]
}

/// A 2 x 2 px opaque grey PNG.
fn png() -> Vec<u8> {
    let mut p = tiny_skia::Pixmap::new(2, 2).unwrap();
    p.fill(tiny_skia::Color::from_rgba8(190, 190, 190, 255));
    p.encode_png().unwrap()
}

fn options() -> PdfOptions {
    PdfOptions { compress: false, ..PdfOptions::default() }
}

/// Each page's image XObjects: (width, height) in pixels.
fn images(bytes: &[u8]) -> Vec<Vec<(f64, f64)>> {
    let pdf = craft_pdf::open_slice(bytes, None).expect("the PDF opens");
    pdf.pages()
        .iter()
        .map(|page| {
            let raw = page.raw();
            let Some(xo) = raw.get::<Dict<'_>>(b"Resources").and_then(|r| r.get::<Dict<'_>>(b"XObject")) else { return Vec::new() };
            let keys: Vec<Vec<u8>> = xo.keys().map(|k| k.deref().to_vec()).collect();
            keys.iter()
                .filter_map(|k| xo.get::<Stream<'_>>(k.as_slice()))
                .filter(|s| s.dict().get::<Name<'_>>(b"Subtype").is_some_and(|n| n.as_str() == "Image"))
                .map(|s| (s.dict().get::<f64>(b"Width").unwrap_or(0.0), s.dict().get::<f64>(b"Height").unwrap_or(0.0)))
                .collect()
        })
        .collect()
}

#[test]
fn an_image_lands_under_the_strokes() {
    let d = drawing();
    let im = PageImage { sheet: 0, rect_in: [1.0, 2.0, 3.0, 4.0], png: png() };
    let bytes = pdf_sheets_with_images(&d, &spaces()[..1], &options(), &[im]).unwrap();
    assert_eq!(images(&bytes), vec![vec![(2.0, 2.0)]]);
    let doc = pdf_read::read(&bytes);
    let ops = pdf_read::ops(doc.content());
    let image = ops.iter().position(|o| o.op == "Do").expect("the image is drawn");
    let stroke = ops.iter().position(|o| o.op == "S").expect("the line is stroked");
    assert!(image < stroke, "the image ({image}) is drawn after the first stroke ({stroke})");
    // The picture's matrix: 2 in wide (144 pt), 2 in high, its top-left corner at (1, 4) in, so
    // its first row is at the top (y up in PDF space: a = 144, d = 144 with the flip folded in).
    let place = ops.iter().take(image).filter(|o| o.op == "cm").fold([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], |m, o| {
        let n = &o.nums;
        [
            n[0] * m[0] + n[1] * m[2],
            n[0] * m[1] + n[1] * m[3],
            n[2] * m[0] + n[3] * m[2],
            n[2] * m[1] + n[3] * m[3],
            n[4] * m[0] + n[5] * m[2] + m[4],
            n[4] * m[1] + n[5] * m[3] + m[5],
        ]
    });
    // Unit square → page: (0, 0) is the picture's bottom-left, (1, 1) its top-right, in points.
    let at = |u: f64, v: f64| (place[0] * u + place[2] * v + place[4], place[1] * u + place[3] * v + place[5]);
    let close = |p: (f64, f64), q: (f64, f64)| (p.0 - q.0).abs() < 1e-3 && (p.1 - q.1).abs() < 1e-3;
    assert!(close(at(0.0, 0.0), (72.0, 144.0)) && close(at(1.0, 1.0), (216.0, 288.0)), "{place:?}");
}

#[test]
fn images_go_on_their_own_sheet_and_bad_pictures_are_left_out() {
    let d = drawing();
    let ims = [
        PageImage { sheet: 1, rect_in: [1.0, 1.0, 2.0, 2.0], png: png() },
        PageImage { sheet: 1, rect_in: [1.0, 1.0, 2.0, 2.0], png: b"not a png".to_vec() },
        PageImage { sheet: 1, rect_in: [3.0, 3.0, 3.0, 5.0], png: png() },
        PageImage { sheet: 7, rect_in: [1.0, 1.0, 2.0, 2.0], png: png() },
    ];
    let bytes = pdf_sheets_with_images(&d, &spaces(), &options(), &ims).unwrap();
    assert_eq!(images(&bytes), vec![vec![], vec![(2.0, 2.0)]]);
    let doc = pdf_read::read(&bytes);
    assert_eq!(doc.pages.len(), 2);
    assert_eq!(doc.pages[1].media, [0.0, 0.0, 1224.0, 792.0]);
    // Without images it is `pdf_sheets`.
    let plain = cadcraft_io::pdf::pdf_sheets(&d, &spaces(), &options()).unwrap();
    assert_eq!(pdf_sheets_with_images(&d, &spaces(), &options(), &[]).unwrap().len(), plain.len());
    assert_eq!(images(&plain), vec![vec![], vec![]]);
}
