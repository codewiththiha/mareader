//! Launch parsing: the URL (`?open=/samples/…&blend=1` — the web test hook's
//! exact contract, now owned by the shell) and the OS-open plumbing (Tauri's
//! pending-file handoff), forwarded into whichever runtime is live.

use runtime_contract::boundary::LaunchDocument;

/// The URL launch for a reader boot. Samples-only guard included: the hook
/// opens exactly the fixtures the repository ships.
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

/// The unnamed launch: no book, no resume point — the URL overrides and the
/// OS-open handoff fill in what they know.
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

/// The OS-open plumbing (shell-lifetime): pull the pending file once at boot
/// and forward the push stream — into the LIVE runtime via its command
/// export. The old `init_open_file_handling` moved here: OS events are
/// window services (§2), and they outlive every runtime session.
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
