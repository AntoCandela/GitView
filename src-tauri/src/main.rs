//! Launches the GitView desktop host without a release-mode Windows console.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    gitview_lib::run();
}
