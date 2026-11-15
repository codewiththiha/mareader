//! One-frame generated resting glyphs and their bounded texture cache.

use egui::{Context, Id, Image, Rect, TextureHandle, TextureOptions, Ui, Vec2};
use mareader_animated_icons::{Pack, RestingGlyph};
use mareader_engine::lru::ByteLruCache;

use super::PackKey;

const CACHE_BYTES: usize = 4 * 1024 * 1024;
const CACHE_ENTRIES: usize = 512;

#[derive(Clone)]
struct CachedGlyph {
    texture: TextureHandle,
    side: u32,
    target_x: u32,
    target_y: u32,
    width: u32,
    height: u32,
}

impl CachedGlyph {
    fn image(&self, size: f32) -> Image<'static> {
        let width = self.width.max(1) as f32;
        let height = self.height.max(1) as f32;
        let uv = Rect::from_min_max(
            egui::pos2(
                -(self.target_x as f32) / width,
                -(self.target_y as f32) / height,
            ),
            egui::pos2(
                self.side.saturating_sub(self.target_x) as f32 / width,
                self.side.saturating_sub(self.target_y) as f32 / height,
            ),
        );
        Image::from_texture(egui::load::SizedTexture::new(
            self.texture.id(),
            Vec2::splat(size),
        ))
        .uv(uv)
        .fit_to_exact_size(Vec2::splat(size))
    }
}

#[derive(Clone)]
struct RestingStore {
    cache: ByteLruCache<PackKey, CachedGlyph>,
}

impl Default for RestingStore {
    fn default() -> Self {
        Self {
            cache: ByteLruCache::new(CACHE_BYTES, CACHE_ENTRIES),
        }
    }
}

pub(super) fn image(ui: &Ui, pack: &'static Pack, size: f32) -> Image<'static> {
    let ctx = ui.ctx();
    let side = side(size, ctx.pixels_per_point());
    let key = PackKey::new(pack, side as u16);
    let store_id = Id::new("resting-icon-glyphs");
    if let Some(glyph) = ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<RestingStore>(store_id)
            .cache
            .get(&key)
            .cloned()
    }) {
        return glyph.image(size);
    }
    let Some(glyph) = mareader_animated_icons::raster_resting(pack, side) else {
        return blank(ctx, size);
    };
    let bytes = glyph.byte_len();
    let cached = upload(ctx, pack.id(), glyph);
    let image = cached.image(size);
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<RestingStore>(store_id)
            .cache
            .insert(key, cached, bytes);
    });
    image
}

fn upload(ctx: &Context, pack_id: &str, glyph: RestingGlyph) -> CachedGlyph {
    let image = egui::ColorImage::from_rgba_premultiplied(
        [glyph.width as usize, glyph.height as usize],
        &glyph.pixels,
    );
    let texture = ctx.load_texture(
        format!("resting-icon-{pack_id}-{}", glyph.side),
        image,
        TextureOptions::LINEAR,
    );
    CachedGlyph {
        texture,
        side: glyph.side,
        target_x: glyph.target_x,
        target_y: glyph.target_y,
        width: glyph.width,
        height: glyph.height,
    }
}

pub(super) fn blank(ctx: &Context, size: f32) -> Image<'static> {
    let key = Id::new("mareader-resting-icon-blank");
    let texture = if let Some(texture) = ctx.data(|data| data.get_temp::<TextureHandle>(key)) {
        texture
    } else {
        let image = egui::ColorImage::new([1, 1], vec![egui::Color32::TRANSPARENT]);
        let texture =
            ctx.load_texture("mareader-resting-icon-blank", image, TextureOptions::LINEAR);
        ctx.data_mut(|data| data.insert_temp(key, texture.clone()));
        texture
    };
    Image::from_texture(egui::load::SizedTexture::new(
        texture.id(),
        Vec2::splat(size),
    ))
    .fit_to_exact_size(Vec2::splat(size))
}

fn side(size: f32, ppp: f32) -> u32 {
    ((size * ppp).round() as i32).clamp(8, mareader_animated_icons::MAX_SIDE as i32) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resting_sizes_are_device_pixel_buckets() {
        assert_eq!(side(16.0, 1.0), 16);
        assert_eq!(side(16.0, 1.25), 20);
        assert_eq!(side(16.0, 2.0), 32);
        assert_eq!(side(1.0, 1.0), 8);
        assert_eq!(side(200.0, 2.0), mareader_animated_icons::MAX_SIDE);
    }

    #[test]
    fn repeated_resting_glyphs_share_the_pack_and_size_entry() {
        let Some(pack) = mareader_animated_icons::find("sun") else {
            panic!("sun pack is missing");
        };
        let ctx = Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(100.0))),
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            let _first = image(ui, pack, 16.0);
            let _second = image(ui, pack, 16.0);
        });
        output.textures_delta.clear();
        let store_id = Id::new("resting-icon-glyphs");
        let store = ctx.data(|data| data.get_temp::<RestingStore>(store_id));
        assert_eq!(
            store.as_ref().map(|store| store.cache.entries.len()),
            Some(1)
        );
        assert!(store.is_some_and(|store| store.cache.bytes > 0));
    }
}
