//! Controlled list navigation. Callers retain items, stable identities and selection.
use crate::Key;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListNavigation {
    Previous,
    Next,
    PagePrevious,
    PageNext,
    First,
    Last,
    Select(usize),
}

impl ListNavigation {
    /// Plain list keys only. A client decides which keys its editor retains.
    pub fn from_key(key: &Key) -> Option<Self> {
        Some(match key {
            Key::Up => Self::Previous,
            Key::Down => Self::Next,
            Key::PageUp => Self::PagePrevious,
            Key::PageDown => Self::PageNext,
            Key::Home => Self::First,
            Key::End => Self::Last,
            _ => return None,
        })
    }

    /// Find a selectable row near the requested destination, without wrapping.
    /// A page is measured in visible rows, including any disabled rows.
    pub fn target(
        self,
        selected: usize,
        count: usize,
        page: usize,
        mut selectable: impl FnMut(usize) -> bool,
    ) -> Option<usize> {
        let last = count.checked_sub(1)?;
        let selected = selected.min(last);
        let page = page.max(1);
        let (target, forward) = match self {
            Self::Previous => (selected.saturating_sub(1), false),
            Self::Next => (selected.saturating_add(1).min(last), true),
            Self::PagePrevious => (selected.saturating_sub(page), false),
            Self::PageNext => (selected.saturating_add(page).min(last), true),
            Self::First => (0, true),
            Self::Last => (last, false),
            Self::Select(index) => (index.min(last), true),
        };
        if forward {
            (target..count)
                .find(|index| selectable(*index))
                .or_else(|| (0..target).rev().find(|index| selectable(*index)))
        } else {
            (0..=target)
                .rev()
                .find(|index| selectable(*index))
                .or_else(|| (target + 1..count).find(|index| selectable(*index)))
        }
    }
}

/// Keep selection visible while preserving the previous window where possible.
/// The empty list has an empty window; a nonempty list shows at least one row.
pub fn list_window(selected: usize, first: usize, count: usize, visible: usize) -> Range<usize> {
    let Some(last) = count.checked_sub(1) else {
        return 0..0;
    };
    let visible = visible.max(1).min(count);
    let selected = selected.min(last);
    let first = first
        .min(selected)
        .max((selected + 1).saturating_sub(visible))
        .min(count - visible);
    first..first + visible
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_use_visible_rows_and_clamp_both_ends() {
        let mut selected = 0;
        let mut first = 0;
        for expected in [7, 14, 19, 19] {
            selected = ListNavigation::PageNext
                .target(selected, 20, 7, |_| true)
                .unwrap();
            first = list_window(selected, first, 20, 7).start;
            assert_eq!(selected, expected);
            assert!(list_window(selected, first, 20, 7).contains(&selected));
        }
        for expected in [12, 5, 0, 0] {
            selected = ListNavigation::PagePrevious
                .target(selected, 20, 7, |_| true)
                .unwrap();
            first = list_window(selected, first, 20, 7).start;
            assert_eq!(selected, expected);
            assert!(list_window(selected, first, 20, 7).contains(&selected));
        }
    }

    #[test]
    fn disabled_destinations_follow_direction_and_endpoints_do_not_wrap() {
        let enabled = |index| matches!(index, 1 | 3 | 8);
        assert_eq!(ListNavigation::First.target(8, 10, 4, enabled), Some(1));
        assert_eq!(ListNavigation::Last.target(1, 10, 4, enabled), Some(8));
        assert_eq!(ListNavigation::Next.target(1, 10, 4, enabled), Some(3));
        assert_eq!(ListNavigation::Previous.target(8, 10, 4, enabled), Some(3));
        assert_eq!(ListNavigation::PageNext.target(1, 10, 4, enabled), Some(8));
        assert_eq!(
            ListNavigation::PagePrevious.target(8, 10, 4, enabled),
            Some(3)
        );
        assert_eq!(ListNavigation::Next.target(8, 10, 4, enabled), Some(8));
        assert_eq!(ListNavigation::Previous.target(1, 10, 4, enabled), Some(1));
        assert_eq!(ListNavigation::Select(4).target(1, 10, 4, enabled), Some(8));
        assert_eq!(ListNavigation::Select(9).target(1, 10, 4, enabled), Some(8));
    }

    #[test]
    fn empty_and_disabled_lists_have_no_destination() {
        for navigation in [
            ListNavigation::First,
            ListNavigation::Last,
            ListNavigation::Next,
            ListNavigation::PagePrevious,
        ] {
            assert_eq!(
                navigation.target(usize::MAX, 0, 0, |_| panic!("empty list")),
                None
            );
            assert_eq!(navigation.target(5, 6, 4, |_| false), None);
        }
        assert_eq!(list_window(usize::MAX, usize::MAX, 0, 0), 0..0);
    }

    #[test]
    fn stale_windows_shrink_and_large_values_do_not_overflow() {
        assert_eq!(list_window(15, 10, 20, 7), 10..17);
        assert_eq!(list_window(15, 10, 3, 7), 0..3);
        assert_eq!(
            list_window(usize::MAX, usize::MAX, usize::MAX, 0),
            usize::MAX - 1..usize::MAX
        );
        assert_eq!(list_window(0, 0, usize::MAX, usize::MAX), 0..usize::MAX);
        assert_eq!(
            ListNavigation::PageNext.target(usize::MAX - 2, usize::MAX, usize::MAX, |_| true),
            Some(usize::MAX - 1)
        );
        assert_eq!(
            ListNavigation::PagePrevious.target(usize::MAX, 3, 0, |_| true),
            Some(1)
        );
    }

    #[test]
    fn plain_key_mapping_keeps_modified_and_editing_keys_with_the_client() {
        assert_eq!(
            ListNavigation::from_key(&Key::PageDown),
            Some(ListNavigation::PageNext)
        );
        assert_eq!(
            ListNavigation::from_key(&Key::Home),
            Some(ListNavigation::First)
        );
        assert_eq!(ListNavigation::from_key(&Key::Left), None);
        assert_eq!(
            ListNavigation::from_key(&Key::Modified {
                key: Box::new(Key::PageDown),
                modifiers: Default::default(),
            }),
            None
        );
    }
}
