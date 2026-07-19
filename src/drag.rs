//! Platform-neutral drag policy shared by the GTK UI and library tests.
//!
//! Backends report drag completion through several values. Keeping their
//! interpretation here prevents individual signal handlers from accidentally
//! treating a cancelled drag as accepted or overlooking GTK's explicit
//! `delete_data` result for a move.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DropEffect {
    None,
    Copy,
    Move,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DragCompletion {
    cancelled: bool,
    effect: DropEffect,
}

impl DragCompletion {
    pub const fn from_backend(
        cancelled: bool,
        selected_effect: DropEffect,
        delete_data: bool,
    ) -> Self {
        let effect = if delete_data {
            DropEffect::Move
        } else {
            selected_effect
        };
        Self { cancelled, effect }
    }

    pub const fn accepted(self) -> bool {
        !self.cancelled && !matches!(self.effect, DropEffect::None)
    }

    pub const fn effect(self) -> DropEffect {
        self.effect
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DragOffer {
    CopyOnly,
    CopyAndMove,
}

impl DragOffer {
    pub fn for_items(pinned: impl IntoIterator<Item = bool>) -> Self {
        let mut pinned = pinned.into_iter();
        let Some(first) = pinned.next() else {
            return Self::CopyOnly;
        };
        if first || pinned.any(|pinned| pinned) {
            Self::CopyOnly
        } else {
            Self::CopyAndMove
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_copy_and_move_results_are_distinguished() {
        let copied = DragCompletion::from_backend(false, DropEffect::Copy, false);
        assert!(copied.accepted());
        assert_eq!(copied.effect(), DropEffect::Copy);

        let moved = DragCompletion::from_backend(false, DropEffect::Move, false);
        assert!(moved.accepted());
        assert_eq!(moved.effect(), DropEffect::Move);
    }

    #[test]
    fn gtk_delete_data_is_an_accepted_move_even_without_a_selected_action() {
        let completion = DragCompletion::from_backend(false, DropEffect::None, true);

        assert!(completion.accepted());
        assert_eq!(completion.effect(), DropEffect::Move);
    }

    #[test]
    fn cancellation_wins_over_stale_backend_actions() {
        let completion = DragCompletion::from_backend(true, DropEffect::Move, true);

        assert!(!completion.accepted());
    }

    #[test]
    fn missing_drop_effect_is_not_accepted() {
        let completion = DragCompletion::from_backend(false, DropEffect::None, false);

        assert!(!completion.accepted());
        assert_eq!(completion.effect(), DropEffect::None);
    }

    #[test]
    fn pinned_items_make_the_whole_drag_copy_only() {
        assert_eq!(DragOffer::for_items([]), DragOffer::CopyOnly);
        assert_eq!(DragOffer::for_items([false, false]), DragOffer::CopyAndMove);
        assert_eq!(
            DragOffer::for_items([false, true, false]),
            DragOffer::CopyOnly
        );
    }
}
