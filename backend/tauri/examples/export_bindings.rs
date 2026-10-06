fn main() -> Result<(), Box<dyn std::error::Error>> {
    let check = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--check") => true,
        _ => return Err("Usage: export_bindings [--check]".into()),
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/bindings.ts");
    app_lib::bindings::export(&path, check)
}
