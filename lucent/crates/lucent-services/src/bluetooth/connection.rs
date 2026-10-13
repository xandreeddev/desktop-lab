//! Transport-scoped connection ownership and terminal operation state.
use super::{
    catalog::{Catalog, Kind},
    native::unavailable,
    *,
};
use zbus::zvariant::OwnedObjectPath;

#[derive(Default)]
pub(super) struct Connection {
    pub attempt: Option<BluetoothConnectionAttempt>,
    pub path: Option<OwnedObjectPath>,
    cancellation_pending: bool,
    cancellation_accepted: bool,
    outcome: Option<Result<()>>,
}

impl Connection {
    pub fn start(
        &mut self,
        id: BluetoothConnectionId,
        device: BluetoothObjectId,
        path: OwnedObjectPath,
    ) {
        self.cancellation_pending = false;
        self.cancellation_accepted = false;
        self.outcome = None;
        self.path = Some(path);
        self.attempt = Some(BluetoothConnectionAttempt {
            id,
            device,
            phase: BluetoothConnectionPhase::Connecting,
        });
    }
    pub fn synchronize(&self, catalog: &Catalog) -> Result<()> {
        if let Some(path) = &self.path {
            let device = self.attempt.as_ref().expect("active connection").device;
            if catalog.resolve(device, Kind::Device).ok().as_ref() != Some(path) {
                return Err(unavailable("Bluetooth connection target disappeared"));
            }
        }
        Ok(())
    }
    pub fn cancel(
        &mut self,
        id: BluetoothConnectionId,
        catalog: &Catalog,
    ) -> Result<OwnedObjectPath> {
        let attempt = self
            .attempt
            .as_ref()
            .filter(|a| a.id == id)
            .ok_or_else(|| DomainError::Invalid("Choose the current connection attempt".into()))?;
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| DomainError::Invalid("Bluetooth connection already ended".into()))?;
        if catalog.resolve(attempt.device, Kind::Device).ok().as_ref() != Some(path) {
            return Err(DomainError::Invalid(
                "Bluetooth connection target retired".into(),
            ));
        }
        if self.outcome.is_some() || self.cancellation_pending {
            return Err(DomainError::Invalid(
                "Bluetooth connection already ended or is cancelling".into(),
            ));
        }
        self.cancellation_pending = true;
        Ok(path.clone())
    }
    pub fn finished(&mut self, id: BluetoothConnectionId, result: Result<()>) {
        if self.attempt.as_ref().is_some_and(|a| a.id == id) && self.path.is_some() {
            self.outcome = Some(result);
            self.settle();
        }
    }
    pub fn cancellation_finished(&mut self, id: BluetoothConnectionId, accepted: bool) {
        if self.attempt.as_ref().is_some_and(|a| a.id == id) && self.cancellation_pending {
            self.cancellation_pending = false;
            self.cancellation_accepted = accepted;
            self.settle();
        }
    }
    fn settle(&mut self) {
        if !self.cancellation_pending
            && let Some(result) = self.outcome.take()
        {
            self.attempt.as_mut().expect("owned connection").phase = match result {
                Ok(()) => BluetoothConnectionPhase::Succeeded,
                Err(_) if self.cancellation_accepted => BluetoothConnectionPhase::Cancelled,
                Err(error) => BluetoothConnectionPhase::Failed(error.to_string()),
            };
            self.path = None;
        }
    }
    pub fn publish(&self, state: &mut BluetoothSnapshot) {
        state.connection = self.attempt.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_waits_for_both_native_replies_in_either_order() {
        for cancel_first in [false, true] {
            for accepted in [false, true] {
                for connected in [false, true] {
                    let id = BluetoothConnectionId::new(1).unwrap();
                    let mut connection = Connection::default();
                    connection.start(
                        id,
                        BluetoothObjectId::new(2).unwrap(),
                        "/device".try_into().unwrap(),
                    );
                    connection.cancellation_pending = true;
                    let outcome = if connected {
                        Ok(())
                    } else {
                        Err(DomainError::Failed("Native connection ended".into()))
                    };
                    if cancel_first {
                        connection.cancellation_finished(id, accepted);
                    } else {
                        connection.finished(id, outcome.clone());
                    }
                    assert_eq!(
                        connection.attempt.as_ref().unwrap().phase,
                        BluetoothConnectionPhase::Connecting
                    );
                    assert!(
                        connection.path.is_some(),
                        "Retain ownership until both replies are known"
                    );
                    if cancel_first {
                        connection.finished(id, outcome);
                    } else {
                        connection.cancellation_finished(id, accepted);
                    }
                    let phase = &connection.attempt.as_ref().unwrap().phase;
                    if connected {
                        assert_eq!(
                            *phase,
                            BluetoothConnectionPhase::Succeeded,
                            "Cancellation cannot erase a successful Connect reply"
                        );
                    } else if accepted {
                        assert_eq!(*phase, BluetoothConnectionPhase::Cancelled);
                    } else {
                        assert!(
                            matches!(phase, BluetoothConnectionPhase::Failed(_)),
                            "Denied cancellation is not reported as cancelled"
                        );
                    }
                    assert!(connection.path.is_none());
                }
            }
        }
    }

    #[test]
    fn retired_completion_cannot_mutate_a_replacement_attempt() {
        let mut connection = Connection::default();
        let current = BluetoothConnectionId::new(2).unwrap();
        let old = BluetoothConnectionId::new(1).unwrap();
        connection.start(
            current,
            BluetoothObjectId::new(3).unwrap(),
            "/device".try_into().unwrap(),
        );
        connection.cancellation_pending = true;
        connection.finished(old, Ok(()));
        connection.cancellation_finished(old, true);
        assert!(connection.outcome.is_none());
        assert!(connection.cancellation_pending);
        assert_eq!(
            connection.attempt.unwrap().phase,
            BluetoothConnectionPhase::Connecting
        );
    }
}
