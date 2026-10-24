//! Opt-in allocator bookkeeping, separate from texture payload and RSS.

/// A device allocator's reported live allocations and reserved memory blocks.
/// These are allocator bytes, not process RSS, total driver memory or residency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuAllocationStats {
    pub total_allocated_bytes: u64,
    pub total_reserved_bytes: u64,
    pub block_count: u64,
    pub allocation_count: u64,
}

/// Availability is explicit: an unsupported backend is not a zero-byte result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuAllocationSnapshot {
    Disabled,
    Unavailable,
    Available(GpuAllocationStats),
    CountOverflow,
}

pub(super) struct AllocationTotals {
    total_allocated_bytes: u64,
    total_reserved_bytes: u64,
    block_count: usize,
    allocation_count: usize,
}

impl From<wgpu::AllocatorReport> for AllocationTotals {
    fn from(report: wgpu::AllocatorReport) -> Self {
        Self {
            total_allocated_bytes: report.total_allocated_bytes,
            total_reserved_bytes: report.total_reserved_bytes,
            block_count: report.blocks.len(),
            allocation_count: report.allocations.len(),
        }
    }
}

pub(super) fn snapshot(
    enabled: bool,
    query: impl FnOnce() -> Option<AllocationTotals>,
) -> GpuAllocationSnapshot {
    if !enabled {
        return GpuAllocationSnapshot::Disabled;
    }
    let Some(report) = query() else {
        return GpuAllocationSnapshot::Unavailable;
    };
    let (Ok(block_count), Ok(allocation_count)) = (
        u64::try_from(report.block_count),
        u64::try_from(report.allocation_count),
    ) else {
        return GpuAllocationSnapshot::CountOverflow;
    };
    GpuAllocationSnapshot::Available(GpuAllocationStats {
        total_allocated_bytes: report.total_allocated_bytes,
        total_reserved_bytes: report.total_reserved_bytes,
        block_count,
        allocation_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn disabled_snapshot_never_queries_the_backend() {
        assert_eq!(
            snapshot(false, || panic!("disabled allocator queried")),
            GpuAllocationSnapshot::Disabled
        );
    }

    #[test]
    fn unavailable_backend_is_distinct_from_an_empty_allocator() {
        let calls = Cell::new(0);
        assert_eq!(
            snapshot(true, || {
                calls.set(calls.get() + 1);
                None
            }),
            GpuAllocationSnapshot::Unavailable
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(
            snapshot(true, || Some(AllocationTotals {
                total_allocated_bytes: 0,
                total_reserved_bytes: 0,
                block_count: 0,
                allocation_count: 0,
            })),
            GpuAllocationSnapshot::Available(GpuAllocationStats {
                total_allocated_bytes: 0,
                total_reserved_bytes: 0,
                block_count: 0,
                allocation_count: 0,
            })
        );
    }

    #[test]
    fn one_query_preserves_reported_totals_and_checked_counts() {
        let calls = Cell::new(0);
        let value = snapshot(true, || {
            calls.set(calls.get() + 1);
            Some(AllocationTotals {
                total_allocated_bytes: 224,
                total_reserved_bytes: 4096,
                block_count: 2,
                allocation_count: 3,
            })
        });
        assert_eq!(calls.get(), 1);
        assert_eq!(
            value,
            GpuAllocationSnapshot::Available(GpuAllocationStats {
                total_allocated_bytes: 224,
                total_reserved_bytes: 4096,
                block_count: 2,
                allocation_count: 3,
            })
        );
    }
}
