//! The committed TypeScript bindings must match the Rust IPC surface.
//! `UPDATE_BINDINGS=1 cargo test --test bindings` rewrites them without
//! starting the app.

use std::path::Path;

#[test]
fn committed_bindings_are_current() {
    let dir = tempfile::tempdir().unwrap();
    let fresh = dir.path().join("bindings.ts");
    bitplane_cc_lib::export_bindings(&bitplane_cc_lib::ipc::builder(), &fresh).unwrap();
    let fresh = std::fs::read_to_string(fresh).unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(bitplane_cc_lib::BINDINGS_PATH);
    if std::env::var_os("UPDATE_BINDINGS").is_some() {
        std::fs::write(&path, &fresh).unwrap();
    }
    let committed = std::fs::read_to_string(path).unwrap_or_default();
    assert!(
        fresh == committed,
        "src/ipc/bindings.ts is stale: run `UPDATE_BINDINGS=1 cargo test --test bindings` or start a debug build"
    );
}
