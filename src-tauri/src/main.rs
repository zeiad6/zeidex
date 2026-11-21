// Zeidex's binary is a thin shim: the app lives in `zeidex_lib::run` so the same
// entry point is reachable from integration tests.
fn main() {
    if let Err(err) = zeidex_lib::run() {
        eprintln!("Zeidex failed to start: {err:#}");
        std::process::exit(1);
    }
}
