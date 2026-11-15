//! Our GPU work in egui's frame (ADR 0003): pages, the glass and grain.

mod glass;
mod grain;
mod page_pass;

use std::sync::Arc;

use eframe::egui_wgpu::{self, wgpu};
use mareader_session::Raster;

pub use glass::{Glass, Look, MAX_SLOTS, Reach};
pub use grain::{GrainCallback, GrainPass, crawl_offset};
pub use page_pass::{Anchor, GpuPage, PagePass, PagesCallback, Quad};

/// The renderer's device and queue, plus our pipelines for it.
#[derive(Clone, Debug)]
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pub pass: Arc<PagePass>,
    pub grain: Arc<GrainPass>,
}

impl Gpu {
    pub fn new(render: &egui_wgpu::RenderState) -> Self {
        let pass = PagePass::new(&render.device, &render.queue, render.target_format);
        Self {
            device: render.device.clone(),
            queue: render.queue.clone(),
            pass: Arc::new(pass),
            grain: Arc::new(GrainPass::new(&render.device, render.target_format)),
        }
    }

    /// A raster as a texture; none if its bytes don't fit its size.
    pub fn upload(&self, raster: &Raster) -> Option<Arc<GpuPage>> {
        let (w, h) = (raster.width, raster.height);
        let page = self
            .pass
            .texture(&self.device, &self.queue, w, h, &raster.pixels)?;
        Some(Arc::new(page))
    }
}
