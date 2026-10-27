//! PDF and DXF output of a drawing (spec §4.7) through the vendored CADCraft writers.
//!
//! - **PDF:** one page per sheet at its ANSI size in points (D = 2448 × 1584 pt), vector strokes
//!   (stroke-font text is geometry; searchable text is deferred, R60), and each shaded view's
//!   picture under its curves as a [`PageImage`] over [`raster_rect`]. The picture is drawn again
//!   at [`PDF_PX_PER_IN`] when a rasterizer draws and the model is the one the view shows; else
//!   the stored picture is used only when it is fresh ([`raster::fresh`]: drawn of the model
//!   revision, orientation and scale the view shows, its pixel size what the view draws now), so
//!   a stale picture is never stretched. A view with no fresh picture prints as lines only.
//! - **DXF:** one sheet's paper space through `cadcraft_io::write_with`, R2018 (default) or R2000.
//!   Pictures are not written (they are not on the paper); entities the view styles suppress
//!   (invisible, R61) are left out, with every other layout.

use cadcraft_doc::{Drawing, Space};
use cadcraft_io::pdf::{PageImage, pdf_sheets_with_images};
use cadcraft_io::{PdfOptions, SaveOptions, Version};
use solidcraft_doc::{DrawingModel, DrawingView};

use super::model_ref::Loads;
use super::raster::{self, PDF_PX_PER_IN, raster_rect};
use crate::{EngineError, Result, Session};

/// A written PDF: its bytes and how many pictures it holds.
pub(crate) struct PdfOut {
    pub bytes: Vec<u8>,
    pub images: usize,
}

/// The picture of shaded view `v` of `m` for a PDF: drawn again at `PDF_PX_PER_IN` (a rasterizer
/// draws and the model is at the view's revision), else its stored picture when fresh
/// ([`raster::fresh`]), else none.
fn picture(s: &Session, m: &DrawingModel, loads: &mut Loads, v: &DrawingView) -> Option<Vec<u8>> {
    let drawn = s.view_rasterizer.as_ref().and_then(|_| {
        let loaded = loads.get(s, &v.model).filter(|l| l.revision == v.revision)?;
        raster::rasterize(s, &loaded, v, PDF_PX_PER_IN).ok().flatten()
    });
    match drawn {
        Some(r) => Some(r.png),
        None => v.raster.as_ref().filter(|_| raster::fresh(m, v)).map(|r| r.png.clone()),
    }
}

/// The pictures of the shaded views on `sheets` (indices into `m.sheets`, in page order).
pub(crate) fn page_images(s: &Session, m: &DrawingModel, sheets: &[usize]) -> Vec<PageImage> {
    let mut loads = Loads::default();
    let mut out = Vec::new();
    for (page, sheet) in sheets.iter().enumerate() {
        for v in m.views.values().filter(|v| v.sheet == *sheet && v.shaded) {
            let Some(rect) = raster_rect(m, v) else { continue };
            if let Some(png) = picture(s, m, &mut loads, v) {
                out.push(PageImage { sheet: page, rect_in: rect, png });
            }
        }
    }
    out
}

/// The PDF of `sheets` (indices into `m.sheets`, one page each, in that order).
pub(crate) fn pdf(s: &Session, m: &DrawingModel, sheets: &[usize], title: &str) -> Result<PdfOut> {
    let spaces = sheets
        .iter()
        .map(|i| m.sheets.get(*i).map(|sh| Space::Paper(sh.layout.clone())).ok_or_else(|| EngineError::Other(format!("sheet {i} does not exist"))))
        .collect::<Result<Vec<_>>>()?;
    let images = page_images(s, m, sheets);
    let o = PdfOptions { title: title.to_string(), compress: true, ..PdfOptions::default() };
    let bytes = pdf_sheets_with_images(&m.paper, &spaces, &o, &images).map_err(|e| EngineError::Other(e.to_string()))?;
    Ok(PdfOut { bytes, images: images.len() })
}

/// The paper of sheet `sheet` alone, as it is written to DXF: the other layouts and every
/// invisible (style-suppressed, R61) entity left out, groups kept to what remains, the sheet's
/// layout the active one.
pub(crate) fn sheet_paper(m: &DrawingModel, sheet: usize) -> Result<Drawing> {
    let layout = m.sheets.get(sheet).map(|sh| sh.layout.clone()).ok_or_else(|| EngineError::Other(format!("sheet {sheet} does not exist")))?;
    let mut d = m.paper.clone();
    d.layouts.retain(|l| l.name == layout);
    let hidden: Vec<_> = d.layouts.iter().flat_map(|l| l.entities.iter()).filter(|e| !e.common.visible).map(|e| e.handle).collect();
    for h in hidden {
        d.remove_entity(h);
    }
    let kept: std::collections::BTreeSet<_> = d.layouts.iter().flat_map(|l| l.entities.iter()).map(|e| e.handle).collect();
    for g in &mut d.groups {
        g.members.retain(|h| kept.contains(h));
    }
    d.groups.retain(|g| !g.members.is_empty());
    d.active_layout = Some(layout);
    Ok(d)
}

/// The DXF of sheet `sheet` at `version`; the file name only names the format.
pub(crate) fn dxf(m: &DrawingModel, sheet: usize, version: Version) -> Result<Vec<u8>> {
    let d = sheet_paper(m, sheet)?;
    cadcraft_io::write_with(&d, "sheet.dxf", &SaveOptions { version: Some(version), binary: false }).map_err(|e| EngineError::Other(e.to_string()))
}
