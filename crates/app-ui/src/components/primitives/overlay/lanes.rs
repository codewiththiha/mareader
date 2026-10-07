//! Overlay lanes: which floating surfaces may be up at once.

use leptos::prelude::*;

/// A mutual-exclusion group: two overlays collide over a lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Lanes(u8);

impl Lanes {
    /// No lanes: a surface that holds and clears nothing.
    const NONE: Self = Self(0);
    /// Anchored menus and popovers.
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

/// What one overlay participates in: the lane it holds, the lanes it clears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayPolicy {
    /// Lane held while this overlay is open.
    pub occupies: Lanes,
    /// Lanes cleared the moment this overlay opens.
    pub displaces: Lanes,
}

impl OverlayPolicy {
    /// An anchored menu: one at a time, and it replaces an open modal.
    pub const MENU: Self = Self {
        occupies: Lanes::POPOVER,
        displaces: Lanes::POPOVER.union(Lanes::MODAL),
    };
    /// A modal: covers the window, so it closes every menu.
    pub const MODAL: Self = Self {
        occupies: Lanes::MODAL,
        displaces: Lanes::POPOVER.union(Lanes::MODAL),
    };
    /// A popover inside a dialog: holds no lane, clears none.
    pub const IN_DIALOG: Self = Self {
        occupies: Lanes::NONE,
        displaces: Lanes::NONE,
    };
}

/// One registered overlay; `open` is the only store of its state.
#[derive(Clone, Copy)]
struct Member {
    token: u32,
    occupies: Lanes,
    open: RwSignal<bool>,
}

/// The registry of overlays taking part in lane arbitration.
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
    /// Put `open` under `policy` for the caller's reactive owner's lifetime.
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

        // Arbitrate on the STATE: any write landing open clears the set.
        Effect::new(move |_| {
            if open.get() {
                self.dismiss(token, policy.displaces);
            }
        });

        on_cleanup(move || {
            // `try_update`: cleanup may run mid-teardown.
            let _ = self
                .members
                .try_update(|ms| ms.retain(|m| m.token != token));
        });
    }

    /// Close every member that occupies a lane in `lanes`, but `token`.
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
            // Writing `false` to a false signal notifies no subscriber.
            victim.set(false);
        }
    }
}

/// Put the open state an overlay already owns under `policy`.
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
        // The bug this module exists for: menu and modal open at once.
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
        // What `MenuPopover`'s `policy` prop takes for a peer surface.
        let coexist = OverlayPolicy {
            occupies: Lanes::default(),
            displaces: Lanes::default(),
        };
        assert_eq!(coexist, OverlayPolicy::IN_DIALOG);
        assert!(!coexist.occupies.intersects(OverlayPolicy::MENU.occupies));
        assert!(!coexist.displaces.intersects(OverlayPolicy::MODAL.occupies));
        assert!(!OverlayPolicy::MENU.displaces.intersects(coexist.occupies));
    }

    #[test]
    fn a_popover_inside_a_dialog_never_evicts_its_own_dialog() {
        // The modal's own dropdowns: they close nothing and hold no lane.
        assert!(
            !OverlayPolicy::IN_DIALOG
                .displaces
                .intersects(OverlayPolicy::MODAL.occupies)
        );
        assert!(
            !OverlayPolicy::IN_DIALOG
                .displaces
                .intersects(OverlayPolicy::MENU.occupies)
        );
        assert!(
            !OverlayPolicy::MENU
                .displaces
                .intersects(OverlayPolicy::IN_DIALOG.occupies)
        );
        assert!(
            !OverlayPolicy::MODAL
                .displaces
                .intersects(OverlayPolicy::IN_DIALOG.occupies)
        );
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
