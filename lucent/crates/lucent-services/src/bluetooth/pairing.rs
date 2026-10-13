use super::{
    agent,
    catalog::{Catalog, Kind},
    native::*,
    *,
};
use std::collections::BTreeMap;
use zbus::zvariant::OwnedObjectPath;

pub(super) struct Pending {
    pub request: BluetoothPairingRequest,
    pub path: OwnedObjectPath,
    pub deadline: Instant,
    pub reply: Option<async_channel::Sender<agent::Answer>>,
}
pub(super) struct Pairing {
    pub host: agent::Host,
    pub attempt: Option<BluetoothPairingAttempt>,
    pub active_path: Option<OwnedObjectPath>,
    pub control_path: Option<OwnedObjectPath>,
    pub prompts: BTreeMap<BluetoothPairingId, Pending>,
    pub proposed: Vec<agent::Proposal>,
    epoch: u64,
    cancel_requested: bool,
    release_requires_retirement: bool,
}
impl Pairing {
    pub fn new(peer: String) -> Self {
        Self {
            host: agent::Host::new(peer),
            attempt: None,
            active_path: None,
            control_path: None,
            prompts: BTreeMap::new(),
            proposed: vec![],
            epoch: 0,
            cancel_requested: false,
            release_requires_retirement: false,
        }
    }
    fn cancel_requests(&mut self) {
        for (_, pending) in std::mem::take(&mut self.prompts) {
            if let Some(reply) = pending.reply {
                let _ = reply.try_send(agent::Answer::Cancelled);
            }
        }
        for proposed in self.proposed.drain(..) {
            if let Some(reply) = proposed.reply {
                let _ = reply.try_send(agent::Answer::Cancelled);
            }
        }
    }
    pub fn synchronize(
        &mut self,
        catalog: &Catalog,
        allocate: &mut dyn FnMut() -> Result<BluetoothPairingId>,
    ) -> Result<()> {
        self.remember_release();
        let epoch = self.host.shared.epoch();
        if epoch != self.epoch {
            self.cancel_requests();
            self.epoch = epoch;
        }
        if self.host.released() {
            self.host.registered = false;
            self.host.incoming = false;
            if self.release_requires_retirement && self.host.release_reply_dispatched() {
                return Err(unavailable("Bluetooth pairing agent was released"));
            }
        }
        self.prompts.retain(|_, pending| {
            let permitted = self.host.incoming
                || self.active_path.as_ref() == Some(&pending.path)
                || self.control_path.as_ref() == Some(&pending.path);
            let live = permitted
                && Instant::now() < pending.deadline
                && catalog.device_at(pending.path.as_str()) == Some(pending.request.device);
            if !live && let Some(reply) = &pending.reply {
                let _ = reply.try_send(agent::Answer::Cancelled);
            }
            live
        });
        if let Some(path) = &self.active_path {
            let target = self.attempt.as_ref().expect("active attempt").device;
            if catalog.resolve(target, Kind::Device).ok().as_ref() != Some(path) {
                // Do not CancelPairing against a reused path. Closing the owner
                // retires our uncertain operation and all connection identities.
                return Err(unavailable("Bluetooth pairing target disappeared"));
            }
        }
        for proposal in self.proposed.drain(..) {
            let device = catalog.device_at(proposal.path.as_str());
            let permitted = self.host.registered
                && (self.host.incoming
                    || self.active_path.as_ref() == Some(&proposal.path)
                    || self.control_path.as_ref() == Some(&proposal.path));
            if proposal.epoch != epoch
                || proposal.deadline <= Instant::now()
                || device.is_none()
                || !permitted
                || self.prompts.len() >= agent::PROPOSAL_LIMIT
            {
                if let Some(reply) = proposal.reply {
                    let _ = reply.try_send(agent::Answer::Cancelled);
                }
                continue;
            }
            // DisplayPasskey updates one display request rather than making a
            // fresh card on each remotely entered digit.
            if proposal.reply.is_none()
                && let Some(pending) = self
                    .prompts
                    .values_mut()
                    .find(|p| p.path == proposal.path && p.reply.is_none())
            {
                pending.request.prompt = proposal.prompt;
                pending.deadline = proposal.deadline;
                continue;
            }
            let id = allocate()?;
            self.prompts.insert(
                id,
                Pending {
                    request: BluetoothPairingRequest {
                        id,
                        device: device.unwrap(),
                        prompt: proposal.prompt,
                    },
                    path: proposal.path,
                    deadline: proposal.deadline,
                    reply: proposal.reply,
                },
            );
        }
        Ok(())
    }
    pub fn publish(&self, state: &mut BluetoothSnapshot) {
        state.pairing = self.attempt.clone();
        state.pairing_requests = self.prompts.values().map(|p| p.request.clone()).collect();
        state.incoming_pairing_enabled = self.host.incoming;
    }
    pub fn start(
        &mut self,
        id: BluetoothPairingId,
        device: BluetoothObjectId,
        path: OwnedObjectPath,
    ) -> Result<()> {
        if self.active_path.is_some() || !self.prompts.is_empty() {
            return Err(DomainError::Invalid(
                "Resolve the current Bluetooth pairing request first".into(),
            ));
        }
        self.cancel_requested = false;
        self.active_path = Some(path);
        self.attempt = Some(BluetoothPairingAttempt {
            id,
            device,
            phase: BluetoothPairingPhase::Pairing,
        });
        Ok(())
    }
    pub fn answer(
        &mut self,
        id: BluetoothPairingId,
        answer: BluetoothPairingAnswer,
    ) -> Result<Option<super::control::Plan>> {
        let pending = self.prompts.get(&id).ok_or_else(|| {
            DomainError::Invalid("Bluetooth request expired; use the current request".into())
        })?;
        lucent_usecases::bluetooth::validate_pairing_answer(&pending.request.prompt, &answer)?;
        let answer = match answer {
            BluetoothPairingAnswer::Accept => agent::Answer::Accepted,
            BluetoothPairingAnswer::Reject => agent::Answer::Rejected,
            BluetoothPairingAnswer::PinCode(value) => agent::Answer::Pin(value),
            BluetoothPairingAnswer::Passkey(value) => agent::Answer::Passkey(value),
        };
        let pending = self.prompts.remove(&id).unwrap();
        if let Some(reply) = pending.reply {
            reply
                .try_send(answer)
                .map_err(|_| DomainError::Invalid("Bluetooth request already ended".into()))?;
            Ok(None)
        } else {
            // Display callbacks already returned void. Rejecting their card
            // must cancel the current native operation, not merely hide it.
            if self.active_path.as_ref() == Some(&pending.path) {
                self.cancel_requested = true;
                Ok(Some(super::control::Plan::Device(
                    pending.path,
                    "CancelPairing",
                )))
            } else {
                // CancelPairing is defined for an operation initiated by Pair.
                // An incoming displayed challenge has already replied void;
                // disconnect its link rather than claiming a pending reply was rejected.
                Ok(Some(super::control::Plan::Device(
                    pending.path,
                    "Disconnect",
                )))
            }
        }
    }
    pub fn cancel(&mut self, id: BluetoothPairingId) -> Result<OwnedObjectPath> {
        if self.attempt.as_ref().is_none_or(|a| a.id != id) {
            return Err(DomainError::Invalid(
                "Choose the current pairing attempt".into(),
            ));
        }
        let path = self
            .active_path
            .clone()
            .ok_or_else(|| DomainError::Invalid("Bluetooth pairing already ended".into()))?;
        self.cancel_requested = true;
        self.cancel_requests();
        Ok(path)
    }
    pub fn finished(&mut self, id: BluetoothPairingId, result: Result<()>) {
        self.remember_release();
        if let Some(attempt) = self.attempt.as_mut().filter(|a| a.id == id) {
            attempt.phase = match result {
                Ok(()) => BluetoothPairingPhase::Succeeded,
                Err(_) if self.cancel_requested => BluetoothPairingPhase::Cancelled,
                Err(error) => BluetoothPairingPhase::Failed(error.to_string()),
            };
            self.active_path = None;
            self.cancel_requests();
        }
    }
    fn remember_release(&mut self) {
        self.release_requires_retirement |=
            self.host.released() && (self.active_path.is_some() || self.control_path.is_some());
    }
    pub fn control_finished(&mut self) {
        self.remember_release();
        self.control_path = None;
    }
}
impl Drop for Pairing {
    fn drop(&mut self) {
        self.cancel_requests();
    }
}
