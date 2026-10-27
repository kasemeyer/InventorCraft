#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The vendored CADCraft drawing crates sit below the engine (Plan D-1 Task 1).

#[test]
fn cadcraft_crates_sit_below_the_engine() {
    for (name, layer) in [
        ("cadcraft-color", 0),
        ("cadcraft-geom", 0),
        ("cadcraft-dxf", 0),
        ("cadcraft-doc", 1),
        ("cadcraft-fonts", 1),
        ("cadcraft-render", 2),
        ("cadcraft-io", 3),
    ] {
        assert_eq!(super::classify(name), Some(super::Class::Layer(layer)), "{name}");
    }
}
