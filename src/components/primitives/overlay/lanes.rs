//! Overlay lanes: which floating surfaces may be up at the same time.
//!
//! Every overlay used to arbitrate its own exclusivity, and each did it
//! differently. Menus relied on a side effect of [`use_dismiss`]: a press on
//! another trigger is an outside press, so the open menu closes *and then* the
//! other one opens. That works menu-to-menu and nowhere else — a modal is not a
//! press target, and a toolbar trigger sitting "under" a modal's backdrop is
//! still clickable because the chrome row lives in a different stacking
//! context. Which is how the appearance menu and the settings modal ended up
//! open at once, each half-covered by the other.
//!
//! So exclusivity is state, not a side effect of hit-testing: one registry
//! ([`OverlayBoard`]) holds who occupies what, and a surface's open signal is
//! the only store of its visibility — the registry WRITES that signal to evict
//! a loser. Because arbitration hangs off the signal rather than off a button,
//! every path into the open state arbitrates the same way: the trigger, Escape,
//! an outside press, a backdrop click, and another component opening the
//! surface on the reader's behalf (the reader menu's "Settings…" item).
//!
//! Two rules keep this from becoming a second source of truth:
//!
//! * The registry never stores its own copy of "open". Each member contributes
//!   the `RwSignal<bool>` it already had, so there is exactly one bit per
//!   surface.
//! * Nothing here knows what an overlay looks like. Position, focus and
//!   dismissal stay in [`crate::components::primitives::floating`]; this module
//!   is only the "who yields to whom" table.
//!
//! # Adding a surface
//!
//! Menus and modals are covered already: `MenuPopover` registers its `open`
//! signal as [`OverlayPolicy::MENU`] and `SettingsModal` registers its own as
//! [`OverlayPolicy::MODAL`], so any new menu or modal is exclusive by
//! construction and cannot forget to be. A surface that wants different
//! collisions passes its own [`OverlayPolicy`] (`MenuPopover`'s `policy` prop),
//! and a brand-new KIND of surface adds one lane bit plus the policy that uses
//! it. Until then `Lanes` is exactly as large as the app's actual collisions.

use leptos::prelude::*;

/// A mutual-exclusion group. Two overlays collide when one of them declares
/// that it displaces a lane the other occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Lanes(u8);

impl Lanes {
    /// No lanes at all: a surface that neither holds nor clears anything.
    /// (`Default` can't be used for this — its impl isn't `const`.)
    pub const NONE: Self = Self(0);
    /// Anchored menus and popovers ([`MenuPopover`](crate::components::shell::titlebar::toolbar_popover::MenuPopover)-hosted).
    pub const POPOVER: Self = Self(1 << 0);
    /// Modal dialogs, of which the reader has exactly one today: settings.
    pub const MODAL: Self = Self(1 << 1);

    const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether any of `self`'s lanes is in `other`.
    const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

}

/// What one overlay participates in: the lane it holds while open, and the
/// lanes it takes away from everyone else when it opens.
///
/// Deliberately two fields rather than one symmetric "group" — the
/// relationships the app needs are not all symmetric. A menu and a modal each
/// clear the other's lane, so neither can be left stranded under the other; a
/// surface that occupies nothing and clears nothing (`Lanes::default()` on both)
/// coexists with everything, which is the opt-out `MenuPopover`'s `policy` prop
/// offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayPolicy {
    /// Lane held while this overlay is open.
    pub occupies: Lanes,
    /// Lanes cleared the moment this overlay opens.
    pub displaces: Lanes,
}

impl OverlayPolicy {
    /// An anchored menu: one menu at a time, and a menu replaces an open modal
    /// instead of stacking under (or behind) it.
    pub const MENU: Self = Self {
        occupies: Lanes::POPOVER,
        displaces: Lanes::POPOVER.union(Lanes::MODAL),
    };
    /// A modal dialog: it covers the window, so it closes every menu, and a
    /// second dialog replaces it.
    pub const MODAL: Self = Self {
        occupies: Lanes::MODAL,
        displaces: Lanes::POPOVER.union(Lanes::MODAL),
    };
    /// A popover that lives INSIDE a dialog (a settings dropdown, a colour
    /// picker): part of the conversation, not a rival for the window. It
    /// holds no lane and clears none, so the dialog it belongs to stays open
    /// while it is up — and it coexists with every other surface, because
    /// its dismissal is already handled by outside-press hit-testing.
    pub const IN_DIALOG: Self = Self {
        occupies: Lanes::NONE,
        displaces: Lanes::NONE,
    };
}

/// One registered overlay. `open` stays the only store of the surface's state:
/// the registry writes it to close the surface, the surface writes it to close
/// itself, and the effect in [`OverlayBoard::register`] settles who wins.
#[derive(Clone, Copy)]
struct Member {
    token: u32,
    occupies: Lanes,
    open: RwSignal<bool>,
}

/// The registry of overlays that participate in lane arbitration.
///
/// Provided once by the app root, so both pages — and anything portaled to
/// `<body>` — share one table. Without the context (a component mounted in
/// isolation, a host test) every surface still works, it just arbitrates
/// nothing, exactly as it did before this module existed.
#[derive(Clone, Copy)]
pub struct OverlayBoard {
    members: RwSignal<Vec<Member>>,
    next_token: RwSignal<u32>,
}

impl Default for OverlayBoard {
    fn default() -> Self {
        Self {
            members: RwSignal::new(Vec::new()),
            next_token: RwSignal::new(1),
        }
    }
}

impl OverlayBoard {
    /// Put `open` under `policy`, for as long as the caller's reactive owner
    /// lives: the registration is dropped in `on_cleanup`, so an overlay that
    /// unmounts (a route change, a `<Show>` branch) never leaves a member whose
    /// signal nobody reads.
    pub fn register(self, policy: OverlayPolicy, open: RwSignal<bool>) {
        let token = self.next_token.get_untracked();
        self.next_token.set(token.wrapping_add(1));
        self.members.update(|ms| {
            ms.push(Member {
                token,
                occupies: policy.occupies,
                open,
            })
        });

        // Arbitrate on the STATE, not on the trigger: any write that lands the
        // overlay as open clears its collision set. Effects are deferred, so a
        // cascade (menu closes the modal, the modal's own effect then finds
        // itself closed and clears nothing) settles in one batch.
        Effect::new(move |_| {
            if open.get() {
                self.dismiss(token, policy.displaces);
            }
        });

        on_cleanup(move || {
            // `try_update`: cleanup can run while the root is being torn down,
            // and a stale member is harmless either way — its signal goes with it.
            let _ = self.members.try_update(|ms| ms.retain(|m| m.token != token));
        });
    }

    /// Close every member (other than `token`) that occupies a lane in
    /// `lanes`. The table is read first and written afterwards, so no signal is
    /// written while `members` is borrowed.
    fn dismiss(self, token: u32, lanes: Lanes) {
        if lanes == Lanes::default() {
            return;
        }
        let victims: Vec<RwSignal<bool>> = self.members.with(|ms| {
            ms.iter()
                .filter(|m| m.token != token && lanes.intersects(m.occupies))
                .map(|m| m.open)
                .collect()
        });
        for victim in victims {
            // Writing `false` to a signal that is already `false` notifies no
            // subscriber, so there is nothing to guard here.
            victim.set(false);
        }
    }
}

/// Put the open state an overlay already owns under `policy`. The return is
/// nothing on purpose: there is no second state to drive — the component keeps
/// reading and writing `open`, and the registry reacts to those writes.
///
/// Called from a component body (where the reactive owner lives). Outside the
/// app root there is no board, and the surface degrades to "arbitrates
/// nothing", which is what it did before this module.
pub fn use_overlay_lane(open: RwSignal<bool>, policy: OverlayPolicy) {
    if let Some(board) = use_context::<OverlayBoard>() {
        board.register(policy, open);
    }
}

#[cfg(test)]
mod tests {
    use super::{Lanes, OverlayPolicy};

    #[test]
    fn a_menu_and_a_modal_clear_each_other() {
        // The bug this module exists for: the appearance menu and the settings
        // modal could both be up. Either opening must close the other.
        assert!(
            OverlayPolicy::MENU
                .displaces
                .intersects(OverlayPolicy::MODAL.occupies)
        );
        assert!(
            OverlayPolicy::MODAL
                .displaces
                .intersects(OverlayPolicy::MENU.occupies)
        );
    }

    #[test]
    fn one_lane_holds_one_surface_of_its_kind() {
        assert!(
            OverlayPolicy::MENU
                .displaces
                .intersects(OverlayPolicy::MENU.occupies)
        );
        assert!(
            OverlayPolicy::MODAL
                .displaces
                .intersects(OverlayPolicy::MODAL.occupies)
        );
    }

    #[test]
    fn the_opt_out_policy_collides_with_nothing() {
        // This is what a caller passes to `MenuPopover`'s `policy` prop when a
        // surface is genuinely a peer of the chrome rather than a competitor
        // — `OverlayPolicy::IN_DIALOG` in its named form. The literal spells
        // the same thing out, so the two can never drift apart.
        let coexist = OverlayPolicy {
            occupies: Lanes::default(),
            displaces: Lanes::default(),
        };
        assert_eq!(coexist, OverlayPolicy::IN_DIALOG);
        assert!(!coexist.occupies.intersects(OverlayPolicy::MENU.occupies));
        assert!(!coexist
            .displaces
            .intersects(OverlayPolicy::MODAL.occupies));
        assert!(!OverlayPolicy::MENU.displaces.intersects(coexist.occupies));
    }

    #[test]
    fn a_popover_inside_a_dialog_never_evicts_its_own_dialog() {
        // The settings modal's dropdowns and pickers: opening one must not
        // close the modal it lives in, and must not close anything else
        // either — dismissal there is outside-press hit-testing, not lanes.
        assert!(!OverlayPolicy::IN_DIALOG
            .displaces
            .intersects(OverlayPolicy::MODAL.occupies));
        assert!(!OverlayPolicy::IN_DIALOG
            .displaces
            .intersects(OverlayPolicy::MENU.occupies));
        assert!(!OverlayPolicy::MENU.displaces.intersects(OverlayPolicy::IN_DIALOG.occupies));
        assert!(!OverlayPolicy::MODAL.displaces.intersects(OverlayPolicy::IN_DIALOG.occupies));
    }

    #[test]
    fn the_lanes_are_disjoint_bits() {
        assert!(!Lanes::POPOVER.intersects(Lanes::MODAL));
        assert!(!Lanes::default().intersects(Lanes::POPOVER));
        assert_eq!(
            Lanes::POPOVER.union(Lanes::MODAL),
            Lanes::MODAL.union(Lanes::POPOVER)
        );
        assert!(Lanes::POPOVER.union(Lanes::MODAL).intersects(Lanes::MODAL));
    }
}
