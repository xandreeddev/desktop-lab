//! Accounting copied from the pinned observer; stopped-only caller contract.
use serde_json::{Value, json};
// glibc's arena accounting includes all arenas, but excludes direct-mapped
// malloc chunks. Thread-cache frees can still count as arena in-use bytes.
// None of these counters describes Rust object ownership or resident pages.
#[cfg(any(test, all(target_os = "linux", target_env = "gnu")))]
struct MallocAccounting {
    arena: u64,
    in_use: u64,
    free: u64,
    direct_mmap: u64,
    direct_mmap_regions: u64,
    free_chunks: u64,
}

fn allocator_unavailable(reason: &'static str) -> Value {
    json!({"kind":"glibc_mallinfo2", "available":false, "reason":reason})
}

#[cfg(any(test, all(target_os = "linux", target_env = "gnu")))]
fn allocator_report(version: &str, stats: MallocAccounting) -> Value {
    let parts: Vec<_> = version.split('.').collect();
    if version.len() > 32
        || parts.len() < 2
        || !parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return allocator_unavailable("invalid_runtime_version");
    }
    let major = parts[0].parse::<u32>().ok();
    let minor = parts[1].parse::<u32>().ok();
    if !matches!((major, minor), (Some(major), Some(minor)) if (major, minor) >= (2, 33)) {
        return allocator_unavailable("unsupported_glibc_version");
    }
    json!({"kind":"glibc_mallinfo2", "available":true, "glibc_version":version,
        "bytes":{"arena":stats.arena, "arena_accounted_in_use":stats.in_use,
                 "arena_free":stats.free, "direct_mmap":stats.direct_mmap,
                 "malloc_accounted_in_use_total":stats.in_use.checked_add(stats.direct_mmap)},
        "counts":{"arena_free_chunks":stats.free_chunks,
                  "direct_mmap_regions":stats.direct_mmap_regions}})
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub(super) fn allocator() -> Value {
    // GNU guarantees a static NUL-terminated version string. This optional
    // diagnostic targets glibc >= 2.33 (mallinfo2's symbol version). It neither
    // tunes nor trims malloc, and runs only with the playback worker stopped.
    let version = unsafe { std::ffi::CStr::from_ptr(libc::gnu_get_libc_version()) };
    let Ok(version) = version.to_str() else {
        return allocator_unavailable("invalid_runtime_version");
    };
    // SAFETY: mallinfo2 returns an owned scalar snapshot without arguments.
    // The example has already initialized malloc, never calls mallopt, and
    // invokes this outside signal handlers and playback-worker lifetimes.
    let stats = unsafe { libc::mallinfo2() };
    let converted = (
        u64::try_from(stats.arena),
        u64::try_from(stats.uordblks),
        u64::try_from(stats.fordblks),
        u64::try_from(stats.hblkhd),
        u64::try_from(stats.hblks),
        u64::try_from(stats.ordblks),
    );
    match converted {
        (
            Ok(arena),
            Ok(in_use),
            Ok(free),
            Ok(direct_mmap),
            Ok(direct_mmap_regions),
            Ok(free_chunks),
        ) => allocator_report(
            version,
            MallocAccounting {
                arena,
                in_use,
                free,
                direct_mmap,
                direct_mmap_regions,
                free_chunks,
            },
        ),
        _ => allocator_unavailable("counter_out_of_range"),
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub(super) fn allocator() -> Value {
    allocator_unavailable("unsupported_target")
}
