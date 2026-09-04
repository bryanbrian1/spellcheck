// Release builds are a GUI app, not a console app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    leaguechecker::run();
}
