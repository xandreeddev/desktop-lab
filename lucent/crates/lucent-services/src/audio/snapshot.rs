//! Copy borrowed backend data, then construct transport-free domain snapshots.
use lucent_domain::*;
use pulse::{PA_VOLUME_NORM, pa_cvolume, pa_cvolume_max, pa_cvolume_valid};
use std::collections::{BTreeMap, BTreeSet};

const OBJECT_LIMIT: usize = 4096;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Kind {
    Output,
    Input,
    Playback,
    Capture,
}
impl Kind {
    pub fn direction(self) -> AudioDirection {
        match self {
            Self::Output | Self::Playback => AudioDirection::Playback,
            Self::Input | Self::Capture => AudioDirection::Capture,
        }
    }
    pub fn device(self) -> bool {
        matches!(self, Self::Output | Self::Input)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Key(pub Kind, pub u32);
#[derive(Clone)]
pub(super) struct Record {
    pub key: Key,
    pub name: String,
    pub label: String,
    pub application: String,
    pub volumes: Option<pa_cvolume>,
    pub writable: bool,
    pub muted: bool,
    pub monitor: bool,
    pub device: Option<Key>,
}
impl Record {
    pub fn device(&self, id: AudioObjectId) -> Result<AudioDevice> {
        if !self.key.0.device() {
            return Err(DomainError::Invalid("Audio stream is not a device".into()));
        }
        Ok(AudioDevice {
            id,
            key: AudioDeviceKey::new(self.name.clone())?,
            name: self.label.clone(),
            direction: self.key.0.direction(),
            volume: volume(
                self.volumes
                    .as_ref()
                    .ok_or_else(|| DomainError::Unavailable("Missing device volume".into()))?,
            )?,
            muted: self.muted,
            monitor: self.monitor,
        })
    }
}
pub(super) fn label(value: Option<&str>) -> String {
    value
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(512)
        .collect()
}
pub(super) fn volume(channels: &pa_cvolume) -> Result<AudioVolume> {
    // SAFETY: channels is a live, fully initialized C value.
    if unsafe { pa_cvolume_valid(channels) } == 0 {
        return Err(DomainError::Unavailable(
            "Audio service returned invalid channel volume".into(),
        ));
    }
    AudioVolume::new(f64::from(unsafe { pa_cvolume_max(channels) }) / f64::from(PA_VOLUME_NORM))
}
#[derive(Default)]
pub(super) struct Data {
    pub records: BTreeMap<Key, Record>,
    pub defaults: [Option<Key>; 2],
    pub remaining: usize,
    pub error: Option<DomainError>,
}
impl Data {
    pub fn receive(&mut self, item: Option<Record>) {
        if let Some(record) = item {
            if self.records.len() >= OBJECT_LIMIT {
                self.error = Some(DomainError::Unavailable("Too many audio objects".into()));
            } else {
                self.records.insert(record.key, record);
            }
        }
    }
    pub fn finish(&mut self, failed: bool) {
        self.remaining = self.remaining.saturating_sub(1);
        if failed {
            self.error = Some(DomainError::Unavailable("Audio state query failed".into()));
        }
    }
}
#[derive(Default)]
pub(super) struct Objects {
    pub entries: BTreeMap<Key, (AudioObjectId, Record)>,
    pub defaults: [Option<Key>; 2],
}
impl Objects {
    pub fn invalidate(&mut self, removed: &BTreeSet<Key>) {
        self.entries.retain(|key, _| !removed.contains(key));
    }
    pub fn replace(
        &mut self,
        data: Data,
        allocate: &mut dyn FnMut() -> Result<AudioObjectId>,
    ) -> Result<AudioSnapshot> {
        let mut entries = BTreeMap::new();
        for (key, record) in data.records {
            let id = if let Some((id, _)) = self.entries.get(&key) {
                *id
            } else {
                allocate()?
            };
            entries.insert(key, (id, record));
        }
        self.entries = entries;
        self.defaults = data.defaults;
        self.snapshot()
    }
    pub fn resolve(&self, id: AudioObjectId) -> Result<&Record> {
        self.entries
            .values()
            .find(|(current, _)| *current == id)
            .map(|(_, r)| r)
            .ok_or_else(|| {
                DomainError::Unavailable("Selected audio object is no longer available".into())
            })
    }
    pub fn default_output(&self) -> Result<AudioObjectId> {
        self.defaults[0]
            .and_then(|key| self.entries.get(&key))
            .map(|(id, _)| *id)
            .ok_or_else(|| DomainError::Unavailable("No default audio output".into()))
    }
    fn snapshot(&self) -> Result<AudioSnapshot> {
        let id = |key: Key| self.entries.get(&key).map(|(id, _)| *id);
        let mut result = AudioSnapshot {
            default_output: self.defaults[0].and_then(id),
            default_input: self.defaults[1].and_then(id),
            ..Default::default()
        };
        for (object_id, record) in self.entries.values() {
            if record.key.0.device() {
                result.devices.push(record.device(*object_id)?);
            } else {
                result.streams.push(AudioStream {
                    id: *object_id,
                    name: record.label.clone(),
                    application: record.application.clone(),
                    direction: record.key.0.direction(),
                    device: record.device.and_then(id),
                    volume: record.volumes.as_ref().map(volume).transpose()?,
                    volume_writable: record.writable,
                    muted: record.muted,
                });
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    pub fn record(kind: Kind, index: u32) -> Record {
        let mut channels = pa_cvolume::default();
        // SAFETY: fully allocated output; two initialized channels.
        unsafe {
            pulse::pa_cvolume_set(&mut channels, 2, PA_VOLUME_NORM);
        }
        Record {
            key: Key(kind, index),
            name: format!("device{index}"),
            label: format!("Device {index}"),
            application: "test".into(),
            volumes: Some(channels),
            writable: true,
            muted: false,
            monitor: false,
            device: None,
        }
    }
    #[test]
    fn audio_snapshot_ids_survive_updates_but_never_removal_or_reconnect() {
        let mut next = 0;
        let mut allocate = || {
            next += 1;
            AudioObjectId::new(next)
        };
        let make = || {
            let mut data = Data::default();
            data.receive(Some(record(Kind::Output, 7)));
            data.defaults[0] = Some(Key(Kind::Output, 7));
            data
        };
        let mut objects = Objects::default();
        let first = objects
            .replace(make(), &mut allocate)
            .unwrap()
            .default_output
            .unwrap();
        assert_eq!(
            objects
                .replace(make(), &mut allocate)
                .unwrap()
                .default_output,
            Some(first)
        );
        objects.invalidate(&[Key(Kind::Output, 7)].into());
        assert!(objects.resolve(first).is_err());
        let replacement = objects
            .replace(make(), &mut allocate)
            .unwrap()
            .default_output
            .unwrap();
        assert_ne!(replacement, first);
        let mut reconnected = Objects::default();
        assert_ne!(
            reconnected
                .replace(make(), &mut allocate)
                .unwrap()
                .default_output,
            Some(replacement)
        );
    }
    #[test]
    fn audio_snapshot_bounds_objects_and_rejects_invalid_channels() {
        let mut data = Data::default();
        for index in 0..=OBJECT_LIMIT {
            data.receive(Some(record(Kind::Output, index as u32)));
        }
        assert_eq!(data.records.len(), OBJECT_LIMIT);
        assert!(data.error.is_some());
        assert!(volume(&pa_cvolume::default()).is_err());
    }
}
