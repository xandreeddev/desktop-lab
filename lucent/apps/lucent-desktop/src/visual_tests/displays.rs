//! Display snapshots use the actual child view and normal root foreground policy.
use super::*;
use crate::displays::{Message as D, tests::fixture};
use lucent_domain::{
    DisplayTransaction, DisplayTransactionId, DisplayTransactionPhase, DomainError,
};
pub(super) fn prepare(app: &mut Desktop, name: &str) {
    let mut effects = Effects::default();
    app.update(Message::Displays(D::Open), &mut effects);
    if name != "displays-loading" {
        let result = if name == "displays-unavailable" {
            Err(DomainError::Unavailable(
                "Display observation disconnected. Your existing settings are preserved.".into(),
            ))
        } else {
            let mut snapshot = fixture();
            if matches!(name, "displays-confirmation" | "displays-failed") {
                snapshot.transaction = Some(DisplayTransaction {
                    id: DisplayTransactionId::new(7).unwrap(),
                    phase: if name == "displays-confirmation" {
                        DisplayTransactionPhase::AwaitingConfirmation {
                            remaining_seconds: 12,
                        }
                    } else {
                        DisplayTransactionPhase::Failed{reason:"The display changed while previewing. Original settings could not be verified; recovery checkpoints are retained.".into()}
                    },
                });
            }
            Ok(snapshot)
        };
        app.update(Message::Displays(D::Observed(result)), &mut effects);
    }
    if name == "displays-last" {
        app.update(
            Message::Displays(D::Scroll(ScrollEvent { offset: 10000. })),
            &mut effects,
        );
    }
}
#[test]
fn display_apply_and_confirmation_controls_remain_visible_on_narrow_panels() {
    for (width, height) in [(390., 580.), (640., 580.), (1920., 1080.)] {
        for name in [
            "displays-controls",
            "displays-confirmation",
            "displays-failed",
            "displays-last",
        ] {
            let mut app = super::fixture(0, false);
            resize_fixture(&mut app, width, height);
            prepare(&mut app, name);
            let layout = Layout::new(fonts(&app));
            let cx = ViewContext {
                surface: "displays",
                instance: None,
                width,
                height,
                now: 1.,
                density: Density::ONE,
            };
            let scene = layout.build(&app.view(&cx), width, height, &Interaction::default(), 1.);
            for id in ["display-close", "display-reset", "display-apply"]
                .into_iter()
                .chain(if name == "displays-confirmation" {
                    vec!["display-keep", "display-revert"]
                } else {
                    vec![]
                })
            {
                let hit = scene
                    .hits
                    .iter()
                    .find(|h| h.id == id)
                    .expect("persistent display action");
                assert!(
                    fully_visible(hit.rect, hit.clip.bounds),
                    "{name} {id} cropped at {width}×{height}"
                );
            }
            assert!(scene.hits.iter().any(|h| h.id == "display-scroll"));
            assert!(!app.launcher.open());
        }
    }
}
