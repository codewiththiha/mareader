//! Results set as pages: shrunk to fit, drawn off the frame.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use mareader_reflow::{Block, Flow, Fonts, MAX_SIDE, PAD, Pixels, Typography};
use mareader_session::Document;

/// The smallest share of the reader's size a result shrinks to.
const MIN_SHARE: f32 = 0.4;
/// Never smaller than this, in layout units, however it is shared.
const MIN_SIZE: f32 = 6.0;

/// What to set: the blocks, their type, and the room on the page.
pub struct Job {
    pub blocks: Arc<Vec<Block>>,
    pub typo: Typography,
    /// The column's width and height, in page units.
    pub room: (f32, f32),
    /// Pixels per page unit.
    pub scale: f32,
    /// The book, whose pictures the result shows.
    pub book: Option<Arc<Document>>,
    /// Pages the result may run over, each `room`, and the one drawn.
    pub pages: usize,
    pub part: usize,
    /// What a page past the result's end says instead.
    pub note: Option<Arc<Vec<Block>>>,
}

/// A job for a slot, keyed by what it sets.
pub type Ask = (u64, u64, Job);

/// What the worker sends back: one slot's set result.
pub struct Done {
    pub slot: u64,
    pub key: u64,
    pub set: Option<Set>,
}

/// A set result: its pixels, and where its pictures and emoji sit.
pub struct Set {
    pub pixels: Pixels,
    /// Its width and depth, in page units.
    pub size: (f32, f32),
    /// Left, top, width and depth as the flow gives them, `PAD` in.
    pub pictures: Vec<[f32; 4]>,
    pub emojis: Vec<[f32; 4]>,
    /// A PDF's: put over white for its filters.
    pub pdf: bool,
}

/// A thread that sets cards; none where threads can't start.
pub fn worker() -> Option<(Sender<Ask>, Receiver<Done>)> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    let (jobs, inbox) = channel::<Ask>();
    let (done, news) = channel();
    let spawned = std::thread::Builder::new()
        .name("page-ai-cards".into())
        .spawn(move || {
            while let Ok(first) = inbox.recv() {
                // Only each slot's newest job is worth setting.
                let mut queue = vec![first];
                queue.extend(inbox.try_iter());
                let mut newest: Vec<Ask> = Vec::new();
                for job in queue {
                    newest.retain(|j| j.0 != job.0);
                    newest.push(job);
                }
                for (slot, key, job) in newest {
                    let set = set(&job);
                    if done.send(Done { slot, key, set }).is_err() {
                        return;
                    }
                }
            }
        });
    spawned.ok().map(|_| (jobs, news))
}

/// `blocks` in `job`'s type at `size`; tall never cuts a page.
fn flow_of(job: &Job, blocks: &Arc<Vec<Block>>, size: f32, tall: bool) -> Flow {
    let typo = Typography {
        font_size: size,
        ..job.typo.clone()
    };
    let blocks = Arc::clone(blocks);
    Flow::sized(blocks, &typo, Fonts::system(), job.room, tall)
}

/// The largest size that fits the pages; past the least, the last is tall.
fn fit(job: &Job) -> (Flow, f32) {
    let base = job.typo.font_size.max(1.0);
    let least = (base * MIN_SHARE).max(MIN_SIZE.min(base));
    let pages = job.pages.max(1);
    let set = |size: f32| flow_of(job, &job.blocks, size, false);
    let whole = set(base);
    if whole.page_count() <= pages {
        return (whole, base);
    }
    let small = set(least);
    if small.page_count() > pages {
        return match pages {
            1 => (flow_of(job, &job.blocks, least, true), least),
            _ => (small, least),
        };
    }
    // Eight halvings land within a fifth of a unit at any reading size.
    let (mut lo, mut hi) = (least, base);
    for _ in 0..8 {
        let mid = (lo + hi) / 2.0;
        if set(mid).page_count() <= pages {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (set(lo), lo)
}

/// The flow to draw part `job.part` from, and its page in it.
fn part(job: &Job) -> Option<(Flow, usize)> {
    let (flow, size) = fit(job);
    let last = job.pages.max(1) - 1;
    let count = flow.page_count();
    if job.part >= count {
        let note = job.note.as_ref()?;
        return Some((flow_of(job, note, job.typo.font_size, false), 0));
    }
    if job.part < last || count <= job.pages {
        return Some((flow, job.part));
    }
    // Too long even at the least size: the last page takes the rest.
    let excerpts = flow.excerpts(last);
    let (first, from) = excerpts.first().map(|(b, r)| (*b, r.start))?;
    let rest = (job.blocks.iter().enumerate())
        .skip(first)
        .map(|(i, b)| match i == first {
            true => b.cut(from..usize::MAX),
            false => b.clone(),
        });
    let rest = Arc::new(rest.collect());
    Some((flow_of(job, &rest, size, true), 0))
}

pub fn set(job: &Job) -> Option<Set> {
    let (flow, page) = part(job)?;
    let flow = match job.book.as_deref() {
        Some(Document::Epub { book, .. }) => book.with_pictures(flow),
        _ => flow,
    };
    let width = job.room.0;
    let depth = flow.depth(page).max(1.0);
    let max = MAX_SIDE as f32;
    let scale = job
        .scale
        .min(max / width.max(1.0))
        .min(max / depth)
        .max(0.05);
    let px = |v: f32| ((v * scale).ceil() as u32).clamp(1, MAX_SIDE);
    let region = [px(PAD), px(PAD), px(width), px(depth)];
    let pixels = flow.render(page, scale, region)?;
    Some(Set {
        pixels,
        size: (width, depth),
        pictures: flow.pictures(page),
        emojis: flow.emojis(page),
        pdf: job.book.is_none(),
    })
}

#[cfg(test)]
#[path = "cards_tests.rs"]
mod tests;
