//! Deferred surface-local overlays share the ordinary layout, hit and paint
//! engine. Tooltip content is paint-only; an explicit popover owns one focus scope.
use crate::{Hit, Interaction, Layout, Scene};
use lucent_api::*;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

#[derive(Clone, PartialEq)]
struct Anchor {
    rect: Rect,
    clip: Clip,
}
#[derive(Clone)]
struct TooltipTarget {
    id: String,
    anchor: Anchor,
    delay: f64,
    transition: Transition,
    opacity: f32,
}
struct PopoverHit<M> {
    id: String,
    rect: Rect,
    radius: f32,
    focus: BTreeSet<String>,
    dismiss: M,
}
struct Retired<M> {
    id: String,
    dismiss: M,
}
struct DeclaredPopover<'a, M> {
    spec: &'a Popover<M>,
    paint: bool,
    opacity: f32,
}
/// Prepared overlay metadata. The runtime uses its bounded deadline through
/// Interaction; application/domain identities never appear here.
pub struct OverlayScene<M> {
    requested: BTreeSet<String>,
    anchors: BTreeMap<String, Option<Anchor>>,
    tooltips: Vec<TooltipTarget>,
    popover: Option<PopoverHit<M>>,
    retired: Vec<Retired<M>>,
    motion: Option<Motion>,
}
impl<M> Default for OverlayScene<M> {
    fn default() -> Self {
        Self {
            requested: BTreeSet::new(),
            anchors: BTreeMap::new(),
            tooltips: vec![],
            popover: None,
            retired: vec![],
            motion: None,
        }
    }
}
impl<M> OverlayScene<M> {
    pub fn has_popover(&self) -> bool {
        self.popover.is_some() || !self.retired.is_empty()
    }
    pub(super) fn anchor(&mut self, id: &str, rect: Rect, clip: &Clip) {
        if !self.requested.contains(id) {
            return;
        }
        let bounds = rect.intersect(clip.bounds);
        let valid = |rect: Rect| {
            [
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                rect.x + rect.w,
                rect.y + rect.h,
            ]
            .iter()
            .all(|v| v.is_finite())
                && rect.w > 0.
                && rect.h > 0.
        };
        let value = (valid(rect)
            && valid(clip.bounds)
            && valid(bounds)
            && crate::scroll::visible(clip, rect))
        .then(|| Anchor {
            rect,
            clip: clip.clone(),
        });
        use std::collections::btree_map::Entry;
        match self.anchors.entry(id.to_owned()) {
            Entry::Vacant(e) => {
                e.insert(value);
            }
            Entry::Occupied(mut e) => {
                e.insert(None);
            }
        }
    }
    pub(super) fn allows_focus(&self, id: &str) -> bool {
        self.popover.as_ref().is_none_or(|p| p.focus.contains(id))
    }
}
#[derive(Clone)]
pub(super) struct Hover {
    id: String,
    anchor: Anchor,
    since: f64,
}
struct Focus {
    id: String,
    opener: Option<String>,
}
pub(super) struct State<M> {
    pub hover: Option<Hover>,
    pub suppressed: Option<String>,
    pub motion: Option<Motion>,
    focus: Option<Focus>,
    rejected: BTreeSet<String>,
    dismissed: VecDeque<(String, M)>,
}
impl<M> Default for State<M> {
    fn default() -> Self {
        Self {
            hover: None,
            suppressed: None,
            motion: None,
            focus: None,
            rejected: BTreeSet::new(),
            dismissed: VecDeque::new(),
        }
    }
}
fn collect<'a, M>(
    e: &'a Element<M>,
    path: &str,
    tooltips: &mut Vec<(String, &'a Tooltip<M>, f32)>,
    popovers: &mut Vec<DeclaredPopover<'a, M>>,
    paint: bool,
    opacity: f32,
) {
    collect_at(e, &mut path.to_owned(), tooltips, popovers, paint, opacity);
}
fn collect_at<'a, M>(
    e: &'a Element<M>,
    path: &mut String,
    tooltips: &mut Vec<(String, &'a Tooltip<M>, f32)>,
    popovers: &mut Vec<DeclaredPopover<'a, M>>,
    paint: bool,
    opacity: f32,
) {
    use std::fmt::Write;
    let opacity = opacity * e.style.opacity;
    if !opacity.is_finite()
        || opacity <= 0.001
        || e.style
            .diagonal_reveal
            .is_some_and(|progress| !progress.is_finite() || progress <= 0.)
    {
        return;
    }
    // input_only retains input scopes but suppresses all deferred subtree paint.
    let paint = paint && e.style.paint;
    let id = if e.id.is_empty() {
        path.as_str()
    } else {
        e.id.as_str()
    };
    if paint && let Some(tooltip) = &e.tooltip {
        tooltips.push((id.to_owned(), tooltip, opacity));
    }
    popovers.extend(e.popovers.iter().map(|spec| DeclaredPopover {
        spec,
        paint,
        opacity,
    }));
    for (i, child) in e.children.iter().enumerate() {
        let length = path.len();
        write!(path, "/{i}").expect("writing a String cannot fail");
        collect_at(child, path, tooltips, popovers, paint, opacity);
        path.truncate(length);
    }
}
pub(super) struct Definitions<'a, M> {
    tips: Vec<(String, &'a Tooltip<M>, f32)>,
    popovers: Vec<DeclaredPopover<'a, M>>,
}
impl<'a, M> Definitions<'a, M> {
    pub fn new(root: &'a Element<M>) -> Self {
        let mut definitions = Self {
            tips: vec![],
            popovers: vec![],
        };
        collect(
            root,
            "root",
            &mut definitions.tips,
            &mut definitions.popovers,
            true,
            1.,
        );
        definitions
    }
    pub fn request(&self, scene: &mut OverlayScene<M>) {
        scene
            .requested
            .extend(self.tips.iter().map(|(id, _, _)| id.clone()));
        scene.requested.extend(
            self.popovers
                .iter()
                .map(|popover| popover.spec.anchor.clone()),
        );
    }
}
fn valid_tooltip<M>(tooltip: &Tooltip<M>) -> bool {
    tooltip.max_width.is_finite()
        && tooltip.max_width > 0.
        && tooltip.delay.is_finite()
        && tooltip.delay >= 0.
        && tooltip.transition.duration.is_finite()
        && tooltip.transition.duration >= 0.
        && tooltip
            .transition
            .curve
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
}
impl Layout {
    pub(super) fn compose_overlays<M: Clone>(
        &self,
        definitions: Definitions<'_, M>,
        viewport: Rect,
        scene: &mut Scene<M>,
        input: &Interaction<M>,
        now: f64,
    ) {
        let Definitions { mut tips, popovers } = definitions;
        // An explicit single scope avoids pretending nested menu semantics exist.
        if popovers.len() == 1 {
            let declared = &popovers[0];
            let popover = declared.spec;
            let anchor = scene
                .overlays
                .anchors
                .get(&popover.anchor)
                .and_then(Option::as_ref)
                .cloned();
            let available = (
                viewport.w - 2. * popover.placement.inset,
                viewport.h - 2. * popover.placement.inset,
            );
            let bounds = anchor.as_ref().and_then(|anchor| {
                let anchor = anchor.rect.intersect(anchor.clip.bounds);
                popover.placement.place(anchor, (1., 1.), viewport)?;
                popover
                    .placement
                    .place(anchor, self.measure(&popover.content, available), viewport)
            });
            if let Some(rect) = bounds.filter(|_| !popover.id.is_empty()) {
                let paint_start = scene.paint.len();
                let hit_start = scene.hits.len();
                let region_start = scene.regions.len();
                let backdrop = Element::empty()
                    .on_click(popover.dismiss.clone())
                    .tab_stop(false)
                    .id(format!("{}/backdrop", popover.id));
                let focus_clips = Arc::from([crate::FocusClip {
                    rect: viewport,
                    radius: 0.,
                    diagonal_reveal: None,
                    scroll_depth: 0,
                }]);
                self.place(
                    &backdrop,
                    viewport,
                    viewport.into(),
                    1.,
                    scene,
                    input,
                    now,
                    &Arc::default(),
                    &focus_clips,
                    "popover-backdrop",
                );
                // The panel blocks backdrop activation through non-interactive gaps.
                let mut blocker = scene.hits.last().expect("interactive backdrop").clone();
                blocker.id = format!("{}/panel", popover.id);
                blocker.rect = rect;
                blocker.clip = rect.into();
                blocker.radius = popover.content.style.radius;
                blocker.click = None;
                scene.hits.push(blocker);
                let content_start = scene.hits.len();
                let path = format!("popover/{}", popover.id);
                let mut nested = vec![];
                let mut popup_tips = vec![];
                collect(
                    &popover.content,
                    &path,
                    &mut popup_tips,
                    &mut nested,
                    declared.paint,
                    declared.opacity,
                );
                scene
                    .overlays
                    .requested
                    .extend(popup_tips.iter().map(|(id, _, _)| id.clone()));
                let mut content = (*popover.content).clone();
                content.style.clip = true;
                content.style.paint &= declared.paint;
                self.place(
                    &content,
                    rect,
                    viewport.into(),
                    declared.opacity,
                    scene,
                    input,
                    now,
                    &Arc::default(),
                    &focus_clips,
                    &path,
                );
                let ids: BTreeSet<_> = scene.hits[content_start..]
                    .iter()
                    .map(|h| h.id.clone())
                    .collect();
                let scope_ids: BTreeSet<_> = scene.hits[hit_start..]
                    .iter()
                    .map(|h| h.id.clone())
                    .collect();
                let valid = scope_ids.len() == scene.hits.len() - hit_start
                    && scene
                        .overlays
                        .anchors
                        .get(&popover.anchor)
                        .and_then(Option::as_ref)
                        == anchor.as_ref()
                    && !scene.hits[..hit_start]
                        .iter()
                        .any(|hit| scope_ids.contains(&hit.id));
                if valid {
                    scene.overlays.popover = Some(PopoverHit {
                        id: popover.id.clone(),
                        rect,
                        radius: popover.content.style.radius,
                        focus: ids,
                        dismiss: popover.dismiss.clone(),
                    });
                    tips.extend(popup_tips);
                    scene
                        .overlays
                        .retired
                        .extend(nested.into_iter().map(|p| Retired {
                            id: p.spec.id.clone(),
                            dismiss: p.spec.dismiss.clone(),
                        }));
                } else {
                    scene.paint.truncate(paint_start);
                    scene.hits.truncate(hit_start);
                    scene.regions.truncate(region_start);
                    scene.overlays.retired.push(Retired {
                        id: popover.id.clone(),
                        dismiss: popover.dismiss.clone(),
                    });
                }
            } else {
                scene.overlays.retired.push(Retired {
                    id: popover.id.clone(),
                    dismiss: popover.dismiss.clone(),
                });
            }
        } else {
            scene
                .overlays
                .retired
                .extend(popovers.into_iter().map(|p| Retired {
                    id: p.spec.id.clone(),
                    dismiss: p.spec.dismiss.clone(),
                }));
        }
        for (id, tooltip, opacity) in &tips {
            if valid_tooltip(tooltip)
                && let Some(anchor) = scene.overlays.anchors.get(id).and_then(Option::as_ref)
                && tooltip
                    .placement
                    .place(
                        anchor.rect.intersect(anchor.clip.bounds),
                        (1., 1.),
                        viewport,
                    )
                    .is_some()
            {
                scene.overlays.tooltips.push(TooltipTarget {
                    id: id.clone(),
                    anchor: anchor.clone(),
                    delay: tooltip.delay,
                    transition: tooltip.transition,
                    opacity: *opacity,
                });
            }
        }
        let Some((target, alpha, motion)) = input.tooltip(scene, now) else {
            return;
        };
        let Some((_, tooltip, _)) = tips.iter().find(|(id, _, _)| id == &target.id) else {
            return;
        };
        let mut size = self.measure(
            &tooltip.content,
            (
                (viewport.w - 2. * tooltip.placement.inset).min(tooltip.max_width),
                viewport.h - 2. * tooltip.placement.inset,
            ),
        );
        size.0 = size.0.min(tooltip.max_width);
        let Some(rect) = tooltip.placement.place(
            target.anchor.rect.intersect(target.anchor.clip.bounds),
            size,
            viewport,
        ) else {
            return;
        };
        scene.overlays.motion = motion;
        let mut passive = Scene::default();
        let mut content = (*tooltip.content).clone();
        content.style.clip = true;
        self.place(
            &content,
            rect,
            viewport.into(),
            alpha,
            &mut passive,
            input,
            now,
            &Arc::default(),
            &Arc::default(),
            "tooltip",
        );
        // Deliberately discard hits, focus, native input regions and nested scopes.
        scene.paint.extend(passive.paint);
    }
}
impl<M: Clone> Interaction<M> {
    fn tooltip_target<'a>(&self, scene: &'a Scene<M>) -> Option<(&'a TooltipTarget, bool)> {
        let keyboard = self.focus_visible
            && self
                .focus
                .as_ref()
                .is_some_and(|id| scene.overlays.tooltips.iter().any(|t| &t.id == id));
        let id = if keyboard {
            self.focus.as_ref()
        } else {
            self.hovered.as_ref()
        }?;
        if self.overlays.suppressed.as_ref() == Some(id) || !scene.overlays.allows_focus(id) {
            return None;
        }
        let target = scene.overlays.tooltips.iter().find(|t| &t.id == id)?;
        if !keyboard {
            let hover = self.overlays.hover.as_ref()?;
            if hover.id != target.id
                || hover.anchor.rect != target.anchor.rect
                || hover.anchor.clip != target.anchor.clip
            {
                return None;
            }
        }
        Some((target, keyboard))
    }
    fn tooltip<'a>(
        &self,
        scene: &'a Scene<M>,
        now: f64,
    ) -> Option<(&'a TooltipTarget, f32, Option<Motion>)> {
        let (target, keyboard) = self.tooltip_target(scene)?;
        if keyboard {
            return Some((target, target.opacity, None));
        }
        let due = self.overlays.hover.as_ref()?.since + target.delay;
        if !now.is_finite()
            || !due.is_finite()
            || !(due + target.transition.duration).is_finite()
            || now < due
        {
            return None;
        }
        if target.transition.duration == 0. {
            return Some((target, target.opacity, None));
        }
        let mut motion = Motion::fixed(0.);
        motion.target(1., due, target.transition.duration, target.transition.curve);
        Some((target, motion.value(now) * target.opacity, Some(motion)))
    }
    /// A single native event-loop timer can sleep until this absolute runtime
    /// timestamp. Dwell does not request animation frames or a polling worker.
    pub fn next_wakeup(&self, scene: &Scene<M>, now: f64) -> Option<f64> {
        let (target, keyboard) = self.tooltip_target(scene)?;
        if keyboard || !target.delay.is_finite() || target.delay < 0. {
            return None;
        }
        let due = self.overlays.hover.as_ref()?.since + target.delay;
        (now.is_finite()
            && due.is_finite()
            && (due + target.transition.duration).is_finite()
            && due > now)
            .then_some(due)
    }
    pub(super) fn observe_tooltip_hover(&mut self, hit: Option<&Hit<M>>, now: f64) {
        let compatible = hit
            .zip(self.overlays.hover.as_ref())
            .is_some_and(|(h, old)| {
                old.id == h.id && old.anchor.rect == h.rect && old.anchor.clip == h.clip
            });
        if !compatible {
            self.overlays.suppressed = None;
            self.overlays.hover = hit.filter(|_| now.is_finite()).map(|h| Hover {
                id: h.id.clone(),
                anchor: Anchor {
                    rect: h.rect,
                    clip: h.clip.clone(),
                },
                since: now,
            });
        }
    }
    pub(super) fn synchronize_overlays(&mut self, scene: &Scene<M>) {
        self.overlays.motion = scene.overlays.motion;
        if let Some(popover) = &scene.overlays.popover {
            if self
                .overlays
                .focus
                .as_ref()
                .is_none_or(|focus| focus.id != popover.id)
            {
                let opener = self
                    .overlays
                    .focus
                    .take()
                    .and_then(|f| f.opener)
                    .or_else(|| self.focus.clone());
                self.overlays.focus = Some(Focus {
                    id: popover.id.clone(),
                    opener,
                });
                self.press = None;
                self.alternate_press = None;
                self.select_all = false;
                self.input_buffer = None;
                self.slider_buffer = None;
            }
            // Ordinary primary buttons otherwise retain a cloned old hit. A
            // stable scope ID cannot authorize a removed/moved content action.
            if let Some(press) = &mut self.press {
                let current = scene.hits.iter().find(|hit| {
                    popover.focus.contains(&hit.id)
                        && hit.id == press.hit.id
                        && hit.rect == press.hit.rect
                        && hit.clip == press.hit.clip
                        && hit.radius == press.hit.radius
                        && hit.parts == press.hit.parts
                });
                if let Some(hit) = current {
                    press.hit = hit.clone();
                } else {
                    self.press = None;
                }
            }
            let reachable = |hit: &Hit<M>| {
                hit.tab_stop
                    && popover.focus.contains(&hit.id)
                    && crate::scroll::reveal(hit, &self.scroll_buffer)
                        .is_some_and(|plan| plan.is_empty())
            };
            if self
                .focus
                .as_ref()
                .is_none_or(|id| !scene.hits.iter().any(|hit| &hit.id == id && reachable(hit)))
            {
                self.focus = scene
                    .hits
                    .iter()
                    .find(|hit| reachable(hit))
                    .map(|h| h.id.clone());
                self.focus_visible = true;
                self.select_all = false;
                self.input_buffer = None;
                self.slider_buffer = None;
            }
        } else if let Some(previous) = self.overlays.focus.take() {
            self.focus = previous.opener.filter(|id| {
                scene.hits.iter().any(|h| {
                    &h.id == id
                        && h.tab_stop
                        && crate::scroll::reveal(h, &self.scroll_buffer)
                            .is_some_and(|plan| plan.is_empty())
                })
            });
            self.focus_visible = self.focus.is_some();
            self.press = None;
            self.alternate_press = None;
            self.select_all = false;
            self.input_buffer = None;
            self.slider_buffer = None;
        }
        let rejected: BTreeSet<_> = scene
            .overlays
            .retired
            .iter()
            .map(|scope| scope.id.clone())
            .collect();
        self.overlays
            .dismissed
            .retain(|(id, _)| rejected.contains(id));
        let mut admitted = BTreeSet::new();
        for scope in &scene.overlays.retired {
            if !self.overlays.rejected.contains(&scope.id) && admitted.insert(scope.id.clone()) {
                self.overlays
                    .dismissed
                    .push_back((scope.id.clone(), scope.dismiss.clone()));
            }
        }
        self.overlays.rejected = rejected;
    }
    /// Drain a rejected/retired declarative scope once after synchronization.
    pub fn take_overlay_dismissal(&mut self) -> Option<M> {
        self.overlays
            .dismissed
            .pop_front()
            .map(|(_, message)| message)
    }
    /// Runtime offers the active scope input before application-global keys.
    /// Escape dismisses; ordinary controls receive the other keys with scoped Tab.
    pub fn popover_key(&mut self, scene: &Scene<M>, key: &Key) -> Option<Vec<M>> {
        let popover = scene.overlays.popover.as_ref()?;
        if key.unmodified() == &Key::Escape {
            self.suppress_tooltip_key(scene, key);
            Some(vec![popover.dismiss.clone()])
        } else {
            Some(self.key_controls(scene, key))
        }
    }
    pub(super) fn suppress_tooltip_key(&mut self, scene: &Scene<M>, key: &Key) {
        self.overlays.suppressed = if key.unmodified() == &Key::Escape {
            self.tooltip_target(scene)
                .map(|(target, _)| target.id.clone())
        } else {
            None
        };
    }
    pub(super) fn popover_outside_press(&mut self, scene: &Scene<M>, x: f32, y: f32) -> Option<M> {
        let p = scene.overlays.popover.as_ref()?;
        if RoundedRect::new(p.rect, p.radius).contains(x, y) {
            return None;
        }
        self.press = None;
        self.alternate_press = None;
        Some(p.dismiss.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Paint;
    #[derive(Clone, Debug, PartialEq)]
    enum Message {
        Open,
        Close,
        First,
        Second,
        Under,
        Input(String),
    }
    fn layout() -> Layout {
        Layout::new(Arc::new(vec![
            fontdue::Font::from_bytes(
                include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
                fontdue::FontSettings::default(),
            )
            .unwrap(),
        ]))
    }
    fn placement() -> AnchoredSpec {
        AnchoredSpec {
            side: OverlaySide::Below,
            align: OverlayAlign::Center,
            gap: 8.,
            inset: 8.,
        }
    }
    fn tooltip() -> Tooltip<Message> {
        Tooltip {
            content: Box::new(
                Element::text("App name")
                    .font(12.)
                    .padding(8.)
                    .background(Color::hex(0x123456)),
            ),
            placement: placement(),
            max_width: 120.,
            delay: 0.5,
            transition: Transition {
                duration: 0.1,
                curve: [0., 0., 1., 1.],
            },
        }
    }
    fn tree(open: bool) -> Element<Message> {
        let root = Element::stack(vec![
            Element::empty()
                .size(30., 30.)
                .on_click(Message::Under)
                .id("under"),
            Element::empty()
                .size(60., 32.)
                .at(100., 30.)
                .on_click(Message::Open)
                .id("trigger")
                .tooltip(tooltip()),
            Element::empty()
                .size(20., 20.)
                .at(200., 10.)
                .on_click(Message::Under)
                .id("plain"),
        ]);
        if !open {
            return root;
        }
        root.popover(Popover {
            id: "menu".into(),
            anchor: "trigger".into(),
            placement: placement(),
            dismiss: Message::Close,
            content: Box::new(
                Element::column(vec![
                    Element::text("First")
                        .font(12.)
                        .on_click(Message::First)
                        .id("first"),
                    Element::text("Second")
                        .font(12.)
                        .on_click(Message::Second)
                        .id("second"),
                ])
                .gap(8.)
                .padding(12.)
                .size(110., 90.)
                .radius(12.)
                .background(Color::hex(0x123456)),
            ),
        })
    }
    fn scene(tree: &Element<Message>, input: &Interaction<Message>, now: f64) -> Scene<Message> {
        layout().build(tree, 300., 220., input, now)
    }
    fn has_tip(scene: &Scene<Message>) -> bool {
        scene
            .paint
            .iter()
            .any(|p| matches!(p,Paint::Text{text,..} if text=="App name"))
    }
    #[test]
    fn tooltip_dwell_sleeps_then_paints_without_adding_input_or_focus() {
        let tree = tree(false);
        let mut input = Interaction::default();
        let initial = scene(&tree, &input, 0.);
        input.motion(&initial, 120., 45., 0.);
        assert_eq!(input.next_wakeup(&initial, 0.), Some(0.5));
        assert!(!input.animating(0.3));
        assert!(!has_tip(&scene(&tree, &input, 0.49)));
        let visible = scene(&tree, &input, 0.55);
        assert!(has_tip(&visible));
        assert_eq!(initial.regions, visible.regions);
        assert_eq!(
            initial.hits.iter().map(|h| &h.id).collect::<Vec<_>>(),
            visible.hits.iter().map(|h| &h.id).collect::<Vec<_>>()
        );
        assert!(input.focus.is_none());
        input.synchronize(&visible);
        assert!(input.animating(0.55));
        let settled = scene(&tree, &input, 0.7);
        input.synchronize(&settled);
        assert!(!input.animating(0.7));
        assert_eq!(input.next_wakeup(&settled, 0.7), None);
        input.leave(0.8);
        assert!(!has_tip(&scene(&tree, &input, 1.)));
    }
    #[test]
    fn invalid_tooltip_policy_never_schedules_or_paints() {
        for case in 0..8 {
            let mut root = tree(false);
            let tooltip = root.children[1].tooltip.as_mut().unwrap();
            match case {
                0 => tooltip.delay = f64::NAN,
                1 => tooltip.delay = -0.1,
                2 => tooltip.transition.duration = f64::INFINITY,
                3 => tooltip.transition.duration = -1.,
                4 => tooltip.transition.curve[1] = f32::NAN,
                5 => tooltip.max_width = 0.,
                6 => tooltip.placement.inset = f32::NAN,
                _ => tooltip.placement.gap = -1.,
            }
            let mut input = Interaction::default();
            let initial = scene(&root, &input, 0.);
            input.motion(&initial, 120., 45., 0.);
            assert_eq!(
                input.next_wakeup(&initial, 0.),
                None,
                "invalid policy {case}"
            );
            let result = scene(&root, &input, 2.);
            input.synchronize(&result);
            assert!(!has_tip(&result));
            assert!(!input.animating(2.));
        }
        let mut root = tree(false);
        root.children[1]
            .tooltip
            .as_mut()
            .unwrap()
            .transition
            .duration = 0.;
        let mut input = Interaction::default();
        let initial = scene(&root, &input, 0.);
        input.motion(&initial, 120., 45., 0.);
        let visible = scene(&root, &input, 0.5);
        input.synchronize(&visible);
        assert!(has_tip(&visible));
        assert!(!input.animating(0.5));
    }
    #[test]
    fn tooltip_keyboard_focus_is_immediate_and_escape_suppresses_actual_hover_target() {
        let tree = tree(false);
        let mut input = Interaction::default();
        let initial = scene(&tree, &input, 0.);
        input.key(&initial, &Key::Tab);
        input.key(&initial, &Key::Tab);
        assert_eq!(input.focus.as_deref(), Some("trigger"));
        assert!(has_tip(&scene(&tree, &input, 0.)));
        input.focus = Some("plain".into());
        input.focus_visible = false;
        input.motion(&initial, 120., 45., 0.);
        let visible = scene(&tree, &input, 1.);
        assert!(has_tip(&visible));
        input.key(&visible, &Key::Escape);
        assert!(!has_tip(&scene(&tree, &input, 1.)));
        input.motion(&initial, 210., 15., 1.);
        input.motion(&initial, 120., 45., 1.1);
        assert_eq!(input.next_wakeup(&initial, 1.1), Some(1.6));
    }
    #[test]
    fn removed_or_moved_tooltip_anchor_retires_the_deadline() {
        let mut input = Interaction::default();
        let original = tree(false);
        let initial = scene(&original, &input, 0.);
        input.motion(&initial, 120., 45., 0.);
        let mut moved = original.clone();
        moved.children[1].style.position = Some((110., 30.));
        let current = scene(&moved, &input, 0.2);
        assert_eq!(input.next_wakeup(&current, 0.2), None);
        assert!(!has_tip(&scene(&moved, &input, 1.)));
        let mut removed = original;
        removed.children.remove(1);
        assert_eq!(input.next_wakeup(&scene(&removed, &input, 0.2), 0.2), None);
    }
    #[test]
    fn popover_scopes_tab_escape_and_restores_only_a_live_opener() {
        let mut input = Interaction::default();
        input.focus = Some("trigger".into());
        input.focus_visible = true;
        let open = scene(&tree(true), &input, 0.);
        input.synchronize(&open);
        assert_eq!(input.focus.as_deref(), Some("first"));
        assert_eq!(
            input.popover_key(&open, &Key::Enter),
            Some(vec![Message::First])
        );
        assert_eq!(input.popover_key(&open, &Key::Tab), Some(vec![]));
        assert_eq!(input.focus.as_deref(), Some("second"));
        input.key(&open, &Key::Tab);
        assert_eq!(input.focus.as_deref(), Some("first"));
        input.key(&open, &Key::BackTab);
        assert_eq!(input.focus.as_deref(), Some("second"));
        assert_eq!(input.key(&open, &Key::Escape), [Message::Close]);
        let closed = scene(&tree(false), &input, 1.);
        input.synchronize(&closed);
        assert_eq!(input.focus.as_deref(), Some("trigger"));
        let open = scene(&tree(true), &input, 2.);
        input.synchronize(&open);
        let absent = scene(&Element::empty(), &input, 3.);
        input.synchronize(&absent);
        assert_eq!(input.focus, None);
    }
    #[test]
    fn outside_press_dismisses_without_releasing_an_underlying_action() {
        let mut input = Interaction::default();
        let open = scene(&tree(true), &input, 0.);
        input.synchronize(&open);
        assert_eq!(input.press(&open, 10., 10.), Some(Message::Close));
        assert_eq!(input.release(10., 10.), None);
        let bounds = open.overlays.popover.as_ref().unwrap().rect;
        assert_eq!(
            input.press(&open, bounds.x + bounds.w / 2., bounds.y + bounds.h - 8.),
            None
        );
        assert_eq!(
            input.release(bounds.x + bounds.w / 2., bounds.y + bounds.h - 8.),
            None
        );
    }
    #[test]
    fn scope_retirement_cancels_a_held_press_and_reports_dismissal() {
        let mut input = Interaction::default();
        let mut open_tree = tree(true);
        let open = scene(&open_tree, &input, 0.);
        input.synchronize(&open);
        let hit = open.hits.iter().find(|h| h.id == "first").unwrap();
        let point = (hit.rect.x + 4., hit.rect.y + 4.);
        input.press(&open, point.0, point.1);
        open_tree.children.retain(|e| e.id != "trigger");
        let retired = scene(&open_tree, &input, 1.);
        input.synchronize(&retired);
        assert_eq!(input.take_overlay_dismissal(), Some(Message::Close));
        assert_eq!(input.take_overlay_dismissal(), None);
        assert_eq!(input.release(point.0, point.1), None);
    }
    #[test]
    fn duplicate_anchor_and_synthetic_hit_id_collisions_are_rejected() {
        for duplicate in ["menu/backdrop", "menu/panel", "under"] {
            let mut tree = tree(true);
            tree.popovers[0].content.children[0].id = duplicate.into();
            let mut input = Interaction::default();
            let invalid = scene(&tree, &input, 0.);
            input.synchronize(&invalid);
            assert!(invalid.overlays.popover.is_none());
            assert_eq!(input.take_overlay_dismissal(), Some(Message::Close));
        }
        let mut tree = tree(true);
        tree.children.push(tree.children[1].clone());
        let mut input = Interaction::default();
        let invalid = scene(&tree, &input, 0.);
        input.synchronize(&invalid);
        assert!(invalid.overlays.popover.is_none());
        assert_eq!(input.take_overlay_dismissal(), Some(Message::Close));
    }
    #[test]
    fn opening_a_scope_drops_legacy_select_all_state_from_its_opener() {
        let mut input = Interaction::default();
        let mut root = Element::stack(vec![
            Element::input("opener", "", Message::Input)
                .size(120., 32.)
                .at(30., 20.)
                .id("trigger")
                .autofocus(),
        ]);
        let initial = scene(&root, &input, 0.);
        input.synchronize(&initial);
        input.key(&initial, &Key::SelectAll);
        root = root.popover(Popover {
            id: "menu".into(),
            anchor: "trigger".into(),
            placement: placement(),
            dismiss: Message::Close,
            content: Box::new(
                Element::input("value", "", Message::Input)
                    .size(120., 32.)
                    .id("editor"),
            ),
        });
        let open = scene(&root, &input, 1.);
        input.synchronize(&open);
        assert_eq!(input.focus.as_deref(), Some("editor"));
        assert_eq!(
            input.key(&open, &Key::Text("q".into())),
            [Message::Input("valueq".into())]
        );
    }
    #[test]
    fn input_only_suppresses_deferred_paint_while_retaining_popup_controls() {
        let mut input = Interaction::default();
        let hidden = tree(false).input_only();
        let initial = scene(&hidden, &input, 0.);
        input.motion(&initial, 120., 45., 0.);
        let result = scene(&hidden, &input, 1.);
        assert!(result.paint.is_empty());
        assert_eq!(input.next_wakeup(&result, 0.), None);
        let root = Element::stack(vec![tree(true).size(300., 220.).input_only()]);
        let result = scene(&root, &input, 1.);
        input.synchronize(&result);
        assert!(result.paint.is_empty());
        assert!(result.overlays.popover.is_some());
        assert_eq!(input.focus.as_deref(), Some("first"));
        assert_eq!(
            input.popover_key(&result, &Key::Enter),
            Some(vec![Message::First])
        );
    }
    #[test]
    fn hidden_owners_cannot_create_or_conflict_with_a_visible_scope() {
        for hidden in [
            Element::empty().opacity(0.),
            Element::empty().diagonal_reveal(0.),
        ] {
            let mut ignored = tree(true).popovers.remove(0);
            ignored.id = "hidden".into();
            let owner = hidden.popover(ignored);
            let mut root = tree(true);
            root.children.push(owner.clone());
            let result = scene(&root, &Interaction::default(), 1.);
            assert_eq!(
                result.overlays.popover.as_ref().map(|p| p.id.as_str()),
                Some("menu")
            );
            assert!(result.overlays.retired.is_empty());
            let mut root = tree(false);
            root.children.push(Element::stack(vec![owner]).opacity(0.));
            let result = scene(&root, &Interaction::default(), 1.);
            assert!(!result.overlays.has_popover());
        }
    }
    #[test]
    fn excluded_rounded_and_diagonal_anchor_pixels_cannot_host_overlays() {
        for diagonal in [false, true] {
            let anchor = Element::empty()
                .size(4., 4.)
                .on_click(Message::Open)
                .id("trigger")
                .tooltip(tooltip())
                .at(
                    if diagonal { 50. } else { 0. },
                    if diagonal { 50. } else { 0. },
                );
            let parent = Element::stack(vec![anchor])
                .size(60., 60.)
                .at(100., 30.)
                .clip();
            let parent = if diagonal {
                parent.diagonal_reveal(0.1)
            } else {
                parent.radius(30.)
            };
            let popup = tree(true).popovers.remove(0);
            let root = Element::stack(vec![parent]).popover(popup);
            let mut input = Interaction::default();
            input.focus = Some("trigger".into());
            input.focus_visible = true;
            let result = scene(&root, &input, 1.);
            input.synchronize(&result);
            assert!(result.overlays.popover.is_none());
            assert!(!has_tip(&result));
            assert_eq!(input.next_wakeup(&result, 0.), None);
            assert_eq!(input.take_overlay_dismissal(), Some(Message::Close));
        }
    }
    #[test]
    fn rejected_scopes_emit_once_until_removed_or_valid_again() {
        let mut root = tree(true);
        let mut other = root.popovers[0].clone();
        other.id = "other".into();
        other.dismiss = Message::Second;
        root.popovers.push(other);
        let mut input = Interaction::default();
        for now in [0., 1., 2.] {
            let rejected = scene(&root, &input, now);
            input.synchronize(&rejected);
            if now == 0. {
                assert_eq!(input.take_overlay_dismissal(), Some(Message::Close));
                assert_eq!(input.take_overlay_dismissal(), Some(Message::Second));
            }
            assert_eq!(
                input.take_overlay_dismissal(),
                None,
                "ignored dismissal cannot make a redraw loop"
            );
        }
        let removed = scene(&tree(false), &input, 3.);
        input.synchronize(&removed);
        let rejected = scene(&root, &input, 4.);
        input.synchronize(&rejected);
        assert_eq!(input.take_overlay_dismissal(), Some(Message::Close));
        assert_eq!(input.take_overlay_dismissal(), Some(Message::Second));
        assert_eq!(input.take_overlay_dismissal(), None);
    }
    #[test]
    fn same_scope_retires_removed_or_moved_primary_hits_and_refreshes_callbacks() {
        for change in 0..3 {
            let mut root = tree(true);
            let mut input = Interaction::default();
            let initial = scene(&root, &input, 0.);
            input.synchronize(&initial);
            let hit = initial.hits.iter().find(|hit| hit.id == "first").unwrap();
            let point = (hit.rect.x + 4., hit.rect.y + 4.);
            input.press(&initial, point.0, point.1);
            match change {
                0 => {
                    root.popovers[0].content.children.remove(0);
                }
                1 => root.children[1].style.position = Some((100., 60.)),
                _ => root.popovers[0].content.children[0].click = Some(Message::Second),
            }
            let current = scene(&root, &input, 1.);
            input.synchronize(&current);
            assert_eq!(
                input.release(point.0, point.1),
                if change == 2 {
                    Some(Message::Second)
                } else {
                    None
                }
            );
        }
    }
    #[test]
    fn initial_popup_focus_skips_scrolled_out_controls() {
        let mut root = tree(true);
        root.popovers[0].content = Box::new(
            Element::scroll(
                ScrollSpec::new(80., 40.).unwrap(),
                Element::column(
                    (0..4)
                        .map(|n| {
                            Element::empty()
                                .size(100., 40.)
                                .on_click(Message::Input(format!("row{n}")))
                                .id(format!("row-{n}"))
                        })
                        .collect(),
                ),
                |_| Message::Under,
            )
            .size(140., 80.)
            .tab_stop(false)
            .id("list"),
        );
        let mut input = Interaction::default();
        let current = scene(&root, &input, 0.);
        input.synchronize(&current);
        assert_eq!(input.focus.as_deref(), Some("row-2"));
        assert_eq!(
            input.popover_key(&current, &Key::Enter),
            Some(vec![Message::Input("row2".into())])
        );
    }
    #[test]
    fn tooltip_width_is_a_hard_bound_even_for_fixed_content() {
        let layout = layout();
        for width in [90., 300., 900.] {
            let mut hint = tooltip();
            hint.max_width = 100.;
            hint.content.style.width = Length::Fixed(600.);
            let root = Element::stack(vec![
                Element::empty()
                    .size(20., 20.)
                    .at(10., 30.)
                    .on_click(Message::Open)
                    .id("trigger")
                    .tooltip(hint),
            ]);
            let mut input = Interaction::default();
            let initial = layout.build(&root, width, 220., &input, 0.);
            input.motion(&initial, 20., 40., 0.);
            let result = layout.build(&root, width, 220., &input, 1.);
            let bound = result
                .paint
                .iter()
                .find_map(|paint| match paint {
                    Paint::Shape { rect, color, .. } if *color == Color::hex(0x123456) => {
                        Some(rect)
                    }
                    _ => None,
                })
                .expect("tooltip painted");
            assert!(bound.w <= 100.);
            assert!(bound.x >= 8. && bound.x + bound.w <= width - 8.);
        }
    }
    #[test]
    fn passive_popup_content_cannot_duplicate_the_source_anchor() {
        let mut root = tree(true);
        root.popovers[0].content =
            Box::new(Element::text("Passive duplicate").font(12.).id("trigger"));
        let mut input = Interaction::default();
        let current = scene(&root, &input, 0.);
        input.synchronize(&current);
        assert!(current.overlays.popover.is_none());
        assert_eq!(input.take_overlay_dismissal(), Some(Message::Close));
    }
}
