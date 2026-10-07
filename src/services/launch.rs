//! Launch parsing: the URL hook and the OS-open plumbing.

use runtime_contract::boundary::LaunchDocument;

/// The URL launch for a reader boot, samples-only.
pub fn launch_from_url() -> LaunchDocument {
    #[cfg(target_arch = "wasm32")]
    {
        let mut launch = blank_launch();
        if let (Some(window), false) = (web_sys::window(), tauri_bridge::has_tauri()) {
            if let Ok(search) = window.location().search() {
                if let Ok(params) = web_sys::UrlSearchParams::new_with_str(&search) {
                    if params.get("blend").is_some() {
                        launch.blend_override = true;
                    }
                    if let Some(path) = params.get("open") {
                        if path.starts_with("/samples/") {
                            launch.path = path;
                        }
                    }
                }
            }
        }
        launch
    }
    // No URL to read off wasm: the blank launch is the whole answer.
    #[cfg(not(target_arch = "wasm32"))]
    {
        blank_launch()
    }
}

/// The unnamed launch: no book, no resume point.
fn blank_launch() -> LaunchDocument {
    LaunchDocument {
        book_id: None,
        path: String::new(),
        resume_page: 1,
        saved_fraction: None,
        blend_override: false,
        cover_data_url: None,
        display_name: None,
    }
}

/// The OS-open plumbing: the pending file and its push stream.
pub fn install_os_open_handling(state: crate::state::ShellState) {
    #[cfg(target_arch = "wasm32")]
    {
        if !tauri_bridge::has_tauri() {
            return;
        }
        use wasm_bindgen_futures::spawn_local;
        let st = state.clone();
        spawn_local(async move {
            if let Some(path) = tauri_bridge::take_pending_file().await {
                let launch = crate::services::resolve_launch(&path).unwrap_or(LaunchDocument {
                    book_id: None,
                    path,
                    resume_page: 1,
                    saved_fraction: None,
                    blend_override: false,
                    cover_data_url: None,
                    display_name: None,
                });
                st.manager.open_document(&st, launch);
            }
        });
        let st = state.clone();
        app_state::tauri_listen::tauri_listen("document-open-file", move |_ev: web_sys::Event| {
            let st = st.clone();
            spawn_local(async move {
                if let Some(path) = tauri_bridge::take_pending_file().await {
                    let launch = crate::services::resolve_launch(&path).unwrap_or(LaunchDocument {
                        book_id: None,
                        path,
                        resume_page: 1,
                        saved_fraction: None,
                        blend_override: false,
                        cover_data_url: None,
                        display_name: None,
                    });
                    st.manager.open_document(&st, launch);
                }
            });
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = state;
}
