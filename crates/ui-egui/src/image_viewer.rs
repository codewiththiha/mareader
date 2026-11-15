//! A book's picture opened over the window: the wheel zooms, a drag pans.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};

use egui::{
    Align, Area, Color32, ColorImage, Context, CursorIcon, FontId, Id, Key, Layout, Modifiers,
    Order, Pos2, Rect, Sense, Spinner, TextureHandle, TextureOptions, UiBuilder, Vec2, WidgetInfo,
    WidgetType, pos2, vec2,
};
use mareader_pdf::Bounds;
use mareader_reflow::Picture;
use mareader_session::Document;

use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;

/// Decoded no larger than this on its long edge.
const SIDE: u32 = 4096;
/// The most points one of the picture's pixels may take.
const MOST: f32 = 8.0;
/// Small pictures are fitted no larger than this many times.
const ENLARGE: f32 = 4.0;
const GAP: f32 = 24.0;
const BAR: f32 = 44.0;
/// One button press zooms by this much.
const STEP: f32 = 1.25;

enum Pixels {
    Waiting(Receiver<Option<ColorImage>>),
    Shown(TextureHandle),
    Failed,
}

/// An open picture: its pixels and how it's zoomed and moved.
#[derive(Clone)]
pub struct Viewer {
    label: String,
    /// The pixels' size once decoded; till then, roughly their shape.
    size: Vec2,
    /// Shared by the view's copies, which are one viewer.
    pixels: Arc<Mutex<Pixels>>,
    /// Points per picture pixel; `None` fits the window.
    zoom: Option<f32>,
    /// The picture's centre from the window's, in points.
    pan: Vec2,
}

impl PartialEq for Viewer {
    fn eq(&self, other: &Self) -> bool {
        let same = Arc::ptr_eq(&self.pixels, &other.pixels);
        same && self.zoom == other.zoom && self.pan == other.pan
    }
}

impl std::fmt::Debug for Viewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Viewer({}×{})", self.size.x, self.size.y)
    }
}

impl Viewer {
    /// Opens a book's `picture`, decoded off the window's thread.
    pub fn picture(ctx: &Context, picture: Arc<Picture>) -> Self {
        let side = side(ctx);
        let label = format!("Picture, {} by {} pixels", picture.width, picture.height);
        let size = vec2(picture.width as f32, picture.height as f32);
        Self::open(ctx, (label, size), move || {
            let (w, h, px) = picture.thumbnail(side)?;
            let size = [w as usize, h as usize];
            Some(ColorImage::from_rgba_unmultiplied(size, &px))
        })
    }

    /// Opens box `at` of a PDF's page `page`, drawn large.
    pub fn figure(ctx: &Context, doc: Arc<Document>, page: usize, at: Bounds) -> Self {
        let side = side(ctx);
        let label = format!("Figure on page {}", page + 1);
        let size = vec2(at.x1 - at.x0, at.y1 - at.y0);
        Self::open(ctx, (label, size), move || {
            let raster = doc.render_region(page, at, side).ok()?;
            let size = [raster.width as usize, raster.height as usize];
            Some(ColorImage::from_rgba_premultiplied(size, &raster.pixels))
        })
    }

    fn open(
        ctx: &Context,
        (label, size): (String, Vec2),
        decode: impl FnOnce() -> Option<ColorImage> + Send + 'static,
    ) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        #[cfg(not(target_arch = "wasm32"))]
        let pixels = {
            let ctx = ctx.clone();
            let spawned = std::thread::Builder::new()
                .name("picture".into())
                .spawn(move || {
                    tx.send(decode()).ok();
                    ctx.request_repaint();
                });
            match spawned {
                Ok(_) => Pixels::Waiting(rx),
                Err(_) => Pixels::Failed,
            }
        };
        // Browsers have no threads: decoded at once, nothing to wake.
        #[cfg(target_arch = "wasm32")]
        let pixels = {
            let _ = ctx;
            tx.send(decode()).ok();
            Pixels::Waiting(rx)
        };
        Self {
            label,
            size,
            pixels: Arc::new(Mutex::new(pixels)),
            zoom: None,
            pan: Vec2::ZERO,
        }
    }

    fn poll(&mut self, ctx: &Context) {
        let mut pixels = self.pixels.lock().unwrap_or_else(PoisonError::into_inner);
        let Pixels::Waiting(rx) = &*pixels else {
            return;
        };
        *pixels = match rx.try_recv() {
            Ok(Some(image)) => {
                self.size = vec2(image.width() as f32, image.height() as f32);
                let options =
                    TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear));
                Pixels::Shown(ctx.load_texture("picture", image, options))
            }
            Ok(None) | Err(TryRecvError::Disconnected) => Pixels::Failed,
            Err(TryRecvError::Empty) => return,
        };
    }

    /// Zooms to `to`, keeping the picture's point under `p` still.
    fn zoom_to(&mut self, to: f32, (from, fit): (f32, f32), p: Vec2) {
        let to = to.clamp(fit.min(1.0), MOST.max(fit));
        let q = (p - self.pan) / from;
        self.pan = p - q * to;
        self.zoom = Some(to);
    }
}

/// The longest side a texture may take here, at most [`SIDE`].
fn side(ctx: &Context) -> u32 {
    let most = u32::try_from(ctx.input(|i| i.max_texture_side)).unwrap_or(SIDE);
    SIDE.min(most)
}

fn shown_id() -> Id {
    Id::new("picture viewer shown")
}

/// Whether a viewer showed last pass: the app's keys stand down then.
pub fn is_open(ctx: &Context) -> bool {
    let last = ctx.data(|d| d.get_temp::<u64>(shown_id()));
    last.is_some_and(|n| n + 1 >= ctx.cumulative_pass_nr())
}

/// Draws the viewer over the window; false once it's closed.
pub fn show(ctx: &Context, viewer: &mut Viewer, t: &Tokens) -> bool {
    viewer.poll(ctx);
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(shown_id(), pass));
    let screen = ctx.content_rect();
    let mut open = true;
    Area::new(Id::new("picture viewer"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let sense = Sense::click_and_drag();
            let (rect, back) = ui.allocate_exact_size(screen.size(), sense);
            back.widget_info(|| WidgetInfo::labeled(WidgetType::Image, true, &viewer.label));
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(225));
            let mut room = rect.shrink(GAP);
            room.max.y -= BAR + GAP / 2.0;
            let centre = room.center();
            let fit = (room.width() / viewer.size.x)
                .min(room.height() / viewer.size.y)
                .clamp(0.01, ENLARGE);
            let scale = viewer.zoom.unwrap_or(fit);
            let pointer = ctx.pointer_hover_pos().map_or(Vec2::ZERO, |p| p - centre);
            if back.hovered() {
                let (scroll, pinch) = ctx.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
                let by = pinch * ((scroll.x + scroll.y) * 0.002).exp();
                if (by - 1.0).abs() > 1e-4 {
                    viewer.zoom_to(scale * by, (scale, fit), pointer);
                }
            }
            if back.dragged() {
                viewer.pan += back.drag_delta();
            }
            if back.double_clicked() {
                if viewer.zoom.is_some() {
                    viewer.zoom = None;
                    viewer.pan = Vec2::ZERO;
                } else {
                    let near = if fit < 1.0 { 1.0 } else { fit * 2.0 };
                    viewer.zoom_to(near, (scale, fit), pointer);
                }
            }
            let mut press = keys(ctx);
            let shown = vec2(viewer.size.x, viewer.size.y) * viewer.zoom.unwrap_or(fit);
            let extra = ((shown - room.size()) / 2.0).max(Vec2::ZERO);
            viewer.pan = viewer.pan.clamp(-extra, extra);
            let at = Rect::from_center_size(centre + viewer.pan, shown);
            let pixels = viewer.pixels.lock().unwrap_or_else(PoisonError::into_inner);
            match &*pixels {
                Pixels::Shown(texture) => {
                    let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
                    ui.painter().image(texture.id(), at, uv, Color32::WHITE);
                }
                Pixels::Waiting(_) => {
                    let spin = Rect::from_center_size(centre, Vec2::splat(28.0));
                    ui.put(spin, Spinner::new().color(Color32::from_gray(200)));
                }
                Pixels::Failed => {
                    ui.painter().text(
                        centre,
                        egui::Align2::CENTER_CENTER,
                        "This picture couldn't be opened.",
                        FontId::proportional(t.text),
                        Color32::from_gray(220),
                    );
                }
            }
            drop(pixels);
            let outside = back.interact_pointer_pos().is_some_and(|p| !at.contains(p));
            if back.clicked() && outside {
                press = Some(Press::Close);
            }
            if back.dragged() {
                ctx.set_cursor_icon(CursorIcon::Grabbing);
            } else if back.hovered()
                && (extra.x > 0.0 || extra.y > 0.0)
                && ctx.pointer_hover_pos().is_some_and(|p| at.contains(p))
            {
                ctx.set_cursor_icon(CursorIcon::Grab);
            }
            let bar = Rect::from_center_size(
                pos2(rect.center().x, rect.bottom() - GAP / 2.0 - BAR / 2.0),
                vec2(4.0 * t.control + 120.0, BAR),
            );
            press = bar_ui(
                ui,
                bar,
                viewer.zoom.unwrap_or(fit),
                viewer.zoom.is_none(),
                t,
            )
            .or(press);
            match press {
                Some(Press::Close) => open = false,
                Some(Press::Fit) => {
                    viewer.zoom = None;
                    viewer.pan = Vec2::ZERO;
                }
                Some(Press::By(by)) => viewer.zoom_to(scale * by, (scale, fit), Vec2::ZERO),
                Some(Press::Actual) => viewer.zoom_to(1.0, (scale, fit), Vec2::ZERO),
                None => {}
            }
        });
    if !open {
        ctx.data_mut(|d| d.remove::<u64>(shown_id()));
    }
    open
}

#[derive(Clone, Copy)]
enum Press {
    Close,
    Fit,
    Actual,
    By(f32),
}

fn keys(ctx: &Context) -> Option<Press> {
    ctx.input_mut(|i| {
        let mut key = |k| i.consume_key(Modifiers::NONE, k);
        if key(Key::Escape) {
            Some(Press::Close)
        } else if key(Key::Plus) || key(Key::Equals) {
            Some(Press::By(STEP))
        } else if key(Key::Minus) {
            Some(Press::By(1.0 / STEP))
        } else if key(Key::Num0) {
            Some(Press::Fit)
        } else {
            None
        }
    })
}

/// The zoom buttons, the zoom, and Close, on a pill at the foot.
fn bar_ui(ui: &mut egui::Ui, bar: Rect, scale: f32, fitted: bool, t: &Tokens) -> Option<Press> {
    let mut press = None;
    ui.painter().rect_filled(bar, BAR / 2.0, t.surface);
    let inner = UiBuilder::new()
        .max_rect(bar.shrink2(vec2(10.0, 6.0)))
        .layout(Layout::left_to_right(Align::Center));
    ui.scope_builder(inner, |ui| {
        if widgets::icon_button(ui, Icon::ZoomOut, "Zoom out", false).clicked() {
            press = Some(Press::By(1.0 / STEP));
        }
        // The zoom, which goes back to one pixel a point when pressed.
        let zoom = egui::Button::new(format!("{:.0}%", scale * 100.0)).frame_when_inactive(false);
        let zoom = ui.add_sized([52.0, t.control], zoom);
        if zoom.on_hover_text("Actual size").clicked() {
            press = Some(Press::Actual);
        }
        if widgets::icon_button(ui, Icon::ZoomIn, "Zoom in", false).clicked() {
            press = Some(Press::By(STEP));
        }
        if widgets::icon_button(ui, Icon::FitPage, "Fit to window", fitted).clicked() {
            press = Some(Press::Fit);
        }
        ui.separator();
        if widgets::icon_button(ui, Icon::Close, "Close picture", false).clicked() {
            press = Some(Press::Close);
        }
    });
    press
}
