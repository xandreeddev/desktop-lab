//! Validate output sizes before issuing GPU commands or allocating readback storage.
use crate::Error;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PhysicalExtent {
    pub width: u32,
    pub height: u32,
}
impl PhysicalExtent {
    pub fn new(width: u32, height: u32, scale: u32, limit: u32) -> Result<Self, Error> {
        if width == 0 || height == 0 || scale == 0 {
            return Err("nonzero render dimensions and scale required".into());
        }
        let width = width.checked_mul(scale).ok_or("render width overflow")?;
        let height = height.checked_mul(scale).ok_or("render height overflow")?;
        if width > limit || height > limit {
            return Err(format!(
                "render extent {width}x{height} exceeds device texture dimension limit {limit}"
            )
            .into());
        }
        Ok(Self { width, height })
    }

    pub fn readback(&self, buffer_limit: u64) -> Result<ReadbackLayout, Error> {
        let row = self.width.checked_mul(4).ok_or("readback row overflow")?;
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let stride = row
            .checked_add(alignment - 1)
            .map(|value| value / alignment)
            .and_then(|rows| rows.checked_mul(alignment))
            .ok_or("readback row alignment overflow")?;
        let buffer_bytes = u64::from(stride) * u64::from(self.height);
        if buffer_bytes > buffer_limit {
            return Err(format!(
                "readback needs {buffer_bytes} bytes, exceeding device buffer limit {buffer_limit}"
            )
            .into());
        }
        // Validate both the mapped slice and the packed output on this host.
        if buffer_bytes > isize::MAX as u64 {
            return Err("readback exceeds host slice capacity".into());
        }
        let rgba_bytes = usize::try_from(u64::from(row) * u64::from(self.height))
            .map_err(|_| "snapshot exceeds host address space")?;
        Ok(ReadbackLayout {
            stride,
            row_bytes: row as usize,
            buffer_bytes,
            rgba_bytes,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ReadbackLayout {
    pub stride: u32,
    pub row_bytes: usize,
    pub buffer_bytes: u64,
    pub rgba_bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_rejects_zero_overflow_and_device_limits() {
        for (width, height, scale) in [(0, 1, 1), (1, 0, 1), (1, 1, 0)] {
            assert!(PhysicalExtent::new(width, height, scale, 8192).is_err());
        }
        for (width, height) in [(u32::MAX, 1), (1, u32::MAX)] {
            assert!(PhysicalExtent::new(width, height, 2, u32::MAX).is_err());
        }
        for (width, height) in [(4097, 1), (1, 4097)] {
            assert!(PhysicalExtent::new(width, height, 2, 8192).is_err());
        }
        assert_eq!(
            PhysicalExtent::new(4096, 1, 2, 8192).unwrap(),
            PhysicalExtent {
                width: 8192,
                height: 2
            }
        );
    }

    #[test]
    fn readback_accounts_for_aligned_rows_at_each_density() {
        for (scale, stride, buffer_bytes, rgba_bytes) in [(1, 512, 1536, 780), (2, 768, 4608, 3120)]
        {
            let layout = PhysicalExtent::new(65, 3, scale, 8192)
                .unwrap()
                .readback(buffer_bytes)
                .unwrap();
            assert_eq!(
                layout,
                ReadbackLayout {
                    stride,
                    row_bytes: (65 * scale * 4) as usize,
                    buffer_bytes,
                    rgba_bytes,
                }
            );
        }
        assert_eq!(
            PhysicalExtent::new(64, 1, 1, 8192)
                .unwrap()
                .readback(256)
                .unwrap()
                .stride,
            256
        );
    }

    #[test]
    fn readback_rejects_stride_overflow_and_padded_buffer_excess() {
        for width in [u32::MAX / 4 + 1, u32::MAX / 4] {
            assert!(
                PhysicalExtent::new(width, 1, 1, u32::MAX)
                    .unwrap()
                    .readback(u64::MAX)
                    .is_err()
            );
        }
        let extent = PhysicalExtent::new(65, 3, 1, 8192).unwrap();
        assert!(
            extent.readback(780).is_err(),
            "packed bytes omit row padding"
        );
        assert!(extent.readback(1535).is_err());
        assert!(extent.readback(1536).is_ok());
        assert!(
            PhysicalExtent::new(u32::MAX / 4 - 64, u32::MAX, 1, u32::MAX)
                .unwrap()
                .readback(u64::MAX)
                .is_err(),
            "mapped slices must fit the host even if device limits permit them"
        );
    }
}
