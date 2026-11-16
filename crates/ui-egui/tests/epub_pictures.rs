//! GPU tests for EPUB `theme_pictures` on fixed pages and thumbnails.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use image::RgbaImage;
use mareader_engine::command::Command;
use mareader_engine::documents::OpenRequest;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::ui_command::Sent;

type Out = Result<(), String>;

const CANVAS: egui::Vec2 = egui::vec2(900.0, 700.0);

fn epub_harness(options: &[(&str, &str)], fixed: bool) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("animations", "false").ok();
    for (k, v) in options {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(CANVAS)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .wgpu()
        .build_eframe(|_| app);
    if fixed {
        let bytes = mareader_epub::sample::fixed_epub("Fixed Artwork");
        let open = OpenRequest::bytes(bytes, "fixed.epub");
        h.state_mut().queue(Sent::Engine(Command::Open(open)));
    }
    h.run_steps(6);
    h.run();
    h
}

fn brightness(image: &RgbaImage, x: u32, y: u32) -> i32 {
    let [r, g, b, _] = image.get_pixel(x, y).0;
    (i32::from(r) * 2126 + i32::from(g) * 7152 + i32::from(b) * 722) / 10_000
}

fn max_shift(a: &RgbaImage, b: &RgbaImage, points: &[(u32, u32)]) -> i32 {
    points
        .iter()
        .map(|&(x, y)| (brightness(a, x, y) - brightness(b, x, y)).abs())
        .max()
        .unwrap_or(0)
}

fn thumb_points(h: &Harness<'static, MareaderApp>, image: &RgbaImage) -> [(u32, u32); 3] {
    let cell = h.get_by_label("Page 1").rect();
    let (w, ht) = image.dimensions();
    let (sx, sy) = (w as f32 / CANVAS.x, ht as f32 / CANVAS.y);
    let px = |x: f32, y: f32| {
        (
            (x * sx).round().clamp(0.0, (w - 1) as f32) as u32,
            (y * sy).round().clamp(0.0, (ht - 1) as f32) as u32,
        )
    };
    let cx = cell.center().x;
    [
        px(cx, cell.top() + cell.height() * 0.25),
        px(cx, cell.top() + cell.height() * 0.42),
        px(cx, cell.top() + cell.height() * 0.62),
    ]
}

#[test]
fn fixed_layout_epub_follows_theme_pictures_in_dark_dim_and_black() -> Out {
    let mut h = epub_harness(&[], true);
    let light = h.render()?;
    let (w, ht) = light.dimensions();
    let points = [(w / 2, ht / 2), (w / 2, ht / 3), (w * 3 / 7, ht * 3 / 5)];

    for theme in ["dark", "dim", "black"] {
        h.state_mut().set_option("theme", theme)?;
        h.state_mut().set_option("theme_pictures", "false")?;
        h.run_steps(2);
        h.run();
        let raw = h.render()?;
        let kept = max_shift(&light, &raw, &points);
        assert!(
            kept <= 3,
            "{theme} with theme_pictures=false should keep fixed art colours: shift={kept}"
        );

        h.state_mut().set_option("theme_pictures", "true")?;
        h.run_steps(2);
        h.run();
        let themed = h.render()?;
        let moved = max_shift(&raw, &themed, &points);
        assert!(
            moved > 40,
            "{theme} with theme_pictures=true should theme fixed art: shift={moved}"
        );

        h.state_mut().set_option("theme_pictures", "false")?;
        h.run_steps(2);
        h.run();
        let restored = h.render()?;
        let back = max_shift(&raw, &restored, &points);
        assert!(
            back <= 3,
            "{theme} toggling theme_pictures back off should restore fixed art: shift={back}"
        );
    }
    Ok(())
}

#[test]
fn settings_switch_toggles_fixed_layout_epub_theme_pictures() -> Out {
    let mut h = epub_harness(&[("theme", "dark")], true);
    let before = h.render()?;
    let (w, ht) = before.dimensions();
    let points = [(w / 2, ht / 2), (w / 2, ht / 3), (w * 3 / 7, ht * 3 / 5)];

    h.state_mut().set_option("settings", "fonts")?;
    h.run_steps(3);
    let switch = "Theme pictures";
    h.get_by_role_and_label(Role::CheckBox, switch)
        .scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::CheckBox, switch).click();
    h.run_steps(2);
    assert!(h.state().settings().text.theme_pictures);
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    h.run();

    let after = h.render()?;
    let moved = max_shift(&before, &after, &points);
    assert!(
        moved > 40,
        "clicking Theme pictures should visibly theme fixed EPUB: shift={moved}"
    );
    Ok(())
}

#[test]
fn epub_thumbnails_follow_theme_pictures_for_reflow_and_fixed() -> Out {
    let reflow: &[(&str, &str)] = &[("epub", "sample:3"), ("sidebar", "thumbnails")];
    let fixed: &[(&str, &str)] = &[("sidebar", "thumbnails")];
    for (kind, opts, is_fixed) in [("reflow", reflow, false), ("fixed", fixed, true)] {
        let mut h = epub_harness(opts, is_fixed);
        let light = h.render()?;
        let points = thumb_points(&h, &light);

        h.state_mut().set_option("theme", "dark")?;
        h.state_mut().set_option("theme_pictures", "false")?;
        h.run_steps(3);
        h.run();
        let raw = h.render()?;
        let kept = max_shift(&light, &raw, &points);
        assert!(
            kept <= 4,
            "{kind} thumbnail with theme_pictures=false should keep colours: shift={kept}"
        );

        h.state_mut().set_option("theme_pictures", "true")?;
        h.run_steps(3);
        h.run();
        let themed = h.render()?;
        let moved = max_shift(&raw, &themed, &points);
        assert!(
            moved > 35,
            "{kind} thumbnail with theme_pictures=true should follow dark theme: shift={moved}"
        );
    }
    Ok(())
}
