//! Dev-only runtime self-check. Run once after mount (debug builds): exercises
//! the engine end-to-end against a bundled sample PDF and logs PASS/FAIL to the
//! console. This is how we verify rendering + text + search WITHOUT screenshots.

use leptos::task::spawn_local;

use crate::api::engine;
use crate::core::bridge;

const SAMPLE: &str = "/samples/sample.pdf";

async fn run() {
    log("[selftest] starting");
    let mut pass = true;
    let mut check = |ok: bool, label: &str| {
        log(&format!(
            "[selftest] {} {}",
            if ok { "OK" } else { "FAIL" },
            label
        ));
        pass = pass && ok;
    };

    let engine_v = bridge::version();
    check(&engine_v == "0.1.0", &format!("engine v{engine_v}"));

    let opened = engine::open(SAMPLE).await;
    let open_ok = match &opened {
        Ok(m) => {
            check(
                m.num_pages > 0 && m.page1_size.width > 0.0 && m.page1_size.height > 0.0,
                &format!(
                    "open sample.pdf (numPages={}, page1={}x{})",
                    m.num_pages, m.page1_size.width, m.page1_size.height
                ),
            );
            true
        }
        Err(e) => {
            check(false, &format!("open sample.pdf ({e})"));
            false
        }
    };

    if open_ok {
        if let Ok(m) = &opened {
            let pc = engine::page_count();
            check(pc == m.num_pages, &format!("page_count={pc} matches numPages"));
        }
        let host_id = "selftest-host";
        let canvas_id = "selftest-canvas";

        let doc = web_sys::window().and_then(|w| w.document()).unwrap();
        let host = doc.create_element("div").unwrap();
        host.set_id(host_id);
        let _ = host.set_attribute("class", "pdf-page");
        // style() would resolve to tachys ElementExt::style (needs an arg); use set_attribute.
        let _ = host.set_attribute("style", "position:fixed;left:-10000px;top:0");
        let canvas = doc.create_element("canvas").unwrap();
        canvas.set_id(canvas_id);
        let text_layer = doc.create_element("div").unwrap();
        let _ = text_layer.set_attribute("class", "textLayer");
        host.append_child(&canvas).unwrap();
        host.append_child(&text_layer).unwrap();
        let body = doc.body().unwrap();
        body.append_child(&host).unwrap();

        engine::register_page(1, canvas_id, Some(host_id));
        match engine::render_page(canvas_id, 1.0, true).await {
            Ok(r) => {
                check(
                    r.width > 0.0 && r.height > 0.0,
                    &format!("render page1 (width={}, height={})", r.width, r.height),
                );
            }
            Err(e) => check(false, &format!("render page1 ({e})")),
        }

        let spans = host
            .query_selector_all(".textLayer span")
            .map(|n| n.length())
            .unwrap_or(0);
        check(spans > 0, &format!("text layer spans={spans} (>0)"));

        let _ = engine::build_search_index().await;
        match engine::search("tracemonkey").await {
            Ok(resp) => check(resp.total > 0, &format!("search 'tracemonkey' hits={}", resp.total)),
            Err(e) => check(false, &format!("search 'tracemonkey' ({e})")),
        }

        engine::unregister_page(canvas_id);
        let _ = host.remove();
    }

    engine::destroy().await;
    log(&format!(
        "[selftest] {}",
        if pass { "PASS" } else { "FAIL (see steps above)" }
    ));
}

fn log(msg: &str) {
    web_sys::console::log_1(&msg.into());
}

/// Call once from the app after mount. Only runs meaningful checks in debug
/// builds, but logs a banner in release so we know the hook fired.
pub fn selftest() {
    #[cfg(debug_assertions)]
    {
        spawn_local(async move {
            run().await;
        });
    }
    #[cfg(not(debug_assertions))]
    {
        web_sys::console::log_1(&"[selftest] skipped (release build)".into());
    }
}
