fn main() {
    tauri_build::try_build(tauri_build::Attributes::new()).unwrap_or_else(|e| {
        eprintln!("{e:#}");
        std::process::exit(1);
    });
}
