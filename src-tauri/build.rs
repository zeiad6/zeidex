fn main() {
    tauri_build::build();

    // The frontend is embedded at compile time by `generate_context!`, but
    // tauri-build only watches `tauri.conf.json`, `capabilities/` and the
    // bundled resources. A `cargo build` after a frontend-only change therefore
    // reused the previous crate and shipped a binary carrying the old UI, which
    // is invisible until someone opens it. Watching the dist directory makes
    // the shell crate rebuild whenever the page it embeds changes.
    println!("cargo:rerun-if-changed=../app/dist");
}
