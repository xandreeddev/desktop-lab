//! Catalog coordination over the root's application aggregate and prepared asset map.
//! No filesystem operations, production selection, launch calls or compositor policy.
use crate::ports::{AssetPort, Images};
use lucent_api::*;
use lucent_domain::Application;
use lucent_domain::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
#[derive(Clone)]
pub enum Message {
    Observed(Result<Vec<Application>>),
    Prepared(u64, Result<Images>),
}
#[derive(Clone, Copy)]
pub enum Status {
    Loading,
    Ready,
    Unavailable,
}
pub struct Catalog {
    assets: Arc<dyn AssetPort>,
    ready: bool,
    error: String,
    asset_error: String,
    revision: u64,
    active: Option<u64>,
    pending: Option<Vec<Application>>,
    keys: BTreeSet<String>,
}
impl Catalog {
    pub fn new(assets: Arc<dyn AssetPort>) -> Self {
        Self {
            assets,
            ready: false,
            error: String::new(),
            asset_error: String::new(),
            revision: 0,
            active: None,
            pending: None,
            keys: BTreeSet::new(),
        }
    }
    pub fn status(&self) -> Status {
        if !self.error.is_empty() {
            Status::Unavailable
        } else if self.ready {
            Status::Ready
        } else {
            Status::Loading
        }
    }
    pub fn error(&self) -> Option<&str> {
        if !self.error.is_empty() {
            Some(&self.error)
        } else {
            (!self.asset_error.is_empty()).then_some(&self.asset_error)
        }
    }
    fn prepare(&mut self, apps: Vec<Application>, effects: &mut Effects<Message>) {
        self.revision = self.revision.wrapping_add(1);
        if self.active.is_some() {
            self.pending = Some(apps);
            return;
        }
        let revision = self.revision;
        self.active = Some(revision);
        let assets = self.assets.clone();
        effects.task_on_with_failure(
            crate::work::ASSETS,
            move || Message::Prepared(revision, Ok(assets.application_icons(&apps))),
            move |failure| {
                Message::Prepared(revision, Err(DomainError::Failed(failure.to_string())))
            },
        );
    }
    /// Return true when applications changed; the root then reconciles launcher/dock geometry.
    pub fn update(
        &mut self,
        message: Message,
        apps: &mut Vec<Application>,
        images: &mut BTreeMap<String, Arc<ImageData>>,
        effects: &mut Effects<Message>,
    ) -> bool {
        match message {
            Message::Observed(Ok(next)) => {
                effects.redraw("dock");
                let recovered = !self.ready || !self.error.is_empty();
                self.ready = true;
                self.error.clear();
                if *apps != next {
                    *apps = next;
                    self.prepare(apps.clone(), effects);
                    return true;
                } else if recovered {
                    self.prepare(apps.clone(), effects);
                }
            }
            Message::Observed(Err(error)) => {
                effects.redraw("dock");
                self.error = error.to_string();
                self.revision = self.revision.wrapping_add(1);
                self.pending = None;
            }
            Message::Prepared(revision, next) if self.active == Some(revision) => {
                self.active = None;
                if revision == self.revision && self.error.is_empty() {
                    match next {
                        Ok(next) => {
                            for key in &self.keys {
                                images.remove(key);
                            }
                            self.keys = next.iter().map(|(key, _)| key.clone()).collect();
                            images.extend(next);
                            self.asset_error.clear();
                            effects.redraw("dock");
                            effects.redraw("widgets");
                        }
                        Err(error) => {
                            self.asset_error = error.to_string();
                            effects.redraw("dock");
                        }
                    }
                }
                if let Some(next) = self.pending.take() {
                    self.prepare(next, effects);
                }
            }
            Message::Prepared(..) => (),
        }
        false
    }
    pub fn inspect(&self) -> serde_json::Value {
        serde_json::json!({"state":match self.status(){Status::Loading=>"loading",Status::Ready=>"ready",Status::Unavailable=>"unavailable"},"preparing_icons":self.active.is_some(),"pending_icons":self.pending.is_some(),"icon_keys":self.keys.len()})
    }
}
#[cfg(test)]
mod tests;
