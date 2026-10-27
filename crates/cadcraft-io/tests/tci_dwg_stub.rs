#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! TCI-CAD: DWG is stubbed out of the vendored `cadcraft-io` (no DWG bridge, no acadrust).
//! Reading DWG bytes and writing `.dwg` or `.dwt` all fail with one plain message.

use cadcraft_doc::Drawing;
use cadcraft_io::SaveOptions;

const REFUSAL: &str = "DWG is not supported in TCI-CAD";

#[test]
fn dwg_is_refused_not_bridged() {
    let read = cadcraft_io::read(b"AC1032\0\0\0\0\0\0", "x.dwg").map(|_| ()).unwrap_err();
    assert!(read.to_string().contains(REFUSAL), "{read}");
    assert_eq!(read.to_string(), "DWG is not supported in TCI-CAD (PDF and DXF only)");

    let write = cadcraft_io::write_with(&Drawing::default(), "x.dwg", &SaveOptions::default()).unwrap_err();
    assert_eq!(write.to_string(), read.to_string());

    let template = cadcraft_io::write(&Drawing::default(), "x.dwt").unwrap_err();
    assert_eq!(template.to_string(), read.to_string());
}
