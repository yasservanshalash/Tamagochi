// Deskfolk desktop entry point.
// `windows_subsystem = "windows"` keeps a console window from appearing behind
// the companion in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    deskfolk_lib::run()
}
