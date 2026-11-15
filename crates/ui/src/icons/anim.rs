//! Trigger policy, lazy atlas caching, and ordered icon mesh painting.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui::{Color32, Context, Id, Image, Rect, Response, TextureHandle, TextureOptions, Ui, Vec2};
use mareader_animated_icons::{Atlas, AtlasCell, Pack};

use crate::motion;

use super::lru::{ByteLruCache, PackKey};
use super::{GRID, Icon, pack};

/// Canvas-to-glyph size ratio including authored headroom.
const BOX: f32 = mareader_animated_icons::SPAN / GRID;
/// Animation samples per second. Repaints land on these frame boundaries.
const FRAME_RATE: f32 = mareader_animated_icons::FRAME_RATE;
/// Hard bound for atlas RGBA pixels.
const CACHE_BYTES: usize = mareader_animated_icons::MAX_ATLAS_BYTES;
/// Tiny atlases are byte-cheap but still own texture-manager entries.
const CACHE_ENTRIES: usize = 512;
const ATLAS_STORE_ID: &str = "animated-icon-atlases";

#[derive(Clone)]
struct CachedFrame {
    atlas: CachedAtlas,
    frame: u16,
}

/// What an active run has ready to paint this pass.
enum CurrentFrame {
    Animated(CachedFrame),
    Pending,
    Resting,
}

/// What the icons of one region are asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// Display, not a control: it never moves.
    Still,
    /// A menu that closes when answered: move as it is reached, while it shows.
    Hover,
    /// A control that stays put: answer a press with the move.
    Click,
}

impl Act {
    /// Whether this region waits for entry or a press.
    fn fires(self, entering: bool, pressed: bool) -> bool {
        match self {
            Self::Still => false,
            Self::Hover => entering,
            Self::Click => pressed,
        }
    }
}

/// What a frame asks of a glyph's run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    /// Leave it as it is.
    None,
    /// Begin after the current run lands.
    Again,
    /// Begin now.
    Start,
}

/// The ask, from what the pointer did and whether something is already moving.
fn order(act: Act, entering: bool, pressed: bool, live: bool) -> Order {
    if !act.fires(entering, pressed) {
        return Order::None;
    }
    if live { Order::Again } else { Order::Start }
}

/// Treats a widget absent for several passes as left.
fn entering(hovered: bool, held: bool, gone: bool) -> bool {
    hovered && (!held || gone)
}

/// The announcements of the pass being built.
#[derive(Clone, Default)]
struct Scope {
    pass: u64,
    acts: Vec<Act>,
}

/// The pass-local announcement stack key.
fn scope() -> Id {
    Id::new("mareader-icon-anim")
}

/// Holds an announcement while its region is built.
#[must_use = "an announcement holds only while it is held"]
pub(crate) struct Announced {
    ctx: Context,
    depth: usize,
}

impl Drop for Announced {
    fn drop(&mut self) {
        self.ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<Scope>(scope())
                .acts
                .truncate(self.depth);
        });
    }
}

/// Announces an action to icons in the current region.
pub(crate) fn announce(ui: &Ui, act: Act) -> Announced {
    shout(ui, act, false)
}

/// Announces only when no enclosing region has spoken.
pub(crate) fn assume(ui: &Ui, act: Act) -> Announced {
    shout(ui, act, true)
}

fn shout(ui: &Ui, act: Act, only_if_silent: bool) -> Announced {
    let ctx = ui.ctx().clone();
    // Context data locks are not reentrant.
    let pass = pass(&ctx);
    let depth = ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<Scope>(scope());
        if s.pass != pass {
            s.pass = pass;
            s.acts.clear();
        }
        let depth = s.acts.len();
        if !only_if_silent || depth == 0 {
            s.acts.push(act);
        }
        depth
    });
    Announced { ctx, depth }
}

/// What the icons being built right now were told to do.
fn current(ctx: &Context) -> Act {
    let pass = pass(ctx);
    ctx.data(|d| {
        d.get_temp::<Scope>(scope())
            .filter(|s| s.pass == pass)
            .and_then(|s| s.acts.last().copied())
            .unwrap_or(Act::Still)
    })
}

/// The current immediate-mode pass stamp.
fn pass(ctx: &Context) -> u64 {
    ctx.cumulative_pass_nr()
}

/// One glyph's run, kept from frame to frame while it plays.
#[derive(Clone)]
struct Run {
    /// When the run began, in input seconds; negative while nothing plays.
    start: f64,
    /// Whether the pointer rested on its widget the last frame it was built.
    held: bool,
    /// That frame, so a widget that has gone is treated as having been left.
    seen: u64,
    /// A press that arrived mid-run asks to be played once the run lands.
    again: bool,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            start: -1.0,
            held: false,
            seen: 0,
            again: false,
        }
    }
}

impl Run {
    /// The active timeline second, excluding the landing instant.
    fn through(&self, now: f64, dur: f32) -> Option<f32> {
        if self.start < 0.0 {
            return None;
        }
        let t = (now - self.start) as f32;
        (t < dur).then_some(t.max(0.0))
    }
}

/// Reserves an active glyph's stock-image slot.
pub(crate) fn glyph(ui: &Ui, icon: Icon, size: f32, tag: &str) -> Image<'static> {
    if is_live(ui, icon, tag) {
        Image::from_texture(egui::load::SizedTexture::new(
            blank(ui.ctx()),
            Vec2::splat(size),
        ))
    } else {
        icon.image(ui, size)
    }
}

/// Drives and overlays a button glyph without changing layout.
pub(crate) fn play(ui: &Ui, resp: &Response, icon: Icon, tag: &str, size: f32) {
    let mark = Rect::from_center_size(resp.rect.center(), Vec2::splat(size));
    play_at(ui, resp, icon, tag, mark);
}

/// Plays a glyph in its laid-out box instead of the response centre.
pub(crate) fn play_at(ui: &Ui, resp: &Response, icon: Icon, tag: &str, mark: Rect) {
    match advance(ui, resp, icon, tag, mark.width()) {
        CurrentFrame::Animated(frame) => {
            paint_cached(ui, mark, ink_of(ui, resp), &frame);
        }
        CurrentFrame::Pending => icon.paint(ui, mark, ink_of(ui, resp)),
        CurrentFrame::Resting => {}
    }
}

fn frame_box(mark: Rect) -> Rect {
    mark.expand(mark.width() * (BOX - 1.0) * 0.5)
}

/// The foreground tint egui applies to the button's resting glyph.
fn ink_of(ui: &Ui, resp: &Response) -> Color32 {
    let w = &ui.visuals().widgets;
    if resp.is_pointer_button_down_on() {
        w.active.fg_stroke.color
    } else if resp.hovered() {
        w.hovered.fg_stroke.color
    } else {
        w.inactive.fg_stroke.color
    }
}

/// Transparent layout placeholder for an overlaid active glyph.
fn blank(ctx: &Context) -> egui::TextureId {
    let key = Id::new("mareader-icon-blank");
    if let Some(tex) = ctx.data(|d| d.get_temp::<TextureHandle>(key)) {
        return tex.id();
    }
    let image = egui::ColorImage::new([1, 1], vec![Color32::TRANSPARENT]);
    let tex = ctx.load_texture("mareader-icon-blank", image, TextureOptions::LINEAR);
    let id = tex.id();
    ctx.data_mut(|d| d.insert_temp(key, tex));
    id
}

/// Drives a glyph painted directly by its widget.
pub(crate) fn paint(ui: &Ui, rect: Rect, icon: Icon, color: Color32, tag: &str, resp: &Response) {
    match advance(ui, resp, icon, tag, rect.width()) {
        CurrentFrame::Animated(frame) => {
            paint_cached(ui, rect, color, &frame);
        }
        CurrentFrame::Pending | CurrentFrame::Resting => icon.paint(ui, rect, color),
    }
}

/// The current quantised frame while its atlas is ready.
fn advance(ui: &Ui, resp: &Response, icon: Icon, tag: &str, size: f32) -> CurrentFrame {
    let Some(pack) = pack::of(icon) else {
        return CurrentFrame::Resting;
    };
    let ctx = ui.ctx();
    let act = current(ctx);
    let icon_dur = motion::icon_secs(ctx, pack.duration());
    if icon_dur <= 0.0 {
        ctx.data_mut(|data| data.remove::<AtlasStore>(Id::new(ATLAS_STORE_ID)));
    }
    let dur = if act == Act::Still { 0.0 } else { icon_dur };
    let id = key(ui, icon, tag);
    if dur <= 0.0 {
        // Disabling motion must discard a partial run.
        if ctx.data(|d| d.get_temp::<Run>(id)).is_some() {
            ctx.data_mut(|d| d.remove::<Run>(id));
        }
        return CurrentFrame::Resting;
    }
    let now = ctx.input(|i| i.time);
    let (hovered, pressed) = (resp.hovered(), resp.clicked());
    let seen = pass(ctx);
    let mut run = ctx.data(|d| d.get_temp::<Run>(id)).unwrap_or_default();
    let live = run.through(now, dur).is_some();
    let ask = order(
        act,
        entering(hovered, run.held, seen > run.seen + 2),
        pressed,
        live,
    );
    match ask {
        Order::Start => {
            run.start = now;
            run.again = false;
        }
        Order::Again => run.again = true,
        Order::None => {}
    }
    run.held = hovered;
    run.seen = seen;
    let atlas_side = side(size * BOX, ctx.pixels_per_point());
    let t = match run.through(now, dur) {
        Some(t) => t,
        None => {
            // Landed. Either the queued press starts next or the glyph rests.
            if run.again {
                run.again = false;
                run.start = now;
                let out = cached_frame(ctx, pack, atlas_side, 0, &mut run, now);
                schedule(ctx, &run, dur, 0, now);
                ctx.data_mut(|d| d.insert_temp(id, run));
                return out;
            }
            run.start = -1.0;
            ctx.data_mut(|d| d.insert_temp(id, run));
            return CurrentFrame::Resting;
        }
    };
    let frame = frame_index(t, dur);
    let out = cached_frame(ctx, pack, atlas_side, frame, &mut run, now);
    schedule(ctx, &run, dur, frame, now);
    ctx.data_mut(|d| d.insert_temp(id, run));
    out
}

/// Converts the padded canvas to a bounded device-pixel side.
fn side(size: f32, ppp: f32) -> u32 {
    ((size * ppp).round() as i32).clamp(8, mareader_animated_icons::MAX_SIDE as i32) as u32
}

fn frame_index(t: f32, dur: f32) -> u16 {
    let last = (dur * FRAME_RATE).ceil().max(1.0) as u32 - 1;
    ((t.max(0.0) * FRAME_RATE).floor() as u32)
        .min(last)
        .min(u16::MAX as u32) as u16
}

fn schedule(ctx: &Context, run: &Run, dur: f32, frame: u16, now: f64) {
    let next = run.start + (f64::from(frame) + 1.0) / f64::from(FRAME_RATE);
    let end = run.start + f64::from(dur);
    let wait = next.min(end) - now;
    if wait.is_finite() && wait > 0.0 {
        ctx.request_repaint_after(Duration::from_secs_f64(wait));
    } else {
        ctx.request_repaint();
    }
}

/// Reports whether a button must reserve a transparent glyph.
fn is_live(ui: &Ui, icon: Icon, tag: &str) -> bool {
    let Some(pack) = pack::of(icon) else {
        return false;
    };
    let ctx = ui.ctx();
    if current(ctx) == Act::Still {
        return false;
    }
    let dur = motion::icon_secs(ctx, pack.duration());
    if dur <= 0.0 {
        return false;
    }
    let now = ctx.input(|i| i.time);
    ctx.data(|d| d.get_temp::<Run>(key(ui, icon, tag)))
        .is_some_and(|run| run.through(now, dur).is_some())
}

/// One glyph's, in one widget's, run.
fn key(ui: &Ui, icon: Icon, tag: &str) -> Id {
    ui.id().with(("icon-anim", icon.slot(), tag))
}

#[cfg(test)]
pub(crate) fn run_started(ui: &Ui, icon: Icon, tag: &str) -> bool {
    ui.ctx()
        .data(|data| data.get_temp::<Run>(key(ui, icon, tag)))
        .is_some_and(|run| run.start >= 0.0)
}

/// The current quantised frame, holding a cold run while its atlas is pending.
fn cached_frame(
    ctx: &Context,
    pack: &'static Pack,
    side: u32,
    frame: u16,
    run: &mut Run,
    now: f64,
) -> CurrentFrame {
    let (atlas, pending) = atlas(ctx, pack, side as u16);
    if pending {
        run.start = now;
    }
    atlas.map_or(CurrentFrame::Pending, |atlas| {
        CurrentFrame::Animated(CachedFrame { atlas, frame })
    })
}

fn paint_cached(ui: &Ui, mark: Rect, tint: Color32, frame: &CachedFrame) {
    let canvas = frame_box(mark);
    let side = frame.atlas.side.max(1) as f32;
    let scale = canvas.size() / side;
    let mut mesh = egui::Mesh::with_texture(frame.atlas.texture.id());
    for cell in frame.atlas.frame_cells(frame.frame) {
        let min = canvas.min
            + egui::vec2(
                cell.target_x as f32 * scale.x,
                cell.target_y as f32 * scale.y,
            );
        let rect = Rect::from_min_size(
            min,
            egui::vec2(cell.width as f32 * scale.x, cell.height as f32 * scale.y),
        );
        let uv = Rect::from_min_max(
            egui::pos2(
                cell.atlas_x as f32 / frame.atlas.width.max(1) as f32,
                cell.atlas_y as f32 / frame.atlas.height.max(1) as f32,
            ),
            egui::pos2(
                cell.atlas_x.saturating_add(cell.width) as f32 / frame.atlas.width.max(1) as f32,
                cell.atlas_y.saturating_add(cell.height) as f32 / frame.atlas.height.max(1) as f32,
            ),
        );
        // One mesh retains cell insertion order under one texture.
        mesh.add_rect_with_uv(rect, uv, tint);
    }
    if !mesh.indices.is_empty() {
        ui.painter().add(egui::Shape::mesh(mesh));
    }
}

#[derive(Clone)]
struct CachedAtlas {
    texture: TextureHandle,
    width: u32,
    height: u32,
    side: u32,
    cells: Arc<[AtlasCell]>,
    frames: u16,
    layers: u16,
}

impl CachedAtlas {
    fn frame_cells(&self, frame: u16) -> &[AtlasCell] {
        let frame = frame.min(self.frames.saturating_sub(1));
        let start = usize::from(frame).saturating_mul(usize::from(self.layers));
        let end = start.saturating_add(usize::from(self.layers));
        self.cells.get(start..end).unwrap_or_default()
    }
}

struct Completed {
    key: PackKey,
    pack_id: &'static str,
    atlas: Option<Atlas>,
    retry: bool,
}

#[derive(Clone, Default)]
struct Mailbox(Arc<Mutex<Vec<Completed>>>);

impl Mailbox {
    fn drain(&self) -> Vec<Completed> {
        self.0
            .lock()
            .map(|mut mailbox| std::mem::take(&mut *mailbox))
            .unwrap_or_default()
    }
}

struct Published {
    key: PackKey,
    atlas: Option<(CachedAtlas, usize)>,
    retry: bool,
}

fn upload(ctx: &Context, completed: Completed) -> Published {
    let Completed {
        key,
        pack_id,
        atlas,
        retry,
    } = completed;
    let atlas = atlas.map(|atlas| {
        let bytes = atlas.byte_len();
        let image = egui::ColorImage::from_rgba_premultiplied(
            [atlas.width as usize, atlas.height as usize],
            &atlas.pixels,
        );
        let texture = ctx.load_texture(
            format!("animated-atlas-{pack_id}-{}", key.side),
            image,
            TextureOptions::LINEAR,
        );
        let cached = CachedAtlas {
            texture,
            width: atlas.width,
            height: atlas.height,
            side: atlas.side,
            cells: atlas.cells.into(),
            frames: atlas.frames,
            layers: atlas.layers,
        };
        (cached, bytes)
    });
    Published { key, atlas, retry }
}

#[derive(Clone)]
struct AtlasStore {
    cache: ByteLruCache<PackKey, CachedAtlas>,
    pending: HashSet<PackKey>,
    failed: HashSet<PackKey>,
    mailbox: Mailbox,
    #[cfg(not(target_arch = "wasm32"))]
    sender: std::sync::mpsc::SyncSender<Job>,
}

impl Default for AtlasStore {
    fn default() -> Self {
        Self {
            cache: ByteLruCache::new(CACHE_BYTES, CACHE_ENTRIES),
            pending: HashSet::new(),
            failed: HashSet::new(),
            mailbox: Mailbox::default(),
            #[cfg(not(target_arch = "wasm32"))]
            sender: worker_sender(),
        }
    }
}

impl AtlasStore {
    fn publish(&mut self, completed: Vec<Published>) {
        for completed in completed {
            self.pending.remove(&completed.key);
            if let Some((atlas, bytes)) = completed.atlas {
                self.cache.insert(completed.key, atlas, bytes);
            } else if !completed.retry {
                self.failed.insert(completed.key);
            }
        }
    }
}

/// The cached atlas and whether its work is still pending.
fn atlas(ctx: &Context, pack: &'static Pack, side: u16) -> (Option<CachedAtlas>, bool) {
    let store_id = Id::new(ATLAS_STORE_ID);
    let mailbox = ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<AtlasStore>(store_id)
            .mailbox
            .clone()
    });
    let completed = mailbox
        .drain()
        .into_iter()
        .map(|completed| upload(ctx, completed))
        .collect();
    let key = PackKey::new(pack, side);
    ctx.data_mut(|data| {
        let store = data.get_temp_mut_or_default::<AtlasStore>(store_id);
        store.publish(completed);
        if let Some(atlas) = store.cache.get(&key) {
            return (Some(atlas.clone()), false);
        }
        if store.pending.contains(&key) {
            return (None, true);
        }
        if store.failed.contains(&key) {
            return (None, false);
        }
        if store.request(ctx, key, pack, side) {
            store.pending.insert(key);
            return (None, true);
        }
        (None, false)
    })
}

#[cfg(not(target_arch = "wasm32"))]
struct Job {
    key: PackKey,
    pack: &'static Pack,
    side: u16,
    mailbox: Mailbox,
    ctx: Context,
}

#[cfg(not(target_arch = "wasm32"))]
fn worker_sender() -> std::sync::mpsc::SyncSender<Job> {
    use std::sync::mpsc::sync_channel;

    let (sender, receiver) = sync_channel::<Job>(2);
    let _worker = std::thread::Builder::new()
        .name("animated-icon-atlas".to_owned())
        .spawn(move || {
            while let Ok(job) = receiver.recv() {
                // One unpublished atlas is retained per context.
                let retry = job.mailbox.0.lock().map_or(true, |mailbox| {
                    mailbox.iter().any(|done| done.atlas.is_some())
                });
                let atlas = if retry {
                    None
                } else {
                    mareader_animated_icons::raster_atlas(job.pack, u32::from(job.side))
                };
                if let Ok(mut mailbox) = job.mailbox.0.lock() {
                    mailbox.push(Completed {
                        key: job.key,
                        pack_id: job.pack.id(),
                        atlas,
                        retry,
                    });
                }
                job.ctx.request_repaint();
            }
        });
    sender
}

#[cfg(not(target_arch = "wasm32"))]
impl AtlasStore {
    fn request(&self, ctx: &Context, key: PackKey, pack: &'static Pack, side: u16) -> bool {
        use std::sync::mpsc::TrySendError;

        match self.sender.try_send(Job {
            key,
            pack,
            side,
            mailbox: self.mailbox.clone(),
            ctx: ctx.clone(),
        }) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl AtlasStore {
    fn request(&self, ctx: &Context, key: PackKey, pack: &'static Pack, side: u16) -> bool {
        let atlas = mareader_animated_icons::raster_atlas(pack, u32::from(side));
        if let Ok(mut mailbox) = self.mailbox.0.lock() {
            mailbox.push(Completed {
                key,
                pack_id: pack.id(),
                atlas,
                retry: false,
            });
            ctx.request_repaint();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_times_are_quantised_and_bounded_by_the_run() {
        assert_eq!(frame_index(0.0, 0.56), 0);
        assert_eq!(frame_index(1.0 / FRAME_RATE - 1.0e-4, 0.56), 0);
        assert_eq!(frame_index(1.0 / FRAME_RATE + 1.0e-4, 0.56), 1);
        assert_eq!(frame_index(0.559, 0.56), 33);
        assert_eq!(frame_index(8.0, 0.56), 33);
        assert!(
            mareader_animated_icons::all()
                .all(|pack| { frame_index(pack.duration(), pack.duration()) <= 50 })
        );
    }

    #[test]
    fn only_pending_atlases_hold_cold_runs_open() {
        let Some(pack) = mareader_animated_icons::find("sun") else {
            panic!("sun pack is missing");
        };
        let ctx = Context::default();
        let key = PackKey::new(pack, 41);
        ctx.data_mut(|data| {
            data.get_temp_mut_or_default::<AtlasStore>(Id::new(ATLAS_STORE_ID))
                .pending
                .insert(key);
        });
        let mut run = Run {
            start: 1.0,
            ..Run::default()
        };
        let pending = cached_frame(&ctx, pack, 41, 0, &mut run, 20.0);
        assert!(matches!(pending, CurrentFrame::Pending));
        assert_eq!(run.start, 20.0);

        ctx.data_mut(|data| {
            let store = data.get_temp_mut_or_default::<AtlasStore>(Id::new(ATLAS_STORE_ID));
            store.pending.remove(&key);
            store.failed.insert(key);
        });
        run.start = 1.0;
        let failed = cached_frame(&ctx, pack, 41, 0, &mut run, 20.0);
        assert!(matches!(failed, CurrentFrame::Pending));
        assert_eq!(run.start, 1.0);
    }

    #[test]
    fn each_act_waits_for_its_own_event() {
        assert!(!Act::Still.fires(true, true));
        assert!(Act::Hover.fires(true, false));
        assert!(!Act::Hover.fires(false, true));
        assert!(Act::Click.fires(false, true));
        assert!(!Act::Click.fires(true, false));
    }

    #[test]
    fn a_live_run_queues_instead_of_snapping() {
        assert_eq!(order(Act::Click, false, true, false), Order::Start);
        assert_eq!(order(Act::Click, false, true, true), Order::Again);
        assert_eq!(order(Act::Hover, false, true, false), Order::None);
    }

    #[test]
    fn every_generated_pack_is_reachable() {
        assert_eq!(mareader_animated_icons::all().len(), 104);
        for pack in mareader_animated_icons::all() {
            assert!(!pack.id().is_empty());
            assert!(pack.frame_count() > 0);
        }
    }

    #[test]
    fn the_nearest_announcement_overrides_its_menu() {
        let ctx = Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(100.0))),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            assert_eq!(current(ui.ctx()), Act::Still);
            let menu = announce(ui, Act::Hover);
            assert_eq!(current(ui.ctx()), Act::Hover);
            {
                let control = announce(ui, Act::Click);
                assert_eq!(current(ui.ctx()), Act::Click);
                drop(control);
            }
            assert_eq!(current(ui.ctx()), Act::Hover);
            drop(menu);
            assert_eq!(current(ui.ctx()), Act::Still);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn a_move_starts_on_a_reaching_not_a_resting() {
        assert!(entering(true, false, false), "reached");
        assert!(!entering(true, true, false), "already resting on it");
        assert!(entering(true, true, true), "back after its menu closed");
        assert!(!entering(false, false, false));
        assert_eq!(order(Act::Hover, true, false, false), Order::Start);
        assert_eq!(order(Act::Click, false, true, true), Order::Again);
        assert_eq!(order(Act::Hover, false, false, true), Order::None);
    }

    fn click_run_liveness(icon_animations: bool) -> [bool; 4] {
        use egui::{Event, Modifiers, PointerButton, Sense, pos2};

        let Some(pack) = mareader_animated_icons::find("sun") else {
            panic!("sun pack is missing");
        };
        let ctx = Context::default();
        let settings = mareader_settings::MotionSettings {
            icon_animations,
            ..Default::default()
        };
        motion::set(&ctx, &settings, false);
        let cell = Rect::from_min_size(pos2(4.0, 4.0), Vec2::splat(18.0));
        let atlas_key = PackKey::new(pack, side(cell.width() * BOX, 1.0) as u16);
        ctx.data_mut(|data| {
            data.get_temp_mut_or_default::<AtlasStore>(Id::new(ATLAS_STORE_ID))
                .failed
                .insert(atlas_key);
        });
        let mut run_of = Id::new("never");
        let mut live = [false; 4];
        for (index, time) in [
            0.0f64,
            0.02,
            f64::from(pack.duration()) * 0.5,
            f64::from(pack.duration()) * 2.0,
        ]
        .into_iter()
        .enumerate()
        {
            let at = cell.center();
            let events = if index == 1 {
                vec![
                    Event::PointerMoved(at),
                    Event::PointerButton {
                        pos: at,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::default(),
                    },
                    Event::PointerButton {
                        pos: at,
                        button: PointerButton::Primary,
                        pressed: false,
                        modifiers: Modifiers::default(),
                    },
                ]
            } else {
                vec![Event::PointerMoved(pos2(390.0, 290.0))]
            };
            let raw = egui::RawInput {
                time: Some(time),
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), Vec2::splat(400.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                let response = ui.interact(cell, ui.id().with("cell"), Sense::click());
                let _act = announce(ui, Act::Click);
                if index == 1 {
                    run_of = key(ui, Icon::Light, "cell");
                }
                paint(ui, cell, Icon::Light, Color32::WHITE, "cell", &response);
            });
            out.textures_delta.clear();
            live[index] = ctx
                .data(|data| data.get_temp::<Run>(run_of))
                .is_some_and(|run| run.start >= 0.0);
        }
        live
    }

    #[test]
    fn a_run_outlives_the_press_that_started_it() {
        assert_eq!(click_run_liveness(true), [false, true, true, false]);
    }

    #[test]
    fn the_icon_switch_prevents_a_run() {
        assert_eq!(click_run_liveness(false), [false; 4]);
    }

    #[test]
    fn switching_icons_off_releases_the_atlas_store() {
        let ctx = Context::default();
        ctx.data_mut(|data| {
            data.insert_temp(Id::new(ATLAS_STORE_ID), AtlasStore::default());
        });
        let settings = mareader_settings::MotionSettings {
            icon_animations: false,
            ..Default::default()
        };
        motion::set(&ctx, &settings, false);
        let cell = Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(18.0));
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(100.0))),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            let response = ui.interact(cell, ui.id().with("cell"), egui::Sense::click());
            let _act = announce(ui, Act::Click);
            let _ = advance(ui, &response, Icon::Light, "cell", cell.width());
        });
        out.textures_delta.clear();
        assert!(
            ctx.data(|data| data.get_temp::<AtlasStore>(Id::new(ATLAS_STORE_ID)))
                .is_none()
        );
    }
}
