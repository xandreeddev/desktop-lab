//! Adapter-owned publication storage. A shared frame is never writable here.
use super::{Arc, Result, failed};

const SLOTS: usize = 2;
const RETAINED_BYTES: usize = 64 * 1024 * 1024;

#[derive(Default)]
pub(super) struct Pool {
    slots: Vec<Arc<Vec<u8>>>,
}

impl Pool {
    pub(super) fn frame(
        &mut self,
        length: usize,
        fill: impl FnOnce(&mut Vec<u8>) -> Result<()>,
    ) -> Result<Arc<Vec<u8>>> {
        for slot in &mut self.slots {
            if slot.capacity() >= length
                && let Some(pixels) = Arc::get_mut(slot)
            {
                pixels.clear();
                fill(pixels)?;
                if pixels.len() != length {
                    return Err(failed());
                }
                return Ok(slot.clone());
            }
        }
        // All aliases must retire before reuse. A slow or paused consumer gets
        // an ordinary non-retained allocation instead of a wait or stolen frame.
        let pixels = packed(length, fill)?;
        let retained = self.slots.iter().map(|slot| slot.capacity()).sum::<usize>();
        if self.slots.len() < SLOTS
            && retained
                .checked_add(pixels.capacity())
                .is_some_and(|bytes| bytes <= RETAINED_BYTES)
        {
            self.slots.push(pixels.clone());
        }
        Ok(pixels)
    }
}

pub(super) fn packed(
    length: usize,
    fill: impl FnOnce(&mut Vec<u8>) -> Result<()>,
) -> Result<Arc<Vec<u8>>> {
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(length).map_err(|_| failed())?;
    fill(&mut pixels)?;
    if pixels.len() != length {
        return Err(failed());
    }
    Ok(Arc::new(pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(pool: &mut Pool, length: usize, value: u8) -> Arc<Vec<u8>> {
        pool.frame(length, |bytes| {
            bytes.resize(length, value);
            Ok(())
        })
        .unwrap()
    }

    #[test]
    fn released_consumers_reuse_storage_and_shared_consumers_never_change() {
        let mut pool = Pool::default();
        let first = frame(&mut pool, 32, 1);
        let first_pointer = first.as_ptr();
        let second = frame(&mut pool, 32, 2);
        assert_ne!(second.as_ptr(), first_pointer);
        assert_eq!(first.as_slice(), &[1; 32]);
        drop(first);
        let third = frame(&mut pool, 32, 3);
        assert_eq!(third.as_ptr(), first_pointer);
        assert_eq!(second.as_slice(), &[2; 32]);
        assert_eq!(pool.slots.len(), SLOTS);
    }

    #[test]
    fn held_slots_fall_back_without_growing_retention_or_waiting_for_release() {
        let mut pool = Pool::default();
        let first = frame(&mut pool, 16, 1);
        let second = frame(&mut pool, 16, 2);
        let retained = pool.slots.iter().map(|slot| slot.capacity()).sum::<usize>();
        let fallback = frame(&mut pool, 16, 3);
        assert!(!pool.slots.iter().any(|slot| Arc::ptr_eq(slot, &fallback)));
        assert_eq!(pool.slots.len(), SLOTS);
        assert_eq!(
            pool.slots.iter().map(|slot| slot.capacity()).sum::<usize>(),
            retained
        );
        assert_eq!(first.as_slice(), &[1; 16]);
        assert_eq!(second.as_slice(), &[2; 16]);
        assert_eq!(fallback.as_slice(), &[3; 16]);
    }

    #[test]
    fn dropping_pool_releases_unused_storage_and_preserves_outstanding_pixels() {
        let mut pool = Pool::default();
        let frame = frame(&mut pool, 16, 4);
        let weak = Arc::downgrade(&frame);
        drop(pool);
        assert_eq!(Arc::strong_count(&frame), 1);
        assert_eq!(frame.as_slice(), &[4; 16]);
        drop(frame);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn retention_charges_capacity_instead_of_length_and_rejects_excess() {
        let mut pool = Pool::default();
        // Reserve the limit without filling/touching a 64-MiB frame. Keeping
        // this alias denies reuse and exercises the actual new-slot admission.
        let held = Arc::new(Vec::with_capacity(RETAINED_BYTES));
        pool.slots.push(held.clone());
        let fallback = frame(&mut pool, 16, 7);
        assert_eq!(pool.slots.len(), 1);
        assert!(!pool.slots.iter().any(|slot| Arc::ptr_eq(slot, &fallback)));
        assert!(pool.slots.iter().map(|slot| slot.capacity()).sum::<usize>() <= RETAINED_BYTES);
    }

    #[test]
    fn failed_packing_never_publishes_partial_storage() {
        let mut pool = Pool::default();
        let first = frame(&mut pool, 16, 1);
        drop(first);
        assert!(
            pool.frame(16, |bytes| {
                bytes.push(7);
                Ok(())
            })
            .is_err()
        );
        assert_eq!(frame(&mut pool, 16, 9).as_slice(), &[9; 16]);
    }
}
