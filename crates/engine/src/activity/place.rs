//! Where the stacks go: one at the workspace's corner, or one a pane.
//!
//! While only the app's own work shows, one stack sits at the workspace's
//! corner. Once a pane's job needs the corner, every pane shows the app's
//! work in its own corner, and the owning pane its job in front of it.

use mareader_workspace::PaneId;

use super::{Key, Scope};

/// Where a stack stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    Workspace,
    Pane(PaneId),
}

/// The stacks for `items`, those whose own visual is out of view, oldest
/// first; `panes` are the panes shown. Each stack lists its keys oldest
/// first, a pane's own jobs after the app's, so they sit in front.
pub fn place(panes: &[PaneId], items: &[(Key, Scope)]) -> Vec<(Spot, Vec<Key>)> {
    let global: Vec<Key> = items
        .iter()
        .filter(|(_, scope)| *scope == Scope::Global)
        .map(|(key, _)| *key)
        .collect();
    let own = |pane: PaneId| {
        let mine = items.iter().filter(move |(_, s)| *s == Scope::Pane(pane));
        mine.map(|(key, _)| *key)
    };
    let any_pane = panes.iter().any(|&pane| own(pane).next().is_some());
    if !any_pane {
        return match global.is_empty() {
            true => Vec::new(),
            false => vec![(Spot::Workspace, global)],
        };
    }
    let stack = |&pane: &PaneId| {
        let keys: Vec<Key> = global.iter().copied().chain(own(pane)).collect();
        (Spot::Pane(pane), keys)
    };
    panes
        .iter()
        .map(stack)
        .filter(|(_, keys)| !keys.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: PaneId = PaneId(1);
    const B: PaneId = PaneId(2);
    const ORGANIZE: Key = Key::Organize { run: 1 };
    const EXPLAIN: Key = Key::Cards { pane: A, from: 4 };

    fn organize() -> (Key, Scope) {
        (ORGANIZE, Scope::Global)
    }

    /// The owner's worked example, PROGRESS.md section 5, steps 1 to 5.
    #[test]
    fn the_app_s_work_stacks_once_until_a_pane_needs_its_corner() {
        let one = place(&[A], &[organize()]);
        assert_eq!(one, [(Spot::Workspace, vec![ORGANIZE])], "1: one pane");
        let two = place(&[A, B], &[organize()]);
        assert_eq!(two, [(Spot::Workspace, vec![ORGANIZE])], "2: two panes");
        // 3: the explain's glow is in view, so it isn't among the items.
        let glowing = place(&[A, B], &[organize()]);
        assert_eq!(glowing, two, "3: its own visual says it");
        let away = place(&[A, B], &[organize(), (EXPLAIN, Scope::Pane(A))]);
        let split = vec![
            (Spot::Pane(A), vec![ORGANIZE, EXPLAIN]),
            (Spot::Pane(B), vec![ORGANIZE]),
        ];
        assert_eq!(away, split, "4: A stacks both, B the app's");
        let back = place(&[A, B], &[organize()]);
        assert_eq!(back, two, "5: back to the one stack");
    }

    #[test]
    fn a_pane_s_job_alone_stacks_in_its_pane_and_nothing_shows_nothing() {
        let alone = place(&[A, B], &[(EXPLAIN, Scope::Pane(A))]);
        assert_eq!(alone, [(Spot::Pane(A), vec![EXPLAIN])]);
        assert!(place(&[A, B], &[]).is_empty());
        let hidden = place(&[B], &[(EXPLAIN, Scope::Pane(A))]);
        assert!(hidden.is_empty(), "a pane not shown has no corner");
    }
}
