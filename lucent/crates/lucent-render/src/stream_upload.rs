//! Private reuse of stream-upload staging. This caps our owned payload, not RSS,
//! driver allocations or the existing queue-write fallback. Static images/mips
//! keep their original upload path.
use super::{Error, Gpu, TextureResource};
use lucent_api::ImageData;
use std::{
    rc::Rc,
    sync::mpsc::{self, Receiver, SyncSender},
};
use wgpu::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoder, Device, Extent3d, MapMode, Origin3d,
    PollType, TexelCopyBufferInfo, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureAspect,
};

// Resource policy in the renderer, independent of desktop layout/theme tokens.
const MAX_SLOTS: usize = 2;
const PAYLOAD_BUDGET: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Layout {
    row: usize,
    stride: u32,
    bytes: u64,
}
impl Layout {
    fn rgba(width: u32, height: u32, pixel_bytes: usize, buffer_limit: u64) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        let row = width.checked_mul(4)?;
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let stride = row.checked_add(alignment - 1)? / alignment * alignment;
        let bytes = u64::from(stride).checked_mul(u64::from(height))?;
        if bytes > buffer_limit
            || bytes > isize::MAX as u64
            || u64::from(row).checked_mul(u64::from(height))? != pixel_bytes as u64
        {
            return None;
        }
        Some(Self {
            row: row as usize,
            stride,
            bytes,
        })
    }
    fn write(self, destination: &mut [u8], pixels: &[u8]) {
        if self.row == self.stride as usize {
            destination[..pixels.len()].copy_from_slice(pixels);
        } else {
            for (target, source) in destination
                .chunks_exact_mut(self.stride as usize)
                .zip(pixels.chunks_exact(self.row))
            {
                target[..self.row].copy_from_slice(source);
            }
        }
    }
}

struct Update {
    texture: Rc<TextureResource>,
    data: ImageData,
    revision: u64,
}
/// Last update of each actual GPU texture wins, as with the old queue writes.
/// Different-sized resources retain separate updates while old draw views live.
#[derive(Default)]
pub(super) struct StreamUpdates(Vec<Update>);
impl StreamUpdates {
    pub(super) fn reserve(&mut self, count: usize) -> Result<(), Error> {
        self.0.try_reserve_exact(count)?;
        Ok(())
    }
    pub(super) fn revision(&self, texture: &Rc<TextureResource>) -> u64 {
        self.0
            .iter()
            .find(|update| Rc::ptr_eq(&update.texture, texture))
            .map_or_else(|| texture.revision.get(), |update| update.revision)
    }
    pub(super) fn defer(&mut self, texture: &Rc<TextureResource>, data: &ImageData, revision: u64) {
        if let Some(update) = self
            .0
            .iter_mut()
            .find(|update| Rc::ptr_eq(&update.texture, texture))
        {
            update.data = data.clone();
            update.revision = revision;
        } else {
            self.0.push(Update {
                texture: texture.clone(),
                data: data.clone(),
                revision,
            });
        }
    }
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub(super) fn encode(
        &self,
        gpu: &Gpu,
        uploads: &mut Submission<'_>,
        encoder: &mut CommandEncoder,
    ) -> u64 {
        let mut bytes = 0;
        for update in &self.0 {
            let data = &update.data;
            // Exactly one path per texture/batch. Queue writes run before the
            // explicit encoder; a later fallback can never overtake an earlier
            // pooled update of this same texture.
            if !uploads.copy(encoder, &update.texture, data) {
                gpu.queue.write_texture(
                    TexelCopyTextureInfo {
                        texture: &update.texture._texture,
                        mip_level: 0,
                        origin: Origin3d::ZERO,
                        aspect: TextureAspect::All,
                    },
                    data.pixels(),
                    TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(data.width() * 4),
                        rows_per_image: Some(data.height()),
                    },
                    Extent3d {
                        width: data.width(),
                        height: data.height(),
                        depth_or_array_layers: 1,
                    },
                );
            }
            bytes += data.bytes() as u64;
        }
        bytes
    }
    pub(super) fn commit(self) {
        for update in self.0 {
            update.texture.revision.set(update.revision);
            *update.texture.image_lifetime.borrow_mut() = Some(update.data.lifetime());
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    id: u64,
    generation: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Mapped,
    Pending(Key),
    Failed,
}
struct Slot {
    id: u64,
    generation: u64,
    buffer: Buffer,
    phase: Phase,
}
#[derive(Clone, Copy)]
struct Completion {
    key: Key,
    success: bool,
}
#[derive(Default, Clone, Copy, Debug)]
struct Counters {
    allocations: u64,
    copies: u64,
    fallbacks: u64,
    peak_owned_bytes: u64,
}
pub(super) struct Pool {
    slots: Vec<Slot>,
    sender: SyncSender<Completion>,
    receiver: Receiver<Completion>,
    next_id: u64,
    disabled: bool,
    budget: u64,
    counters: Counters,
}
impl Default for Pool {
    fn default() -> Self {
        let (sender, receiver) = mpsc::sync_channel(MAX_SLOTS);
        Self {
            slots: Vec::with_capacity(MAX_SLOTS),
            sender,
            receiver,
            next_id: 0,
            disabled: false,
            budget: PAYLOAD_BUDGET,
            counters: Counters::default(),
        }
    }
}
impl Pool {
    fn owned_bytes(&self) -> u64 {
        self.slots.iter().map(|slot| slot.buffer.size()).sum()
    }
    fn complete(&mut self, reply: Completion) {
        if let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| slot.id == reply.key.id && slot.phase == Phase::Pending(reply.key))
        {
            slot.phase = if reply.success {
                Phase::Mapped
            } else {
                Phase::Failed
            };
            // Do not allocate/retry a failing native map every frame.
            self.disabled |= !reply.success;
        }
    }
    fn drain(&mut self) {
        while let Ok(reply) = self.receiver.try_recv() {
            self.complete(reply);
        }
    }
    pub(super) fn begin<'a>(&'a mut self, device: &'a Device, demand: bool) -> Submission<'a> {
        if demand {
            self.drain();
            if !self.disabled
                && self
                    .slots
                    .iter()
                    .any(|slot| matches!(slot.phase, Phase::Pending(_)))
            {
                // Maintenance follows an already requested upload, never an idle
                // timer or a blocking GUI wait. Callbacks only send a tiny ack.
                self.disabled |= device.poll(PollType::Poll).is_err();
                self.drain();
            }
        }
        Submission {
            pool: self,
            device,
            staged: [None; MAX_SLOTS],
            state: SubmissionState::Encoding,
        }
    }
    fn acquire(&mut self, device: &Device, bytes: u64) -> Option<usize> {
        if self.disabled || bytes > self.budget {
            return None;
        }
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.phase == Phase::Mapped && slot.buffer.size() >= bytes)
        {
            return Some(index);
        }
        // Retire only CPU-owned mapped buffers. Pending buffers and their bytes
        // stay owned/charged until the actual successful completion arrives.
        while self.slots.len() >= MAX_SLOTS || self.owned_bytes().checked_add(bytes)? > self.budget
        {
            let index = self
                .slots
                .iter()
                .position(|slot| slot.phase == Phase::Mapped)?;
            self.slots.swap_remove(index);
        }
        let id = self.next_id.checked_add(1)?;
        self.next_id = id;
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some("bounded framework stream upload"),
            size: bytes,
            usage: BufferUsages::MAP_WRITE | BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });
        self.slots.push(Slot {
            id,
            generation: 0,
            buffer,
            phase: Phase::Mapped,
        });
        self.counters.allocations += 1;
        self.counters.peak_owned_bytes = self.counters.peak_owned_bytes.max(self.owned_bytes());
        Some(self.slots.len() - 1)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SubmissionState {
    Encoding,
    Submitting,
    Submitted,
}
pub(super) struct Submission<'a> {
    pool: &'a mut Pool,
    device: &'a Device,
    staged: [Option<Key>; MAX_SLOTS],
    state: SubmissionState,
}
impl Submission<'_> {
    fn copy(
        &mut self,
        encoder: &mut CommandEncoder,
        texture: &TextureResource,
        data: &ImageData,
    ) -> bool {
        let Some(layout) = Layout::rgba(
            data.width(),
            data.height(),
            data.pixels().len(),
            self.device.limits().max_buffer_size,
        ) else {
            self.pool.counters.fallbacks += 1;
            return false;
        };
        let Some(index) = self.pool.acquire(self.device, layout.bytes) else {
            self.pool.counters.fallbacks += 1;
            return false;
        };
        let slot = &mut self.pool.slots[index];
        let Some(generation) = slot.generation.checked_add(1) else {
            self.pool.disabled = true;
            self.pool.counters.fallbacks += 1;
            return false;
        };
        {
            let Ok(mut view) = slot.buffer.get_mapped_range_mut(..) else {
                slot.phase = Phase::Failed;
                self.pool.disabled = true;
                self.pool.counters.fallbacks += 1;
                return false;
            };
            layout.write(&mut view[..layout.bytes as usize], data.pixels());
        }
        slot.buffer.unmap();
        let key = Key {
            id: slot.id,
            generation,
        };
        slot.generation = generation;
        slot.phase = Phase::Pending(key);
        let target = self
            .staged
            .iter_mut()
            .find(|entry| entry.is_none())
            .expect("at most two staged slots");
        *target = Some(key);
        encoder.copy_buffer_to_texture(
            TexelCopyBufferInfo {
                buffer: &slot.buffer,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(layout.stride),
                    rows_per_image: Some(data.height()),
                },
            },
            TexelCopyTextureInfo {
                texture: &texture._texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            Extent3d {
                width: data.width(),
                height: data.height(),
                depth_or_array_layers: 1,
            },
        );
        let sender = self.pool.sender.clone();
        encoder.map_buffer_on_submit(&slot.buffer, MapMode::Write, .., move |result| {
            let _ = sender.try_send(Completion {
                key,
                success: result.is_ok(),
            });
        });
        self.pool.counters.copies += 1;
        true
    }
    pub(super) fn submitting(&mut self) {
        self.state = SubmissionState::Submitting;
    }
    pub(super) fn submitted(&mut self) {
        self.state = SubmissionState::Submitted;
    }
}
impl Drop for Submission<'_> {
    fn drop(&mut self) {
        if self.state == SubmissionState::Submitting {
            // A panic inside submit leaves native ownership uncertain. Keep the
            // bounded slots quarantined; never issue a competing map request.
            self.pool.disabled = true;
        } else if self.state == SubmissionState::Encoding {
            for key in self.staged.iter().flatten() {
                if let Some(slot) = self
                    .pool
                    .slots
                    .iter()
                    .find(|slot| slot.phase == Phase::Pending(*key))
                {
                    let sender = self.pool.sender.clone();
                    let key = *key;
                    // No encoder was submitted. The original deferred map will
                    // never run; schedule one recovery map without a busy wait.
                    slot.buffer.map_async(MapMode::Write, .., move |result| {
                        let _ = sender.try_send(Completion {
                            key,
                            success: result.is_ok(),
                        });
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
