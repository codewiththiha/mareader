//! The morphing surface component — a direct port of the Gloss reference's
//! `gloss-surface`, composed on the generic [`FloatingCard`] primitive so the
//! box morph mechanics, the no-reflow content wrapper and the drag-handle
//! slot are shared with any future floating card.
//!
//! One fixed `div` whose `left/top/width/height/border-radius` come from the
//! sprung box; the primitive owns that shell. This module keeps ONLY the
//! gloss policy: the surface's neutral card chrome (opaque surface fill,
//! standard floating elevation, progress-driven opacity/pointer-events), the
//! drag-handle contents, and the header + scroll column.
//!
//! The card's look is deliberately PLAIN: an opaque panel on the page's
//! colour tokens, the same elevation every menu in the app wears. It used to
//! cross-fade through accent-tinted fills with a coloured halo while it
//! morphed, which read as decoration rather than information; the morph's
//! shape change plus the opacity ramp is the whole story now.
//!
//! Dismiss is not a button on the card: Escape / outside-tap / origin-exit are
//! owned by the popover's window listeners.

use std::sync::Arc;

use ai_core::gloss::GlossBox;
use ai_core::settings::GlossDensity;
use leptos::{html, prelude::*};
use pdf_core::math::smoothstep;

use crate::components::ai::types::{AiError, AiPhase, GlossPhase, WordInfo};
use crate::components::primitives::floating::floating_card::FloatingCard;
use crate::components::primitives::floating::types::FloatBox;

use super::placement::CARD_WIDTH;
use super::word_info::WordInfoSections;

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
    /// The word's part of speech, printed beside it in the header the way a
    /// dictionary prints it.
    #[prop(into)]
    pos: Signal<String>,
    /// How much air the card's typography carries (Settings → Theme).
    #[prop(into)]
    density: Signal<GlossDensity>,
    /// Begin dragging the expanded card.
    on_drag_start: Callback<(f64, f64, GlossBox)>,
    /// Card body: word sections / error.
    children: Children,
) -> impl IntoView {
    // The primitive speaks FloatBox; the domain speaks GlossBox. Convert at
    // the seam — one place, one From impl, math shared in pdf_core.
    let box_f = Signal::derive(move || FloatBox::from(box_.get()));
    let expanded_f = Signal::derive(move || FloatBox::from(expanded.get()));

    // Gloss policy: the neutral card chrome plus the progress-driven
    // opacity/pointer-events. The fill and elevation are the same in every
    // phase — one card look — and the opacity is what makes the morph read:
    // fade IN as the shape leaves the stroke, fade OUT as it returns onto it.
    // The mark stroke underneath owns the fully-collapsed look, so the outro
    // reads as "card shrinks AND dissolves back into the highlight".
    let surface_style = Signal::derive(move || {
        let pr = progress.get();
        let opacity = smoothstep(pr, 0.05, 0.5);
        let pe = if phase.get() == GlossPhase::Expanded && pr > 0.4 {
            "auto"
        } else {
            "none"
        };
        format!(
            "box-shadow:var(--gloss-shadow-card);background:var(--color-surface);\
             opacity:{opacity};pointer-events:{pe};"
        )
    });

    let content_opacity = Signal::derive(move || {
        if phase.get() == GlossPhase::Processing {
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
            hide_scrollbar=true
        >
            <GlossBody word=word pos=pos density=density>
                {children()}
            </GlossBody>
        </FloatingCard>
    }
}

/// The card body's density-dependent class sets: outer padding, the header's
/// bottom margin, the word's type size and the separator's bottom margin.
/// One tuple per density, shared with the measure twin through the
/// component — the twin's height is only correct if it renders EXACTLY what
/// the card renders.
fn body_classes(density: GlossDensity) -> (&'static str, &'static str, &'static str, &'static str) {
    match density {
        GlossDensity::Compact => ("px-4 pb-3 pt-4", "mb-2", "text-base font-semibold leading-snug", "mb-3"),
        GlossDensity::Comfortable => ("px-5 pb-4 pt-6", "mb-4", "text-lg font-semibold leading-tight", "mb-4"),
    }
}

/// The card body: the dictionary header (word + part of speech on one
/// baseline), a hairline rule, and the content column. ONE definition shared
/// by the visible surface and the hidden measure twin in
/// [`gloss_ai_popover`](super::gloss_ai_popover), so the twin's measured
/// height can never drift from the real layout — that is what makes
/// `content_height` correct. Block-flow container: the flex-squeeze
/// protection lives on this root (`shrink-0`), so inner sections need none.
#[component]
pub(crate) fn GlossBody(
    /// The word being explained (header title).
    #[prop(into)]
    word: Signal<String>,
    /// The word's part of speech, printed beside the word (hidden while the
    /// model has not supplied one yet).
    #[prop(into)]
    pos: Signal<String>,
    /// The spacing preset for the whole body.
    #[prop(into)]
    density: Signal<GlossDensity>,
    /// Body content (word sections / shimmer / error row).
    children: Children,
) -> impl IntoView {
    let classes = Signal::derive(move || body_classes(density.get()));
    view! {
        <div class=move || format!("shrink-0 {}", classes.get().0)>
            <header class=move || classes.get().1.to_string()>
                <div class="flex min-w-0 items-baseline gap-1.5">
                    <h2
                        class=move || format!("{} min-w-0 text-balance text-ink", classes.get().2)
                    >
                        {move || word.get()}
                    </h2>
                    <Show when=move || !pos.get().is_empty()>
                        <span class="ai-pos shrink-0">{move || pos.get()}</span>
                    </Show>
                </div>
            </header>
            <div class=move || format!("h-px bg-line {}", classes.get().3)></div>
            <div>{children()}</div>
        </div>
    }
}

/// The invisible measurement twin: a pixel-exact replica of the surface's
/// scroll column (same width, same density classes, same header, separator),
/// so the measured height already includes chrome and wrap — that is what
/// makes `content_height` correct. Rendered off-screen for the lifetime of
/// the popover.
#[component]
pub fn GlossMeasureTwin(
    /// NodeRef of the twin; handed to the content-measure hook.
    node_ref: NodeRef<html::Div>,
    #[prop(into)] word: Signal<String>,
    #[prop(into)] word_info: Signal<Option<Arc<WordInfo>>>,
    #[prop(into)] density: Signal<GlossDensity>,
) -> impl IntoView {
    // The header's POS line is derived here exactly as the surface derives
    // it, so a streaming snapshot that fills the POS in mid-answer moves
    // both headers in the same frame.
    let pos = Signal::derive(move || word_info.get().map(|i| i.pos.clone()).unwrap_or_default());
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
            <GlossBody word=word pos=pos density=density>
                <WordInfoSections info=word_info density=density />
            </GlossBody>
        </div>
    }
}

/// The card body by data phase: the word sections once anything is there
/// (streaming or done), the friendly error — with a retry affordance when
/// the failure is retryable — on failure. Pure presentation of the content
/// signals; the lifecycle that produces them lives in the controller.
///
/// The phases are separate `<Show>`s rather than one reactive `match`, and
/// Streaming and Done share ONE mount on purpose: a `match` re-runs its whole
/// arm when the phase flips, and reading `word_info` at this level would
/// re-run it on every streamed snapshot too — either way the sections
/// remount, the entrance animation replays, and the card flickers. `Show`
/// keeps the sections mounted while their condition holds, so snapshots
/// patch the text in place.
#[component]
pub fn GlossSurfaceContent(
    #[prop(into)] phase: Signal<AiPhase>,
    #[prop(into)] word_info: Signal<Option<Arc<WordInfo>>>,
    #[prop(into)] density: Signal<GlossDensity>,
    #[prop(into)] error: Signal<Option<AiError>>,
    /// Retry the current mark after a retryable failure.
    retry: Callback<()>,
) -> impl IntoView {
    let has_info = Signal::derive(move || matches!(phase.get(), AiPhase::Streaming | AiPhase::Done));
    view! {
        <Show when=move || has_info.get()>
            <WordInfoSections info=word_info density=density />
        </Show>
        <Show when=move || phase.get() == AiPhase::Error>
            <GlossErrorCard error=error retry=retry />
        </Show>
    }
}

/// The friendly failure — with a retry affordance when the failure is
/// retryable. Reads the error signal fine-grained, so a late-arriving
/// failure text patches in rather than rebuilding the card.
#[component]
fn GlossErrorCard(
    #[prop(into)] error: Signal<Option<AiError>>,
    /// Retry the current mark after a retryable failure.
    retry: Callback<()>,
) -> impl IntoView {
    let message =
        Signal::derive(move || error.get().unwrap_or_else(AiError::unknown).friendly().into_owned());
    let retryable = Signal::derive(move || error.get().is_some_and(|e| e.retryable));
    view! {
        <div class="ai-text-reveal flex flex-col gap-2.5">
            <p class="text-sm leading-normal text-ink/80">{move || message.get()}</p>
            <Show when=move || retryable.get()>
                <button
                    type="button"
                    on:click=move |_| retry.run(())
                    class="self-start rounded-full border border-line bg-surface \
                           px-3.5 py-1 text-sm font-medium text-ink \
                           transition-[transform,background-color] duration-150 ease-out \
                           hover:bg-line active:scale-[0.96] \
                           focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                >
                    "Try again"
                </button>
            </Show>
        </div>
    }
}
