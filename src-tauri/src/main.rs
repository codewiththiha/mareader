// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Load the workspace .env so FM_BRIDGE_BIN reaches the process
    // environment before Tauri (and the AI provider) starts up.
    let _ = dotenvy::dotenv();

    pdf_lib::run()
}
