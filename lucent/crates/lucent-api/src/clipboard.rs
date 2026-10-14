//! Clipboard effects and current-field completion policy. No native IO; pending snapshots are bounded and never canonical values.
use crate::{Effects, TextChange, TextEdit, TextEditError, TextSelection};
use std::fmt;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardTarget {
    pub surface: &'static str,
    pub instance: Option<crate::SurfaceInstance>,
    pub element: String,
}
impl ClipboardTarget {
    pub fn new(surface: &'static str, element: impl Into<String>) -> Self {
        Self {
            surface,
            instance: None,
            element: element.into(),
        }
    }
    pub fn in_instance(mut self, instance: crate::SurfaceInstance) -> Self {
        self.instance = Some(instance);
        self
    }
    pub fn address(&self) -> crate::SurfaceAddress {
        crate::SurfaceAddress {
            role: self.surface,
            instance: self.instance.clone(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardError {
    Unavailable,
    NoFocus,
    Unsupported,
    Busy,
    TooLarge,
    Timeout,
    InvalidText,
    Disconnected,
    Cancelled,
}
impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "Clipboard is unavailable",
            Self::NoFocus => "Clipboard requires the current focused field",
            Self::Unsupported => "Clipboard has no supported text format",
            Self::Busy => "Clipboard transfer is busy",
            Self::TooLarge => "Clipboard text exceeds the transfer limit",
            Self::Timeout => "Clipboard transfer timed out",
            Self::InvalidText => "Clipboard text is not valid UTF-8",
            Self::Disconnected => "Clipboard transfer disconnected",
            Self::Cancelled => "Clipboard target changed",
        })
    }
}
#[derive(Clone, PartialEq, Eq)]
pub enum ClipboardOutcome {
    Text(String),
    WritePosted,
    Error(ClipboardError),
}
impl fmt::Debug for ClipboardOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(_) => f.write_str("Text(..)"),
            Self::WritePosted => f.write_str("WritePosted"),
            Self::Error(error) => f.debug_tuple("Error").field(error).finish(),
        }
    }
}
pub enum ClipboardAction {
    Read,
    Write(String),
}
pub struct ClipboardRequest<M> {
    pub target: ClipboardTarget,
    pub action: ClipboardAction,
    complete: Box<dyn FnOnce(ClipboardOutcome) -> M + Send>,
}
impl<M: 'static> ClipboardRequest<M> {
    pub fn new(
        target: ClipboardTarget,
        action: ClipboardAction,
        complete: impl FnOnce(ClipboardOutcome) -> M + Send + 'static,
    ) -> Self {
        Self {
            target,
            action,
            complete: Box::new(complete),
        }
    }
    pub fn finish(self, outcome: ClipboardOutcome) -> M {
        (self.complete)(outcome)
    }
    pub fn map<N: 'static>(self, map: impl FnOnce(M) -> N + Send + 'static) -> ClipboardRequest<N> {
        ClipboardRequest {
            target: self.target,
            action: self.action,
            complete: Box::new(move |outcome| map((self.complete)(outcome))),
        }
    }
}
/// Maximum ordinary text retained for a clipboard request or native transfer.
pub const CLIPBOARD_TEXT_LIMIT: usize = 64 * 1024;
#[derive(Clone, Copy)]
enum Operation {
    Copy,
    Cut,
    Paste,
}
struct Pending {
    id: u64,
    value: String,
    selection: TextSelection,
    operation: Operation,
}
/// Ordinary field selection and one clipboard ticket. The String stays in its
/// component/aggregate. Any newer edit/reset retires the pending completion.
#[derive(Default)]
pub struct TextFieldState {
    selection: TextSelection,
    next: u64,
    pending: Option<Pending>,
}
impl TextFieldState {
    pub fn selection(&self) -> TextSelection {
        self.selection
    }
    pub fn reset(&mut self, value: &str) {
        self.cancel();
        self.selection = TextSelection::at_end(value);
    }
    pub fn cancel(&mut self) {
        self.pending = None;
        self.next = self.next.wrapping_add(1);
    }
    pub fn update<M: 'static>(
        &mut self,
        value: &mut String,
        edit: TextEdit,
        max_bytes: usize,
        target: ClipboardTarget,
        effects: &mut Effects<M>,
        map: impl Fn(TextEdit) -> M + Send + 'static,
    ) -> Result<TextChange, TextEditError> {
        if let TextEdit::ClipboardReply(id, outcome) = edit {
            let Some(pending) = self.pending.take() else {
                return Ok(TextChange::default());
            };
            if pending.id != id {
                self.pending = Some(pending);
                return Ok(TextChange::default());
            }
            if pending.value != *value || pending.selection != self.selection {
                return Ok(TextChange::default());
            }
            return match outcome {
                ClipboardOutcome::Text(text) if matches!(pending.operation, Operation::Paste) => {
                    self.selection
                        .apply(value, TextEdit::Insert(text), max_bytes)
                }
                ClipboardOutcome::WritePosted if matches!(pending.operation, Operation::Cut) => {
                    self.selection.apply(value, TextEdit::Delete, max_bytes)
                }
                ClipboardOutcome::Error(ClipboardError::Cancelled) => Ok(TextChange::default()),
                ClipboardOutcome::Error(error) => Err(TextEditError::Clipboard(error)),
                _ => Ok(TextChange::default()),
            };
        }
        self.cancel();
        if matches!(edit, TextEdit::Copy | TextEdit::Cut | TextEdit::Paste) {
            if value.len() > CLIPBOARD_TEXT_LIMIT {
                return Err(TextEditError::Clipboard(ClipboardError::TooLarge));
            }
            let selection = self.selection.reconciled(value);
            let action = if matches!(edit, TextEdit::Paste) {
                ClipboardAction::Read
            } else {
                let text = selection.selected(value);
                if text.is_empty() {
                    return Ok(TextChange::default());
                }
                ClipboardAction::Write(text.into())
            };
            let id = self.next;
            let operation = match edit {
                TextEdit::Copy => Operation::Copy,
                TextEdit::Cut => Operation::Cut,
                _ => Operation::Paste,
            };
            self.pending = Some(Pending {
                id,
                value: value.clone(),
                selection: self.selection,
                operation,
            });
            effects
                .clipboard
                .push(ClipboardRequest::new(target, action, move |outcome| {
                    map(TextEdit::ClipboardReply(id, outcome))
                }));
            Ok(TextChange::default())
        } else {
            self.selection.apply(value, edit, max_bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn edit(
        field: &mut TextFieldState,
        value: &mut String,
        edit: TextEdit,
        effects: &mut Effects<TextEdit>,
    ) -> Result<TextChange, TextEditError> {
        field.update(
            value,
            edit,
            8192,
            ClipboardTarget::new("test", "input"),
            effects,
            |message| message,
        )
    }
    #[test]
    fn paste_applies_at_current_selection_and_enforces_edit_budget() {
        let mut field = TextFieldState::default();
        let mut value = "café".to_string();
        field.reset(&value);
        let mut effects = Effects::default();
        edit(&mut field, &mut value, TextEdit::SelectAll, &mut effects).unwrap();
        edit(&mut field, &mut value, TextEdit::Paste, &mut effects).unwrap();
        assert_eq!(value, "café");
        let reply = effects
            .clipboard
            .pop()
            .unwrap()
            .finish(ClipboardOutcome::Text("tea\n".into()));
        assert!(
            edit(&mut field, &mut value, reply, &mut effects)
                .unwrap()
                .content
        );
        assert_eq!(value, "tea");
        edit(&mut field, &mut value, TextEdit::Paste, &mut effects).unwrap();
        let reply = effects
            .clipboard
            .pop()
            .unwrap()
            .finish(ClipboardOutcome::Text("x".repeat(8192)));
        assert!(edit(&mut field, &mut value, reply, &mut effects).is_err());
        assert_eq!(value, "tea");
    }
    #[test]
    fn late_paste_cannot_replace_new_edits_selection_or_reset() {
        for change in [
            TextEdit::Insert("!".into()),
            TextEdit::Move {
                movement: crate::TextMove::Start,
                extend: false,
            },
        ] {
            let mut field = TextFieldState::default();
            let mut value = "abc".to_string();
            field.reset(&value);
            let mut effects = Effects::default();
            edit(&mut field, &mut value, TextEdit::Paste, &mut effects).unwrap();
            let request = effects.clipboard.pop().unwrap();
            edit(&mut field, &mut value, change, &mut effects).unwrap();
            let expected = value.clone();
            edit(
                &mut field,
                &mut value,
                request.finish(ClipboardOutcome::Text("old".into())),
                &mut effects,
            )
            .unwrap();
            assert_eq!(value, expected);
        }
        let mut field = TextFieldState::default();
        let mut value = "abc".to_string();
        field.reset(&value);
        let mut effects = Effects::default();
        edit(&mut field, &mut value, TextEdit::Paste, &mut effects).unwrap();
        let request = effects.clipboard.pop().unwrap();
        field.reset(&value);
        edit(
            &mut field,
            &mut value,
            request.finish(ClipboardOutcome::Text("old".into())),
            &mut effects,
        )
        .unwrap();
        assert_eq!(value, "abc");
    }
    #[test]
    fn old_reply_does_not_consume_newer_request_and_external_change_is_rejected() {
        let mut field = TextFieldState::default();
        let mut value = "abc".to_string();
        field.reset(&value);
        let mut effects = Effects::default();
        edit(&mut field, &mut value, TextEdit::Paste, &mut effects).unwrap();
        let old = effects.clipboard.pop().unwrap();
        edit(&mut field, &mut value, TextEdit::Paste, &mut effects).unwrap();
        let current = effects.clipboard.pop().unwrap();
        edit(
            &mut field,
            &mut value,
            old.finish(ClipboardOutcome::Text("old".into())),
            &mut effects,
        )
        .unwrap();
        edit(
            &mut field,
            &mut value,
            current.finish(ClipboardOutcome::Text("new".into())),
            &mut effects,
        )
        .unwrap();
        assert_eq!(value, "abcnew");
        edit(&mut field, &mut value, TextEdit::Paste, &mut effects).unwrap();
        let request = effects.clipboard.pop().unwrap();
        value = "external".into();
        edit(
            &mut field,
            &mut value,
            request.finish(ClipboardOutcome::Text("stale".into())),
            &mut effects,
        )
        .unwrap();
        assert_eq!(value, "external");
    }
    #[test]
    fn cut_changes_only_after_write_posted_and_copy_keeps_value() {
        for action in [TextEdit::Copy, TextEdit::Cut] {
            let cut = matches!(action, TextEdit::Cut);
            let mut field = TextFieldState::default();
            let mut value = "🧑‍💻 café".to_string();
            field.reset(&value);
            let mut effects = Effects::default();
            edit(&mut field, &mut value, TextEdit::SelectAll, &mut effects).unwrap();
            edit(&mut field, &mut value, action, &mut effects).unwrap();
            let request = effects.clipboard.pop().unwrap();
            assert!(matches!(&request.action,ClipboardAction::Write(text) if text==&value));
            let reply = request.finish(ClipboardOutcome::WritePosted);
            assert!(!value.is_empty());
            edit(&mut field, &mut value, reply, &mut effects).unwrap();
            assert_eq!(value.is_empty(), cut);
        }
    }
    #[test]
    fn cancelled_or_denied_cut_preserves_original_and_reports_no_payload() {
        let mut field = TextFieldState::default();
        let mut value = "ordinary selected text".to_string();
        field.reset(&value);
        let mut effects = Effects::default();
        edit(&mut field, &mut value, TextEdit::SelectAll, &mut effects).unwrap();
        edit(&mut field, &mut value, TextEdit::Cut, &mut effects).unwrap();
        let reply = effects
            .clipboard
            .pop()
            .unwrap()
            .finish(ClipboardOutcome::Error(ClipboardError::NoFocus));
        assert_eq!(
            edit(&mut field, &mut value, reply, &mut effects),
            Err(TextEditError::Clipboard(ClipboardError::NoFocus))
        );
        assert_eq!(value, "ordinary selected text");
        let debug = format!("{:?}", ClipboardOutcome::Text(value.clone()));
        assert!(!debug.contains(&value));
        edit(&mut field, &mut value, TextEdit::Cut, &mut effects).unwrap();
        let reply = effects
            .clipboard
            .pop()
            .unwrap()
            .finish(ClipboardOutcome::Error(ClipboardError::Cancelled));
        assert_eq!(
            edit(&mut field, &mut value, reply, &mut effects).unwrap(),
            TextChange::default()
        );
        assert_eq!(value, "ordinary selected text");
    }
    #[test]
    fn oversized_existing_value_is_not_cloned_and_deletion_still_recovers() {
        let mut field = TextFieldState::default();
        let mut value = "x".repeat(CLIPBOARD_TEXT_LIMIT + 1);
        field.reset(&value);
        let mut effects = Effects::default();
        assert_eq!(
            edit(&mut field, &mut value, TextEdit::Paste, &mut effects),
            Err(TextEditError::Clipboard(ClipboardError::TooLarge))
        );
        assert!(effects.clipboard.is_empty());
        edit(&mut field, &mut value, TextEdit::SelectAll, &mut effects).unwrap();
        edit(&mut field, &mut value, TextEdit::Delete, &mut effects).unwrap();
        assert!(value.is_empty());
    }
    #[test]
    fn delegation_preserves_target_action_and_maps_completion_once() {
        let mut child = Effects::<TextEdit>::default();
        child.clipboard.push(ClipboardRequest::new(
            ClipboardTarget::new("child", "field"),
            ClipboardAction::Read,
            |outcome| TextEdit::ClipboardReply(42, outcome),
        ));
        let mut parent = Effects::<Option<TextEdit>>::default();
        parent.delegate(|effects| *effects = child, Some);
        let request = parent.clipboard.pop().unwrap();
        assert_eq!(request.target, ClipboardTarget::new("child", "field"));
        assert_eq!(
            request.finish(ClipboardOutcome::Text("ok".into())),
            Some(TextEdit::ClipboardReply(
                42,
                ClipboardOutcome::Text("ok".into())
            ))
        );
    }
}
