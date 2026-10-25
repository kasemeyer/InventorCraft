use super::starts_new_design;

#[test]
fn every_new_design_command_starts_a_new_design() {
    for id in ["file.new", "document.new_part", "document.new_assembly"] {
        assert!(starts_new_design(id), "{id}");
    }
    for id in ["doc.open", "doc.recover", "file.save", "document.kind", "document.inspect"] {
        assert!(!starts_new_design(id), "{id} does not start a new design");
    }
}
