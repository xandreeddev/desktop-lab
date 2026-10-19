//! Bounded diagnostic identities of streamed resources actually bound to a draw.
//! Contains no image data, asset names, media timestamps or domain policy.
const LIMIT: usize = 16;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamTrace {
    revisions: [(u64, u64); LIMIT],
    len: usize,
    truncated: bool,
}
impl StreamTrace {
    pub fn revisions(&self) -> &[(u64, u64)] {
        &self.revisions[..self.len]
    }
    pub fn truncated(&self) -> bool {
        self.truncated
    }
}
impl FromIterator<(u64, u64)> for StreamTrace {
    fn from_iter<T: IntoIterator<Item = (u64, u64)>>(values: T) -> Self {
        let mut trace = Self::default();
        for value in values {
            if trace.revisions().contains(&value) {
                continue;
            }
            if trace.len == LIMIT {
                trace.truncated = true;
                continue;
            }
            trace.revisions[trace.len] = value;
            trace.len += 1;
        }
        trace
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};
    #[test]
    fn tracing_reads_final_bound_revisions_after_shared_resource_updates() {
        let shared = Rc::new(Cell::new((11, 1)));
        let first_draw = shared.clone();
        shared.set((11, 2));
        let second_draw = shared.clone();
        let trace: StreamTrace = [first_draw, second_draw]
            .iter()
            .map(|value| value.get())
            .collect();
        assert_eq!(trace.revisions(), [(11, 2)]);
        assert!(!trace.truncated());
    }
    #[test]
    fn distinct_resources_are_bounded_and_duplicate_draws_do_not_consume_capacity() {
        let trace: StreamTrace = (0..LIMIT + 3).flat_map(|i| [(i as u64, 1); 2]).collect();
        assert_eq!(trace.revisions().len(), LIMIT);
        assert_eq!(trace.revisions()[0], (0, 1));
        assert!(trace.truncated());
        assert!(StreamTrace::default().revisions().is_empty());
    }
}
