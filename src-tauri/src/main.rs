// Hide the console window in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "desktop")]
fn main() {
    codecal_lib::run();
}
#[cfg(not(feature = "desktop"))]
fn main() {}
