//! Client presentation ownership for the bar shared by exclusive panels.
//! Submission acknowledges a prepared buffer, not compositor presentation.
use lucent_api::{Event, OutputName, SurfaceInstance};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Carrier {
    pub role: &'static str,
    pub generation: u64,
    pub instance: Option<SurfaceInstance>,
    pub output: Option<OutputName>,
}

impl Carrier {
    fn panel(&self, role: &'static str) -> Self {
        Self {
            role,
            ..self.clone()
        }
    }
    fn native_instance(&self) -> Option<&SurfaceInstance> {
        if self.role == "bar" {
            self.instance.as_ref()
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub struct Receipt {
    target: Carrier,
    requested_at: f64,
    revision: u64,
    frame_at: f64,
}
struct Transfer {
    owner: Carrier,
    target: Carrier,
    requested_at: f64,
    revision: Option<u64>,
}
#[derive(Default)]
pub(super) struct Presentation {
    displays: BTreeMap<Option<SurfaceInstance>, Transfer>,
    revision: u64,
}
impl Presentation {
    pub fn reconcile(
        &mut self,
        displays: Vec<Carrier>,
        active: Option<&SurfaceInstance>,
        panel: Option<&'static str>,
        visible: bool,
        now: f64,
    ) -> bool {
        if !visible {
            let changed = !self.displays.is_empty();
            self.displays.clear();
            return changed;
        }
        let mut changed = false;
        self.displays.retain(|instance, _| {
            let keep = displays.iter().any(|display| &display.instance == instance);
            changed |= !keep;
            keep
        });
        fn next(revision: &mut u64) -> Option<u64> {
            let value = revision.checked_add(1)?;
            *revision = value;
            Some(value)
        }
        for display in displays {
            let target = panel
                .filter(|_| display.instance.as_ref() == active)
                .map_or_else(|| display.clone(), |role| display.panel(role));
            let revision = &mut self.revision;
            let transfer = self
                .displays
                .entry(display.instance.clone())
                .or_insert_with(|| {
                    changed = true;
                    Transfer {
                        owner: display.clone(),
                        target: target.clone(),
                        requested_at: now,
                        revision: next(revision),
                    }
                });
            if transfer.owner.generation != display.generation
                || transfer.owner.output != display.output
            {
                // An output name/instance can survive replacement of its native
                // protocol object. A new host starts with its own reserving bar.
                transfer.owner = display;
                transfer.target = target;
                transfer.requested_at = now;
                transfer.revision = next(revision);
                changed = true;
                continue;
            }
            if transfer.target != target {
                transfer.target = target;
                transfer.requested_at = now;
                transfer.revision = next(revision);
                changed = true;
            }
        }
        changed
    }
    pub fn standalone(&self, instance: Option<&SurfaceInstance>) -> bool {
        self.displays
            .get(&instance.cloned())
            .is_none_or(|transfer| transfer.owner.role == "bar" || transfer.target.role == "bar")
    }
    pub fn retained(&self, role: &str) -> Option<&Carrier> {
        self.displays.values().find_map(|transfer| {
            (transfer.owner.role == role && role != "bar" && transfer.owner != transfer.target)
                .then_some(&transfer.owner)
        })
    }
    pub fn incoming(&self, role: &str) -> Option<&Carrier> {
        // A panel moving between outputs keeps its old native carrier passive
        // until that output's reserving bar has submitted. One native address
        // cannot simultaneously present the old output and the new one.
        if self.retained(role).is_some() {
            return None;
        }
        self.displays.values().find_map(|transfer| {
            (role != "bar" && transfer.target.role == role).then_some(&transfer.target)
        })
    }
    pub fn receipt(&self, event: &Event) -> Option<Receipt> {
        let (instance, event) = match event {
            Event::Scoped { instance, event } => (Some(instance), event.as_ref()),
            event => (None, event),
        };
        let Event::FrameSubmitted { surface, now, .. } = event else {
            return None;
        };
        let transfer = self.displays.values().find(|transfer| {
            transfer.owner != transfer.target
                && transfer.target.role == *surface
                && transfer.target.native_instance() == instance
                && *now >= transfer.requested_at
                && (*surface == "bar" || self.retained(surface).is_none())
        })?;
        Some(Receipt {
            target: transfer.target.clone(),
            requested_at: transfer.requested_at,
            revision: transfer.revision?,
            frame_at: *now,
        })
    }
    pub fn submitted(&mut self, receipt: Receipt) -> bool {
        if receipt.target.role != "bar" && self.retained(receipt.target.role).is_some() {
            return false;
        }
        let Some(transfer) = self.displays.get_mut(&receipt.target.instance) else {
            return false;
        };
        if transfer.target != receipt.target
            || transfer.requested_at != receipt.requested_at
            || transfer.revision != Some(receipt.revision)
            || receipt.frame_at < transfer.requested_at
            || transfer.owner == transfer.target
        {
            return false;
        }
        transfer.owner = transfer.target.clone();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn display(name: Option<&str>) -> Carrier {
        Carrier {
            role: "bar",
            generation: 1,
            instance: name.map(|name| SurfaceInstance::new(name).unwrap()),
            output: name.map(|name| OutputName::new(name).unwrap()),
        }
    }
    fn sync(p: &mut Presentation, panel: Option<&'static str>, now: f64) {
        p.reconcile(vec![display(None)], None, panel, true, now);
    }
    fn frame(role: &'static str, instance: Option<&str>, now: f64) -> Event {
        let event = Event::FrameSubmitted {
            surface: role,
            width: 1920.,
            height: 1080.,
            now,
        };
        instance.map_or(event.clone(), |name| Event::Scoped {
            instance: SurfaceInstance::new(name).unwrap(),
            event: Box::new(event),
        })
    }
    fn submit(p: &mut Presentation, role: &'static str, instance: Option<&str>, now: f64) {
        let receipt = p
            .receipt(&frame(role, instance, now))
            .expect("pending matching carrier");
        assert!(p.submitted(receipt));
    }
    fn opened() -> Presentation {
        let mut p = Presentation::default();
        sync(&mut p, Some("audio"), 1.);
        submit(&mut p, "audio", None, 2.);
        p
    }
    #[test]
    fn opening_keeps_reserving_bar_until_incoming_submission() {
        let mut p = Presentation::default();
        sync(&mut p, Some("audio"), 1.);
        assert!(p.standalone(None));
        assert!(p.incoming("audio").is_some());
        submit(&mut p, "audio", None, 2.);
        assert!(!p.standalone(None));
    }
    #[test]
    fn unrelated_submission_does_not_clear_the_bar() {
        let mut p = Presentation::default();
        sync(&mut p, Some("audio"), 1.);
        assert!(p.receipt(&frame("network", None, 2.)).is_none());
        assert!(p.standalone(None));
    }
    #[test]
    fn frame_started_before_request_cannot_authorize_handoff() {
        let mut p = Presentation::default();
        sync(&mut p, Some("audio"), 3.);
        assert!(p.receipt(&frame("audio", None, 2.)).is_none());
        assert!(p.standalone(None));
    }
    #[test]
    fn closing_retains_old_panel_until_standalone_submission() {
        let mut p = opened();
        sync(&mut p, None, 3.);
        assert!(p.standalone(None));
        assert!(p.retained("audio").is_some());
        submit(&mut p, "bar", None, 4.);
        assert!(p.retained("audio").is_none());
    }
    #[test]
    fn switching_panels_retains_only_previous_owner() {
        let mut p = opened();
        sync(&mut p, Some("network"), 3.);
        assert!(p.retained("audio").is_some());
        assert!(p.incoming("network").is_some());
        assert!(!p.standalone(None));
        submit(&mut p, "network", None, 4.);
        assert!(p.retained("audio").is_none());
    }
    #[test]
    fn rapid_replacement_rejects_the_old_receipt() {
        let mut p = opened();
        sync(&mut p, Some("network"), 3.);
        let stale = p.receipt(&frame("network", None, 4.)).unwrap();
        sync(&mut p, Some("power"), 5.);
        assert!(!p.submitted(stale));
        assert!(p.retained("audio").is_some());
        assert!(p.retained("network").is_none());
        submit(&mut p, "power", None, 6.);
        assert!(p.retained("audio").is_none());
    }
    #[test]
    fn same_timestamp_superseding_request_still_retires_old_receipt() {
        let mut p = opened();
        sync(&mut p, Some("network"), 3.);
        let stale = p.receipt(&frame("network", None, 4.)).unwrap();
        sync(&mut p, Some("power"), 3.);
        sync(&mut p, Some("network"), 3.);
        assert!(!p.submitted(stale));
        submit(&mut p, "network", None, 4.);
    }
    #[test]
    fn reopen_before_standalone_submission_reuses_previous_owner() {
        let mut p = opened();
        sync(&mut p, None, 3.);
        let stale = p.receipt(&frame("bar", None, 4.)).unwrap();
        sync(&mut p, Some("audio"), 5.);
        assert!(!p.submitted(stale));
        assert!(p.retained("audio").is_none());
        assert!(p.incoming("audio").is_some());
        assert!(!p.standalone(None));
    }
    #[test]
    fn hide_retires_all_carriers_and_receipts() {
        let mut p = opened();
        sync(&mut p, None, 3.);
        let stale = p.receipt(&frame("bar", None, 4.)).unwrap();
        assert!(p.reconcile(vec![display(None)], None, None, false, 5.));
        assert!(!p.submitted(stale));
        assert!(p.retained("audio").is_none());
        assert!(p.incoming("audio").is_none());
    }
    #[test]
    fn other_output_keeps_its_own_bar() {
        let mut p = Presentation::default();
        let left = display(Some("LEFT"));
        let right = display(Some("RIGHT"));
        p.reconcile(
            vec![left.clone(), right.clone()],
            left.instance.as_ref(),
            Some("audio"),
            true,
            1.,
        );
        submit(&mut p, "audio", None, 2.);
        assert!(!p.standalone(left.instance.as_ref()));
        assert!(p.standalone(right.instance.as_ref()));
    }
    #[test]
    fn retarget_returns_old_output_bar_before_moving_panel_native() {
        let mut p = Presentation::default();
        let left = display(Some("LEFT"));
        let right = display(Some("RIGHT"));
        p.reconcile(
            vec![left.clone(), right.clone()],
            left.instance.as_ref(),
            Some("audio"),
            true,
            1.,
        );
        submit(&mut p, "audio", None, 2.);
        p.reconcile(
            vec![left.clone(), right.clone()],
            right.instance.as_ref(),
            Some("audio"),
            true,
            3.,
        );
        assert_eq!(p.retained("audio"), Some(&left.panel("audio")));
        assert!(p.incoming("audio").is_none());
        assert!(p.receipt(&frame("audio", None, 4.)).is_none());
        submit(&mut p, "bar", Some("LEFT"), 4.);
        assert_eq!(p.incoming("audio"), Some(&right.panel("audio")));
        submit(&mut p, "audio", None, 5.);
        assert!(p.standalone(left.instance.as_ref()));
        assert!(!p.standalone(right.instance.as_ref()));
    }
    #[test]
    fn retired_output_does_not_accept_its_old_receipt() {
        let mut p = Presentation::default();
        let left = display(Some("LEFT"));
        p.reconcile(
            vec![left.clone()],
            left.instance.as_ref(),
            Some("audio"),
            true,
            1.,
        );
        let stale = p.receipt(&frame("audio", None, 2.)).unwrap();
        p.reconcile(vec![], None, None, true, 3.);
        assert!(!p.submitted(stale));
        assert!(p.retained("audio").is_none());
    }
    #[test]
    fn scoped_standalone_receipt_must_match_the_output_instance() {
        let mut p = Presentation::default();
        let left = display(Some("LEFT"));
        p.reconcile(
            vec![left.clone()],
            left.instance.as_ref(),
            Some("audio"),
            true,
            1.,
        );
        submit(&mut p, "audio", None, 2.);
        p.reconcile(vec![left.clone()], left.instance.as_ref(), None, true, 3.);
        assert!(p.receipt(&frame("bar", Some("RIGHT"), 4.)).is_none());
        assert!(p.receipt(&frame("bar", None, 4.)).is_none());
        submit(&mut p, "bar", Some("LEFT"), 4.);
    }
}
