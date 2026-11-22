// The binary is a thin shim: the app lives in `zeidex_lib::run` so the same
// entry point is reachable from integration tests.
//
// `windows_subsystem = "windows"` is what makes a release build a GUI process
// rather than a console one. Without it Windows allocates a console for the
// process: a terminal window appears next to Zeidex, and closing that console
// sends CTRL_CLOSE_EVENT to the process group, which terminates the app. The
// attribute is debug-only gated so `cargo run` keeps a console for panics and
// `println!` diagnostics during development.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(err) = zeidex_lib::run() {
        // In a GUI build there is no console to print to, so the failure is also
        // appended to the log file the shell writes next to the database.
        zeidex_lib::report_startup_failure(&format!("{err:#}"));
        eprintln!("Zeidex failed to start: {err:#}");
        std::process::exit(1);
    }
}
