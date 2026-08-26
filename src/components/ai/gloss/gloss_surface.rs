//! The morphing surface component — a direct port of the Gloss reference's
//! `gloss-surface`, composed on the generic [`FloatingCard`] primitive so the
//! box morph mechanics, the no-reflow content wrapper and the drag-handle
//! slot are shared with any future floating card.
//!
//! One fixed `div` whose `left/top/width/height/border-radius` come from the
//! sprung box; the primitive owns that shell. This module keeps ONLY the
//! gloss policy: phase fills/shadows/opacity (the `GlossBox` → `FloatBox`
//! conversions happen here, at the primitive boundary), the drag-handle
//! contents, and the header + scroll column.
//!
//! Dismiss is not a button on the card: Escape / outside-tap / origin-exit are
//! owned by the popover's window listeners.

use leptos::{html, prelude::*};

use pdf_core::gloss::GlossBox;
use pdf_core::math::smoothstep;

use crate::components::ai::types::{AiError, AiPhase, GlossPhase, WordInfo};
use crate::components::ai::word_info::WordInfoSections;
use crate::components::primitives::feedback::shimmer::LoadingShimmer;
use crate::components::primitives::floating::floating_card::FloatingCard;
use crate::components::primitives::floating::types::FloatBox;

use super::placement::CARD_WIDTH;

/// Surface background by phase.
///
/// Processing and compact are deliberately SURFACE-heavy rather than a bare
/// accent tint: both sit directly on top of page text. Expanded cross-fades
/// from that fill to the opaque card as the morph completes — and the same
/// mix run backwards is what makes the outro read as the card dissolving into
/// the stroke it is landing on.
fn fill_for(phase: GlossPhase, progress: f64) -> String {
    match phase {
        GlossPhase::Processing => concat!(
            "color-mix(in oklab, var(--color-surface) 78%, ",
            "color-mix(in oklab, var(--color-accent) 34%, transparent))",
        )
        .into(),
        GlossPhase::Compact => concat!(
            "color-mix(in oklab, var(--color-surface) 70%, ",
            "color-mix(in oklab, var(--color-accent) 30%, transparent))",
        )
        .into(),
        GlossPhase::Expanded => {
            let p = (progress.clamp(0.0, 1.0) * 100.0).round() as u32;
            format!(
                "color-mix(in oklab, var(--color-surface) {p}%, \
                 color-mix(in oklab, var(--color-accent) 36%, transparent))"
            )
        }
    }
}

/// Surface shadow by phase — an accent halo while processing/thin, the card
/// elevation once expanded.
fn shadow_for(phase: GlossPhase) -> String {
    match phase {
        GlossPhase::Processing => concat!(
            "0 0 0 1px color-mix(in oklab, var(--color-accent) 50%, transparent),",
            " 0 0 20px 5px color-mix(in oklab, var(--color-accent) 38%, transparent)"
        )
        .into(),
        GlossPhase::Compact => {
            "0 0 0 1px color-mix(in oklab, var(--color-accent) 22%, transparent)".into()
        }
        GlossPhase::Expanded => "var(--gloss-shadow-card)".into(),
    }
}

#[component]
pub fn GlossSurface(
    /// The card's geometry phase (independent of the AI data phase).
    phase: Signal<GlossPhase>,
    /// The current sprung box — written per-frame by the spring.
    box_: Signal<GlossBox>,
    /// The expanded target — the content wrapper is sized to THIS, not to the
    /// current box, so the text never reflows as the box morphs.
    expanded: Signal<GlossBox>,
    /// 0..1 morph progress; drives the content fade-in.
    progress: Signal<f64>,
    /// The selected word, for the card header.
    word: Signal<String>,
    /// Begin dragging the expanded card.
    on_drag_start: Callback<(f64, f64, GlossBox)>,
    /// Card body: shimmer / word sections / error.
    children: Children,
) -> impl IntoView {
    // The primitive speaks FloatBox; the domain speaks GlossBox. Convert at
    // the seam — one place, one From impl, math shared in pdf_core.
    let box_f = Signal::derive(move || FloatBox::from(box_.get()));
    let expanded_f = Signal::derive(move || FloatBox::from(expanded.get()));

    // Gloss policy: fills, shadows, surface opacity/pointer-events. This is
    // the `surface_style` extra the primitive appends after the geometry.
    let surface_style = Signal::derive(move || {
        let p = phase.get();
        let pr = progress.get();
        // Fade IN as the morph leaves the stroke, fade OUT as it returns onto
        // it. The mark stroke underneath owns the fully-collapsed look, so
        // the outro reads as "card shrinks AND dissolves back into the
        // highlight".
        let opacity = smoothstep(pr, 0.05, 0.5);
        let pe = if p == GlossPhase::Expanded && pr > 0.4 {
            "auto"
        } else {
            "none"
        };
        format!(
            "box-shadow:{};background:{};opacity:{};pointer-events:{};",
            shadow_for(p),
            fill_for(p, pr),
            opacity,
            pe
        )
    });

    let content_opacity = Signal::derive(move || {        if phase.get() == GlossPhase::Processing {
            0.0
        } else {
            smoothstep(progress.get(), 0.18, 0.7)
        }
    });
    let content_interactive = Signal::derive(move || progress.get() > 0.55);

    let phase_str = Signal::derive(move || match phase.get() {
        GlossPhase::Processing => "processing",
        GlossPhase::Expanded => "expanded",
        GlossPhase::Compact => "compact",
    });
    let role = Signal::derive(move || match phase.get() {
        GlossPhase::Expanded => "dialog",
        _ => "",
    });
    let aria_label = Signal::derive(move || match phase.get() {
        GlossPhase::Expanded => format!("Gloss for {}", word.get()),
        _ => String::new(),
    });

    let word_h = word.clone();
    let drag_handle: leptos::children::Children = Box::new(move || {
        // Drag handle — only live when expanded (guarded in the callback).
        view! {
            <div
                class="absolute left-0 right-0 top-2.5 z-10 flex cursor-grab \
                       justify-center active:cursor-grabbing"
                style:opacity=move || format!("{}", smoothstep(progress.get(), 0.5, 0.95))
                on:pointerdown=move |ev| {
                    if phase.get_untracked() != GlossPhase::Expanded {
                        return;
                    }
                    ev.prevent_default();
                    ev.stop_propagation();
                    on_drag_start.run((ev.client_x() as f64, ev.client_y() as f64, box_.get_untracked()));
                }
            >
                <span class="block h-1 w-8 rounded-full bg-ink/15"></span>
            </div>
        }
        .into_any()
    });

    view! {
        <FloatingCard
            box_=box_f
            expanded=expanded_f
            surface_style=surface_style
            content_opacity=content_opacity
            content_interactive=content_interactive
            drag_handle=drag_handle
            data_phase=phase_str
            role=role
            aria_label=aria_label
            class="gloss-surface"
        >
            <GlossBody word=word_h>
                {children()}
            </GlossBody>
        </FloatingCard>
    }
}

/// The card body: padded header + separator + content column. ONE definition
/// shared by the visible surface and the hidden measure twin in
/// [`gloss_ai_popover`](super::gloss_ai_popover), so the twin's measured
/// height can never drift from the real layout — that is what makes
/// `content_height` correct. Block-flow container: the flex-squeeze
/// protection lives on this root (`shrink-0`), so inner sections need none.
#[component]
pub(crate) fn GlossBody(
    /// The word being explained (header title).
    #[prop(into)]
    word: Signal<String>,
    /// Body content (word sections / shimmer / error row).
    children: Children,
) -> impl IntoView {
    view! {
        <div class="shrink-0 px-5 pb-4 pt-6">
            <header class="mb-4">
                <h2 class="text-lg font-semibold leading-tight text-balance text-ink">
                    {move || word.get()}
                </h2>
            </header>
            <div class="mb-4 h-px bg-line"></div>
            <div>{children()}</div>
        </div>
    }
}

/// The invisible measurement twin: a pixel-exact replica of the surface's
/// scroll column (same width, `px-5/pt-6/pb-4`, header, separator), so the
/// measured height already includes chrome and wrap — that is what makes
/// `content_height` correct. Rendered off-screen for the lifetime of the
/// popover.
#[component]
pub fn GlossMeasureTwin(
    /// NodeRef of the twin; handed to the content-measure hook.
    node_ref: NodeRef<html::Div>,
    #[prop(into)] word: Signal<String>,
    #[prop(into)] word_info: Signal<Option<WordInfo>>,
) -> impl IntoView {
    view! {
        <div
            node_ref=node_ref
            class=format!(
                "pointer-events-none invisible fixed left-0 top-0 {}",
                crate::components::primitives::floating::types::z::CONTENT
            )
            style=format!("width:{CARD_WIDTH}px")
            aria-hidden="true"
        >
            <GlossBody word=word>
                {move || word_info.get().map(|info| view! { <WordInfoSections info=info /> })}
            </GlossBody>
        </div>
    }
}

/// The card body by data phase: shimmer while waiting, the word sections
/// once anything is there (streaming or done), and the friendly error —
/// with a retry affordance when the failure is retryable — on failure.
/// Pure presentation of the content signals; the lifecycle that produces
/// them lives in the controller.
#[component]
pub fn GlossSurfaceContent(
    #[prop(into)] phase: Signal<AiPhase>,
    #[prop(into)] word_info: Signal<Option<WordInfo>>,
    #[prop(into)] error: Signal<Option<AiError>>,
    /// Retry the current mark after a retryable failure.
    retry: Callback<()>,
) -> impl IntoView {
    view! {
        {move || match phase.get() {
            AiPhase::Processing => view! { <LoadingShimmer /> }.into_any(),
            AiPhase::Streaming | AiPhase::Done => match word_info.get() {
                Some(info) => view! { <WordInfoSections info=info /> }.into_any(),
                None => view! { <LoadingShimmer /> }.into_any(),
            },
            AiPhase::Error => {
                let err = error.get().unwrap_or_else(AiError::unknown);
                let msg = err.friendly().into_owned();
                let retryable = err.retryable;
                view! {
                    <div class="ai-text-reveal flex flex-col gap-3 p-1">
                        <p class="text-sm leading-relaxed text-ink/80">{msg}</p>
                        <Show when=move || retryable>
                            <button
                                type="button"
                                on:click=move |_| retry.run(())
                                class="self-start rounded-full border border-line bg-surface \
                                       px-4 py-1.5 text-sm font-medium text-ink \
                                       transition-[transform,background-color] duration-150 ease-out \
                                       hover:bg-line active:scale-[0.96] \
                                       focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                            >
                                "Try again"
                            </button>
                        </Show>
                    </div>
                }
                .into_any()
            }
            AiPhase::Idle => ().into_any(),
        }}
    }
}
