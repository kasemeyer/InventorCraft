//! Drawing templates (#552): a `.dwt` is written as DWG content and read back, and a template
//! named `.dwt` that holds DXF text reads too.

use cadcraft_doc::{Drawing, Layer};
use cadcraft_io::{read, write};

fn office() -> Drawing {
    let mut d = Drawing::new_metric();
    d.layers.push(Layer::new("A-WALL"));
    d
}

// TCI-CAD: test `dwt_is_written_as_dwg_and_read_back` removed (it needs the DWG bridge, not vendored).

#[test]
fn a_dxf_template_reads_under_either_name() {
    let dxf = write(&office(), "office.dxf").unwrap();
    for name in ["office.dxf", "office.dwt"] {
        assert!(read(&dxf, name).unwrap().layers.iter().any(|l| l.name == "A-WALL"), "{name}");
    }
}
