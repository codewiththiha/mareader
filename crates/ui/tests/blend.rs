//! Blend mode: the backdrop takes the page's own paper, themed.

use egui_kittest::Harness;
use image::RgbaImage;
use mareader_appearance::Rgb;
use mareader_settings::PaperArea;
use mareader_ui::MareaderApp;

use crate::common::scroll;

type Out = Result<(), String>;

const SIZE: egui::Vec2 = egui::vec2(900.0, 700.0);
/// The scan's first chapter's paper, and the plate over each page.
const CREAM: Rgb = Rgb(250, 242, 224);
const PLATE: Rgb = Rgb(120, 135, 153);
const BLUE: Rgb = Rgb(219, 230, 242);

fn reader(options: &[(&str, &str)]) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    for (k, v) in options {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(300)
        .wgpu()
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

fn paper(h: &Harness<'static, MareaderApp>, area: PaperArea) -> Option<Rgb> {
    let app = h.state();
    app.documents().paper(app.active_pane(), area)
}

fn near(a: Rgb, b: Rgb, by: i32) -> bool {
    let d = |x: u8, y: u8| (i32::from(x) - i32::from(y)).abs();
    d(a.0, b.0) <= by && d(a.1, b.1) <= by && d(a.2, b.2) <= by
}

/// Mid-way down the margin left of the first page, in pixels.
fn gutter(h: &Harness<'static, MareaderApp>, image: &RgbaImage) -> Result<(u32, u32), String> {
    let pane = h.state().active_pane();
    let sheets = h.state().documents().sheets(pane);
    let page = sheets.first().map(|s| s.rect).ok_or("no page drawn")?;
    let ppp = image.width() as f32 / SIZE.x;
    let x = (page.left() - 12.0).max(1.0) * ppp;
    Ok((x as u32, (page.center().y * ppp) as u32))
}

fn pixel(image: &RgbaImage, (x, y): (u32, u32)) -> Rgb {
    let [r, g, b, _] = image.get_pixel(x, y).0;
    Rgb(r, g, b)
}

#[test]
fn the_gutter_takes_the_page_paper_through_the_theme() -> Out {
    let base = [("pdf", "scan:6"), ("margin", "48"), ("preset", "sepia")];
    let mut on = base.to_vec();
    on.extend([("blend", "true"), ("blend_area", "edges")]);
    let mut h = reader(&on);
    let raw = paper(&h, PaperArea::Edges).ok_or("no paper was read")?;
    assert!(near(raw, CREAM, 2), "read {raw:?}");
    let tone = h.state().settings().appearance.page_tone();
    let want = Rgb::from_unit(tone.apply(raw.to_unit()));
    let image = h.render()?;
    let got = pixel(&image, gutter(&h, &image)?);
    assert!(near(got, want, 3), "gutter {got:?}, baked {want:?}");
    // Blend off: the theme's own paper, whatever the page shows.
    let mut h = reader(&base);
    assert_eq!(paper(&h, PaperArea::Edges), None, "nothing is read");
    let image = h.render()?;
    let got = pixel(&image, gutter(&h, &image)?);
    let theme = h.state().settings().appearance.ui_tokens().paper;
    assert!(near(got, theme, 3), "gutter {got:?}, theme {theme:?}");
    Ok(())
}

#[test]
fn whole_page_reads_the_plate_and_edges_the_margins() -> Out {
    let h = reader(&[("pdf", "scan:6"), ("blend", "true")]);
    let whole = paper(&h, PaperArea::WholePage).ok_or("no whole-page paper")?;
    let edges = paper(&h, PaperArea::Edges).ok_or("no edge paper")?;
    assert!(near(whole, PLATE, 2), "whole {whole:?}");
    assert!(near(edges, CREAM, 2), "edges {edges:?}");
    Ok(())
}

#[test]
fn the_texture_runs_from_the_page_into_the_gutter() -> Out {
    let opts = [
        ("pdf", "scan:6"),
        ("margin", "48"),
        ("blend", "true"),
        ("blend_area", "edges"),
        ("texture", "lined"),
    ];
    let mut h = reader(&opts);
    let image = h.render()?;
    let (gx, _) = gutter(&h, &image)?;
    let pane = h.state().active_pane();
    let page = h.state().documents().sheets(pane).first().map(|s| s.rect);
    let ppp = image.width() as f32 / SIZE.x;
    // Inside the page, in the paper strip left of the plate.
    let px = ((page.ok_or("no page")?.left() + 6.0) * ppp) as u32;
    let rows = |x: u32| -> Vec<u32> {
        let ys = image.height() / 3..image.height() * 2 / 3;
        let base = ys.clone().map(|y| luma(&image, x, y)).max().unwrap_or(0);
        ys.filter(|&y| luma(&image, x, y) < base - 4).collect()
    };
    let (outside, inside) = (rows(gx), rows(px));
    assert!(
        outside.len() >= 8,
        "{} rule rows in the gutter",
        outside.len()
    );
    let shared = outside.iter().filter(|y| inside.contains(y)).count();
    assert!(
        shared * 10 >= outside.len() * 8,
        "rules jar at the page edge: {shared} of {} rows line up",
        outside.len()
    );
    Ok(())
}

fn luma(image: &RgbaImage, x: u32, y: u32) -> i32 {
    let [r, g, b, _] = image.get_pixel(x, y).0;
    (i32::from(r) * 2126 + i32::from(g) * 7152 + i32::from(b) * 722) / 10_000
}

#[test]
fn scrolling_between_chapters_mixes_their_papers() -> Out {
    let mut h = reader(&[
        ("pdf", "scan:8"),
        ("blend", "true"),
        ("blend_area", "edges"),
    ]);
    let mut seen = Vec::new();
    for _ in 0..30 {
        seen.extend(paper(&h, PaperArea::Edges));
        scroll(&mut h, 280.0);
    }
    let cream = seen.iter().any(|c| near(*c, CREAM, 2));
    let blue = seen.iter().any(|c| near(*c, BLUE, 2));
    assert!(cream && blue, "both chapters' paper: {seen:?}");
    let between = seen
        .iter()
        .any(|c| !near(*c, CREAM, 4) && !near(*c, BLUE, 4) && c.2 > CREAM.2 && c.2 < BLUE.2);
    assert!(between, "a mix on the way: {seen:?}");
    Ok(())
}

#[test]
fn a_book_focused_beside_a_pdf_keeps_the_pdf_s_paper() -> Out {
    let mut h = reader(&[
        ("panes", "2"),
        ("focus", "1"),
        ("pdf", "scan:6"),
        ("focus", "2"),
        ("epub", "sample:3"),
        ("blend", "true"),
        ("blend_area", "edges"),
    ]);
    h.run_steps(3);
    let panes = h.state().panes();
    let (Some(&pdf), Some(&book)) = (panes.first(), panes.last()) else {
        return Err("two panes".into());
    };
    let app = h.state();
    assert_eq!(app.active_pane(), book, "the book is focused");
    let raw = app.documents().paper(pdf, PaperArea::Edges);
    assert!(raw.is_some(), "the scan's paper was read");
    // The book has none, so every pane keeps the last PDF's.
    assert_eq!(app.blend_paper(book), raw);
    assert_eq!(app.blend_paper(pdf), raw);
    // Each pane its own: the book goes back to its text paper.
    h.state_mut().set_option("independent", "true")?;
    h.run();
    assert_eq!(h.state().blend_paper(book), None);
    assert_eq!(h.state().blend_paper(pdf), raw);
    Ok(())
}
