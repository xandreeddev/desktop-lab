//! Pure ordinary editing and bounded history; values stay component-owned.
use std::{collections::VecDeque, fmt, ops::Range};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextSelection {
    anchor: usize,
    caret: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextMove {
    PreviousGrapheme,
    NextGrapheme,
    PreviousWord,
    NextWord,
    Start,
    End,
}
#[derive(Clone, PartialEq, Eq)]
pub enum TextEdit {
    Copy,
    Cut,
    Paste,
    ClipboardReply(u64, super::ClipboardOutcome),
    Insert(String),
    Replace(String),
    /// One atomic compositor edit, relative to the current selected span.
    InputMethod {
        before: usize,
        after: usize,
        commit: String,
    },
    Backspace,
    BackspaceWord,
    Delete,
    DeleteWord,
    Clear,
    Undo,
    Redo,
    SelectAll,
    Move {
        movement: TextMove,
        extend: bool,
    },
    Select {
        anchor: usize,
        caret: usize,
    },
}
impl fmt::Debug for TextEdit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Copy => f.write_str("Copy"),
            Self::Cut => f.write_str("Cut"),
            Self::Paste => f.write_str("Paste"),
            Self::ClipboardReply(id, _) => f.debug_tuple("ClipboardReply").field(id).finish(),
            Self::Insert(_) => f.write_str("Insert(..)"),
            Self::Replace(_) => f.write_str("Replace(..)"),
            Self::InputMethod { before, after, .. } => f
                .debug_struct("InputMethod")
                .field("before", before)
                .field("after", after)
                .finish(),
            Self::Backspace => f.write_str("Backspace"),
            Self::BackspaceWord => f.write_str("BackspaceWord"),
            Self::Delete => f.write_str("Delete"),
            Self::DeleteWord => f.write_str("DeleteWord"),
            Self::Clear => f.write_str("Clear"),
            Self::Undo => f.write_str("Undo"),
            Self::Redo => f.write_str("Redo"),
            Self::SelectAll => f.write_str("SelectAll"),
            Self::Move { movement, extend } => f
                .debug_struct("Move")
                .field("movement", movement)
                .field("extend", extend)
                .finish(),
            Self::Select { anchor, caret } => f
                .debug_struct("Select")
                .field("anchor", anchor)
                .field("caret", caret)
                .finish(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextChange {
    pub content: bool,
    pub selection: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEditError {
    ByteLimit,
    Clipboard(super::ClipboardError),
}
impl fmt::Display for TextEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ByteLimit => f.write_str("Text edit exceeds the field byte limit"),
            Self::Clipboard(error) => fmt::Display::fmt(error, f),
        }
    }
}
impl std::error::Error for TextEditError {}
fn normalized(value: String, multiline: bool) -> String {
    if multiline {
        value.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        value.replace(['\n', '\r'], "")
    }
}

fn floor(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }
    text.grapheme_indices(true)
        .map(|(at, _)| at)
        .take_while(|at| *at <= offset)
        .last()
        .unwrap_or(0)
}
fn next(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .map(|(at, _)| at)
        .find(|at| *at > offset)
        .unwrap_or(text.len())
}
fn previous(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .map(|(at, _)| at)
        .take_while(|at| *at < offset)
        .last()
        .unwrap_or(0)
}
impl TextSelection {
    pub fn boundaries(text: &str) -> impl Iterator<Item = usize> + '_ {
        text.grapheme_indices(true)
            .map(|(at, _)| at)
            .chain(std::iter::once(text.len()))
    }
    pub fn at_end(text: &str) -> Self {
        Self {
            anchor: text.len(),
            caret: text.len(),
        }
    }
    pub fn anchor(self) -> usize {
        self.anchor
    }
    pub fn caret(self) -> usize {
        self.caret
    }
    /// Clamp an externally changed value at extended-grapheme boundaries.
    pub fn reconciled(self, text: &str) -> Self {
        Self {
            anchor: floor(text, self.anchor),
            caret: floor(text, self.caret),
        }
    }
    pub fn range(self, text: &str) -> Range<usize> {
        let s = self.reconciled(text);
        s.anchor.min(s.caret)..s.anchor.max(s.caret)
    }
    pub fn selected(self, text: &str) -> &str {
        &text[self.range(text)]
    }
    /// Apply an edit against the current value, not the last painted snapshot.
    /// A rejected insertion changes neither the value nor selection. Existing
    /// values above the admission budget remain readable/deletable. Undo/Redo
    /// require TextFieldState, since a selection owns no history.
    pub fn apply(
        &mut self,
        text: &mut String,
        edit: TextEdit,
        max_bytes: usize,
    ) -> Result<TextChange, TextEditError> {
        self.apply_inner(text, edit, max_bytes, false, false)
            .map(|(change, _)| change)
    }
    fn apply_inner(
        &mut self,
        text: &mut String,
        edit: TextEdit,
        max_bytes: usize,
        record: bool,
        multiline: bool,
    ) -> Result<(TextChange, Option<TextDelta>), TextEditError> {
        let before = *self;
        let mut selection = self.reconciled(text);
        let range = selection.range(text);
        let mut content = false;
        let mut delta = None;
        match edit {
            TextEdit::Copy
            | TextEdit::Cut
            | TextEdit::Paste
            | TextEdit::ClipboardReply(..)
            | TextEdit::Undo
            | TextEdit::Redo => {
                return Ok((TextChange::default(), None));
            }
            TextEdit::Replace(value) => {
                let value = normalized(value, multiline);
                if value.len() > max_bytes {
                    return Err(TextEditError::ByteLimit);
                }
                content = *text != value;
                if record && content {
                    delta = TextDelta::new(text, 0..text.len(), &value, selection);
                }
                *text = value;
                selection = Self::at_end(text);
            }
            TextEdit::InputMethod {
                before: delete_before,
                after: delete_after,
                commit,
            } => {
                let insert = normalized(commit, multiline);
                let start = floor(text, range.start.saturating_sub(delete_before));
                let desired = range.end.saturating_add(delete_after).min(text.len());
                let end = if floor(text, desired) == desired {
                    desired
                } else {
                    next(text, desired)
                };
                let retained = text.len() - (end - start);
                if retained > max_bytes || insert.len() > max_bytes.saturating_sub(retained) {
                    return Err(TextEditError::ByteLimit);
                }
                content = text.get(start..end) != Some(insert.as_str());
                if record && content {
                    delta = TextDelta::new(text, start..end, &insert, selection);
                }
                text.replace_range(start..end, &insert);
                let target = start + insert.len();
                let at = if floor(text, target) == target {
                    target
                } else {
                    next(text, target)
                };
                selection = Self {
                    anchor: at,
                    caret: at,
                };
            }
            TextEdit::Insert(insert) => {
                let insert = normalized(insert, multiline);
                if insert.is_empty() {
                    return Ok((TextChange::default(), None));
                }
                let retained = text.len() - range.len();
                if insert.len() > max_bytes.saturating_sub(retained) || retained > max_bytes {
                    return Err(TextEditError::ByteLimit);
                }
                content = text.get(range.clone()) != Some(insert.as_str());
                if record && content {
                    delta = TextDelta::new(text, range.clone(), &insert, selection);
                }
                text.replace_range(range.clone(), &insert);
                let target = range.start + insert.len();
                let at = if floor(text, target) == target {
                    target
                } else {
                    next(text, target)
                };
                selection = Self {
                    anchor: at,
                    caret: at,
                };
            }
            TextEdit::Backspace
            | TextEdit::Delete
            | TextEdit::BackspaceWord
            | TextEdit::DeleteWord => {
                let deletion = if !range.is_empty() {
                    range
                } else if matches!(edit, TextEdit::Backspace) {
                    previous(text, selection.caret)..selection.caret
                } else if matches!(edit, TextEdit::BackspaceWord) {
                    text.unicode_word_indices()
                        .map(|(at, _)| at)
                        .take_while(|at| *at < selection.caret)
                        .last()
                        .unwrap_or(0)..selection.caret
                } else if matches!(edit, TextEdit::DeleteWord) {
                    selection.caret
                        ..text
                            .unicode_word_indices()
                            .map(|(at, word)| at + word.len())
                            .find(|at| *at > selection.caret)
                            .unwrap_or(text.len())
                } else {
                    selection.caret..next(text, selection.caret)
                };
                if !deletion.is_empty() {
                    if record {
                        delta = TextDelta::new(text, deletion.clone(), "", selection);
                    }
                    let at = deletion.start;
                    text.replace_range(deletion, "");
                    let at = floor(text, at);
                    selection = Self {
                        anchor: at,
                        caret: at,
                    };
                    content = true;
                }
            }
            TextEdit::Clear => {
                content = !text.is_empty();
                if record && content {
                    delta = TextDelta::new(text, 0..text.len(), "", selection);
                }
                text.clear();
                selection = Self::default();
            }
            TextEdit::SelectAll => {
                selection = Self {
                    anchor: 0,
                    caret: text.len(),
                };
            }
            TextEdit::Select { anchor, caret } => {
                selection = Self {
                    anchor: floor(text, anchor),
                    caret: floor(text, caret),
                };
            }
            TextEdit::Move { movement, extend } => {
                let collapse = !extend
                    && !range.is_empty()
                    && matches!(
                        movement,
                        TextMove::PreviousGrapheme | TextMove::NextGrapheme
                    );
                let target = if collapse {
                    if movement == TextMove::PreviousGrapheme {
                        range.start
                    } else {
                        range.end
                    }
                } else {
                    match movement {
                        TextMove::PreviousGrapheme => previous(text, selection.caret),
                        TextMove::NextGrapheme => next(text, selection.caret),
                        TextMove::Start => 0,
                        TextMove::End => text.len(),
                        TextMove::PreviousWord => text
                            .unicode_word_indices()
                            .map(|(at, _)| at)
                            .take_while(|at| *at < selection.caret)
                            .last()
                            .unwrap_or(0),
                        TextMove::NextWord => text
                            .unicode_word_indices()
                            .map(|(at, word)| at + word.len())
                            .find(|at| *at > selection.caret)
                            .unwrap_or(text.len()),
                    }
                };
                selection.caret = floor(text, target);
                if !extend {
                    selection.anchor = selection.caret;
                }
            }
        }
        *self = selection;
        if let Some(delta) = &mut delta {
            delta.after = selection;
        }
        Ok((
            TextChange {
                content,
                selection: before != selection,
            },
            delta,
        ))
    }
}

// Shared by ordinary fields only. Exact-sized spans bound retained payload,
// including redo; oversized edits remain accepted but retire earlier history.
const HISTORY_EDITS: usize = 128;
const HISTORY_BYTES: usize = 64 * 1024;
const SNAPSHOT_BYTES: usize = 64 * 1024;
const TYPING_BYTES: usize = 4 * 1024;
struct TextDelta {
    at: usize,
    removed: Box<str>,
    inserted: Box<str>,
    before: TextSelection,
    after: TextSelection,
}
impl TextDelta {
    fn new(text: &str, range: Range<usize>, inserted: &str, before: TextSelection) -> Option<Self> {
        if range.len() > HISTORY_BYTES.saturating_sub(inserted.len())
            || inserted.len() > HISTORY_BYTES
        {
            return None;
        }
        Some(Self {
            at: range.start,
            removed: text[range].into(),
            inserted: inserted.into(),
            before,
            after: before,
        })
    }
    fn bytes(&self) -> usize {
        self.removed.len() + self.inserted.len()
    }
    fn typing(&self) -> bool {
        self.removed.is_empty()
            && self.before.anchor == self.before.caret
            && self.after.anchor == self.after.caret
            && self.after.caret == self.at + self.inserted.len()
    }
}
#[derive(Default)]
pub(crate) struct TextHistory {
    undo: VecDeque<TextDelta>,
    redo: VecDeque<TextDelta>,
    bytes: usize,
    // Exact comparison retires history even if an external replacement kept
    // the same length and changed span. This bounded snapshot is not canonical.
    snapshot: Option<Box<str>>,
    typing: bool,
}
impl TextHistory {
    fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
        self.break_group();
    }
    pub(crate) fn reset(&mut self, value: &str) {
        self.clear();
        self.remember(value);
    }
    pub(crate) fn break_group(&mut self) {
        self.typing = false;
    }
    pub(crate) fn reconcile(&mut self, value: &str) -> bool {
        let changed = self
            .snapshot
            .as_deref()
            .is_some_and(|previous| previous != value);
        if changed || value.len() > SNAPSHOT_BYTES {
            self.clear();
        }
        if changed || self.snapshot.is_none() {
            self.remember(value);
        }
        changed
    }
    fn remember(&mut self, value: &str) {
        self.snapshot = (value.len() <= SNAPSHOT_BYTES).then(|| value.into());
    }
    pub(crate) fn apply(
        &mut self,
        selection: &mut TextSelection,
        value: &mut String,
        edit: TextEdit,
        max_bytes: usize,
        typed: bool,
        multiline: bool,
    ) -> Result<TextChange, TextEditError> {
        if matches!(edit, TextEdit::Undo | TextEdit::Redo) {
            return self.replay(selection, value, max_bytes, matches!(edit, TextEdit::Undo));
        }
        // Hard line insertion is an independent undo boundary, including typed Enter.
        let line_break =
            multiline && matches!(&edit, TextEdit::Insert(value) if value.contains(['\n', '\r']));
        let typing = typed && !line_break && matches!(edit, TextEdit::Insert(_));
        if line_break {
            self.break_group();
        }
        let record = value.len() <= SNAPSHOT_BYTES;
        let (change, delta) = match selection.apply_inner(value, edit, max_bytes, record, multiline)
        {
            Ok(result) => result,
            Err(error) => {
                self.break_group();
                return Err(error);
            }
        };
        if change.content {
            self.bytes -= self.redo.iter().map(TextDelta::bytes).sum::<usize>();
            self.redo.clear();
            if let Some(delta) = delta.filter(|_| value.len() <= SNAPSHOT_BYTES) {
                self.record(delta, typing);
            } else {
                self.clear();
            }
            self.remember(value);
        } else {
            self.break_group();
        }
        Ok(change)
    }
    fn record(&mut self, delta: TextDelta, typing: bool) {
        let grouping = typing && delta.typing();
        if grouping
            && self.typing
            && let Some(previous) = self.undo.back_mut()
            && previous.typing()
            && previous.after == delta.before
            && previous.at + previous.inserted.len() == delta.at
            && previous.inserted.len() + delta.inserted.len() <= TYPING_BYTES
        {
            let mut inserted =
                String::with_capacity(previous.inserted.len() + delta.inserted.len());
            inserted.push_str(&previous.inserted);
            inserted.push_str(&delta.inserted);
            previous.inserted = inserted.into_boxed_str();
            previous.after = delta.after;
            self.bytes += delta.bytes();
            self.trim();
            return;
        }
        self.bytes += delta.bytes();
        self.undo.push_back(delta);
        self.typing = grouping;
        self.trim();
    }
    fn trim(&mut self) {
        while self.undo.len() + self.redo.len() > HISTORY_EDITS || self.bytes > HISTORY_BYTES {
            let Some(oldest) = self.undo.pop_front() else {
                break;
            };
            self.bytes -= oldest.bytes();
        }
    }
    fn replay(
        &mut self,
        selection: &mut TextSelection,
        value: &mut String,
        max_bytes: usize,
        undo: bool,
    ) -> Result<TextChange, TextEditError> {
        self.break_group();
        let before = *selection;
        let source = if undo { &self.undo } else { &self.redo };
        let Some(delta) = source.back() else {
            *selection = selection.reconciled(value);
            return Ok(TextChange {
                content: false,
                selection: before != *selection,
            });
        };
        let (expected, replacement, restored) = if undo {
            (&delta.inserted, &delta.removed, delta.before)
        } else {
            (&delta.removed, &delta.inserted, delta.after)
        };
        let end = delta.at + expected.len();
        if value.get(delta.at..end) != Some(expected.as_ref()) {
            self.reset(value);
            *selection = selection.reconciled(value);
            return Ok(TextChange {
                content: false,
                selection: before != *selection,
            });
        }
        let retained = value.len() - expected.len();
        if !replacement.is_empty()
            && (retained > max_bytes || replacement.len() > max_bytes.saturating_sub(retained))
        {
            return Err(TextEditError::ByteLimit);
        }
        value.replace_range(delta.at..end, replacement);
        *selection = restored.reconciled(value);
        if undo {
            self.redo.push_back(self.undo.pop_back().unwrap());
        } else {
            self.undo.push_back(self.redo.pop_back().unwrap());
        }
        self.remember(value);
        Ok(TextChange {
            content: true,
            selection: before != *selection,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn edit(s: &mut TextSelection, value: &mut String, e: TextEdit) -> TextChange {
        s.apply(value, e, 8192).unwrap()
    }
    fn history_edit(
        history: &mut TextHistory,
        selection: &mut TextSelection,
        value: &mut String,
        edit: TextEdit,
        max_bytes: usize,
    ) -> Result<TextChange, TextEditError> {
        history.reconcile(value);
        history.apply(selection, value, edit, max_bytes, true, false)
    }
    #[test]
    fn undo_restores_joined_graphemes_and_directional_selection() {
        for (original, caret, inserted) in [("a", 1, "\u{301}"), ("👩💻", 4, "\u{200d}")] {
            let mut value = original.to_string();
            let mut selection = TextSelection {
                anchor: caret,
                caret,
            };
            let mut history = TextHistory::default();
            let before = selection;
            history_edit(
                &mut history,
                &mut selection,
                &mut value,
                TextEdit::Insert(inserted.into()),
                8192,
            )
            .unwrap();
            let combined = value.clone();
            let after = selection;
            history_edit(
                &mut history,
                &mut selection,
                &mut value,
                TextEdit::Undo,
                8192,
            )
            .unwrap();
            assert_eq!(value, original);
            assert_eq!(selection, before);
            history_edit(
                &mut history,
                &mut selection,
                &mut value,
                TextEdit::Redo,
                8192,
            )
            .unwrap();
            assert_eq!(value, combined);
            assert_eq!(selection, after);
        }
        let mut value = "Aé💻Z".to_string();
        let mut selection = TextSelection {
            anchor: value.len(),
            caret: 1,
        };
        let original = selection;
        let mut history = TextHistory::default();
        history_edit(
            &mut history,
            &mut selection,
            &mut value,
            TextEdit::Insert("!".into()),
            8192,
        )
        .unwrap();
        assert_eq!(value, "A!");
        history_edit(
            &mut history,
            &mut selection,
            &mut value,
            TextEdit::Undo,
            8192,
        )
        .unwrap();
        assert_eq!(value, "Aé💻Z");
        assert_eq!(selection, original);
        assert_eq!(selection.selected(&value), "é💻Z");
    }
    #[test]
    fn adjacent_typing_coalesces_and_navigation_starts_a_new_group() {
        let mut value = String::new();
        let mut selection = TextSelection::default();
        let mut history = TextHistory::default();
        for inserted in ["h", "é", "!"] {
            history_edit(
                &mut history,
                &mut selection,
                &mut value,
                TextEdit::Insert(inserted.into()),
                8192,
            )
            .unwrap();
        }
        assert_eq!(history.undo.len(), 1);
        history_edit(
            &mut history,
            &mut selection,
            &mut value,
            TextEdit::Move {
                movement: TextMove::Start,
                extend: false,
            },
            8192,
        )
        .unwrap();
        history_edit(
            &mut history,
            &mut selection,
            &mut value,
            TextEdit::Insert("new ".into()),
            8192,
        )
        .unwrap();
        history_edit(
            &mut history,
            &mut selection,
            &mut value,
            TextEdit::Undo,
            8192,
        )
        .unwrap();
        assert_eq!(value, "hé!");
        assert_eq!(selection.caret(), 0);
        history_edit(
            &mut history,
            &mut selection,
            &mut value,
            TextEdit::Undo,
            8192,
        )
        .unwrap();
        assert_eq!(value, "");
        for expected in ["hé!", "new hé!"] {
            history_edit(
                &mut history,
                &mut selection,
                &mut value,
                TextEdit::Redo,
                8192,
            )
            .unwrap();
            assert_eq!(value, expected);
        }
    }
    #[test]
    fn rejection_and_identical_edits_preserve_redo_but_new_content_retires_it() {
        let mut value = "abc".to_string();
        let mut selection = TextSelection::at_end(&value);
        let mut history = TextHistory::default();
        history_edit(
            &mut history,
            &mut selection,
            &mut value,
            TextEdit::Insert("!".into()),
            4,
        )
        .unwrap();
        history_edit(&mut history, &mut selection, &mut value, TextEdit::Undo, 4).unwrap();
        assert_eq!(
            history_edit(
                &mut history,
                &mut selection,
                &mut value,
                TextEdit::Insert("XX".into()),
                4
            ),
            Err(TextEditError::ByteLimit)
        );
        history_edit(
            &mut history,
            &mut selection,
            &mut value,
            TextEdit::SelectAll,
            4,
        )
        .unwrap();
        assert!(
            !history_edit(
                &mut history,
                &mut selection,
                &mut value,
                TextEdit::Insert("abc".into()),
                4
            )
            .unwrap()
            .content
        );
        history_edit(&mut history, &mut selection, &mut value, TextEdit::Redo, 4).unwrap();
        assert_eq!(value, "abc!");
        history_edit(&mut history, &mut selection, &mut value, TextEdit::Undo, 4).unwrap();
        history_edit(
            &mut history,
            &mut selection,
            &mut value,
            TextEdit::Insert("?".into()),
            4,
        )
        .unwrap();
        assert_eq!(
            history_edit(&mut history, &mut selection, &mut value, TextEdit::Redo, 4).unwrap(),
            TextChange::default()
        );
        assert_eq!(value, "abc?");
    }
    #[test]
    fn history_playback_respects_current_byte_admission_without_losing_the_edit() {
        let mut value = "abcdef".to_string();
        let mut selection = TextSelection::at_end(&value);
        let mut history = TextHistory::default();
        history_edit(&mut history, &mut selection, &mut value, TextEdit::Clear, 3).unwrap();
        let before = selection;
        assert_eq!(
            history_edit(&mut history, &mut selection, &mut value, TextEdit::Undo, 3),
            Err(TextEditError::ByteLimit)
        );
        assert!(value.is_empty());
        assert_eq!(selection, before);
        history_edit(&mut history, &mut selection, &mut value, TextEdit::Undo, 6).unwrap();
        assert_eq!(value, "abcdef");
        history_edit(&mut history, &mut selection, &mut value, TextEdit::Redo, 3).unwrap();
        assert!(value.is_empty());
    }
    #[test]
    fn external_replacement_reset_and_oversized_edits_retire_history() {
        let mut value = "abc".to_string();
        let mut selection = TextSelection::at_end(&value);
        let mut history = TextHistory::default();
        history_edit(
            &mut history,
            &mut selection,
            &mut value,
            TextEdit::Insert("!".into()),
            8192,
        )
        .unwrap();
        value = "xyz!".into(); // Same length and same changed span as the old value.
        assert_eq!(
            history_edit(
                &mut history,
                &mut selection,
                &mut value,
                TextEdit::Undo,
                8192
            )
            .unwrap(),
            TextChange::default()
        );
        assert_eq!(value, "xyz!");
        history_edit(
            &mut history,
            &mut selection,
            &mut value,
            TextEdit::Clear,
            8192,
        )
        .unwrap();
        history.reset(&value);
        assert_eq!(
            history_edit(
                &mut history,
                &mut selection,
                &mut value,
                TextEdit::Undo,
                8192
            )
            .unwrap(),
            TextChange::default()
        );
        value = "x".repeat(HISTORY_BYTES + 1);
        selection = TextSelection::at_end(&value);
        history_edit(
            &mut history,
            &mut selection,
            &mut value,
            TextEdit::Clear,
            8192,
        )
        .unwrap();
        assert!(value.is_empty());
        assert!(history.undo.is_empty());
        assert_eq!(history.bytes, 0);
    }
    #[test]
    fn undo_and_redo_share_bounded_count_and_retained_payload() {
        let mut value = String::new();
        let mut selection = TextSelection::default();
        let mut history = TextHistory::default();
        for step in 0..140 {
            history_edit(
                &mut history,
                &mut selection,
                &mut value,
                TextEdit::Replace(format!("step{step}")),
                8192,
            )
            .unwrap();
            assert!(history.undo.len() + history.redo.len() <= HISTORY_EDITS);
        }
        for _ in 0..HISTORY_EDITS {
            history_edit(
                &mut history,
                &mut selection,
                &mut value,
                TextEdit::Undo,
                8192,
            )
            .unwrap();
        }
        assert_eq!(value, "step11");
        assert_eq!(history.undo.len() + history.redo.len(), HISTORY_EDITS);
        value.clear();
        selection = TextSelection::default();
        history.reset(&value);
        for step in 0..140 {
            history.reconcile(&value);
            history
                .apply(
                    &mut selection,
                    &mut value,
                    TextEdit::Replace(format!("{step:04}{}", "x".repeat(1020))),
                    usize::MAX,
                    false,
                    false,
                )
                .unwrap();
            assert!(history.bytes <= HISTORY_BYTES);
            assert!(
                history.snapshot.as_ref().unwrap().len() + history.bytes
                    <= SNAPSHOT_BYTES + HISTORY_BYTES
            );
        }
        for _ in 0..32 {
            history_edit(
                &mut history,
                &mut selection,
                &mut value,
                TextEdit::Undo,
                usize::MAX,
            )
            .unwrap();
            assert!(history.bytes <= HISTORY_BYTES);
        }
        assert!(value.starts_with("0107"));
        assert_eq!(value.len(), 1024);
        assert_eq!(history.redo.len(), 32);
        assert_eq!(history.bytes, HISTORY_BYTES);
    }
    #[test]
    fn crossing_snapshot_limit_clears_history_and_bounded_edits_reenable_it() {
        let mut value = "x".to_string();
        let mut selection = TextSelection::at_end(&value);
        let mut history = TextHistory::default();
        history_edit(
            &mut history,
            &mut selection,
            &mut value,
            TextEdit::Insert("y".repeat(SNAPSHOT_BYTES)),
            usize::MAX,
        )
        .unwrap();
        assert_eq!(value.len(), SNAPSHOT_BYTES + 1);
        assert!(history.snapshot.is_none());
        assert!(history.undo.is_empty());
        assert_eq!(history.bytes, 0);
        history_edit(
            &mut history,
            &mut selection,
            &mut value,
            TextEdit::Backspace,
            1,
        )
        .unwrap();
        assert_eq!(value.len(), SNAPSHOT_BYTES);
        assert!(history.snapshot.is_some());
        assert!(history.undo.is_empty());
        history_edit(&mut history, &mut selection, &mut value, TextEdit::Clear, 1).unwrap();
        assert!(value.is_empty());
        assert_eq!(history.bytes, HISTORY_BYTES);
        history_edit(
            &mut history,
            &mut selection,
            &mut value,
            TextEdit::Insert("z".into()),
            1,
        )
        .unwrap();
        assert_eq!(history.bytes, 1);
        history_edit(&mut history, &mut selection, &mut value, TextEdit::Undo, 1).unwrap();
        assert!(value.is_empty());
    }
    #[test]
    fn grapheme_deletion_and_burst_insertion_use_current_client_state() {
        for grapheme in ["e\u{301}", "👩‍💻", "🇺🇳", "क्‍ष"] {
            let mut value = format!("A{grapheme}B");
            let mut selection = TextSelection::at_end(&value);
            edit(
                &mut selection,
                &mut value,
                TextEdit::Move {
                    movement: TextMove::PreviousGrapheme,
                    extend: false,
                },
            );
            edit(&mut selection, &mut value, TextEdit::Backspace);
            assert_eq!(value, "AB");
            for chunk in ["é", "!", "x"] {
                edit(&mut selection, &mut value, TextEdit::Insert(chunk.into()));
            }
            assert_eq!(value, "Aé!xB");
        }
    }
    #[test]
    fn selection_replacement_shift_navigation_and_forward_delete_are_atomic() {
        let mut value = "Aé💻Z".to_string();
        let mut selection = TextSelection::at_end(&value);
        for _ in 0..2 {
            edit(
                &mut selection,
                &mut value,
                TextEdit::Move {
                    movement: TextMove::PreviousGrapheme,
                    extend: true,
                },
            );
        }
        assert_eq!(selection.selected(&value), "💻Z");
        assert!(edit(&mut selection, &mut value, TextEdit::Insert("!\n".into())).content);
        assert_eq!(value, "Aé!");
        edit(
            &mut selection,
            &mut value,
            TextEdit::Move {
                movement: TextMove::Start,
                extend: false,
            },
        );
        edit(&mut selection, &mut value, TextEdit::Delete);
        assert_eq!(value, "é!");
        edit(&mut selection, &mut value, TextEdit::SelectAll);
        edit(&mut selection, &mut value, TextEdit::Insert("new".into()));
        assert_eq!(value, "new");
    }
    #[test]
    fn byte_limit_rejection_preserves_selection_and_oversized_data_can_be_recovered() {
        let mut value = "abcdef".to_string();
        let mut s = TextSelection::at_end(&value);
        let before = s;
        assert_eq!(
            s.apply(&mut value, TextEdit::Insert("x".into()), 3),
            Err(TextEditError::ByteLimit)
        );
        assert_eq!(s, before);
        assert_eq!(value, "abcdef");
        edit(&mut s, &mut value, TextEdit::SelectAll);
        s.apply(&mut value, TextEdit::Insert("é".into()), 2)
            .unwrap();
        assert_eq!(value, "é");
        assert!(!format!("{:?}", TextEdit::Insert("fixture text".into())).contains("fixture text"));
    }
    #[test]
    fn external_changes_and_pointer_offsets_never_split_utf8_or_graphemes() {
        let mut value = "e\u{301}💻z".to_string();
        let mut s = TextSelection::default();
        edit(
            &mut s,
            &mut value,
            TextEdit::Select {
                anchor: 1,
                caret: 5,
            },
        );
        assert_eq!(s.anchor(), 0);
        assert_eq!(s.caret(), 3);
        assert_eq!(s.selected(&value), "e\u{301}");
        value = "x".into();
        assert_eq!(s.reconciled(&value).caret(), 1);
        edit(&mut s, &mut value, TextEdit::Delete);
        assert_eq!(value, "");
    }
    #[test]
    fn word_navigation_and_selection_collapse_preserve_direction() {
        let mut value = "one two 三".to_string();
        let mut s = TextSelection::at_end(&value);
        edit(
            &mut s,
            &mut value,
            TextEdit::Move {
                movement: TextMove::PreviousWord,
                extend: true,
            },
        );
        assert_eq!(s.selected(&value), "三");
        edit(
            &mut s,
            &mut value,
            TextEdit::Move {
                movement: TextMove::PreviousGrapheme,
                extend: false,
            },
        );
        assert_eq!(s.caret(), 8);
        assert!(s.selected(&value).is_empty());
        edit(
            &mut s,
            &mut value,
            TextEdit::Move {
                movement: TextMove::NextWord,
                extend: false,
            },
        );
        assert_eq!(s.caret(), value.len());
    }
    #[test]
    fn insertion_and_deletion_reconcile_newly_joined_clusters() {
        let mut value = "a".to_string();
        let mut s = TextSelection::at_end(&value);
        edit(&mut s, &mut value, TextEdit::Insert("\u{301}".into()));
        assert_eq!(s.caret(), value.len());
        edit(&mut s, &mut value, TextEdit::Backspace);
        assert!(value.is_empty());
    }
    #[test]
    fn word_deletion_and_identical_replacement_report_actual_content_changes() {
        let mut value = "one two three".to_string();
        let mut s = TextSelection::at_end(&value);
        edit(&mut s, &mut value, TextEdit::BackspaceWord);
        assert_eq!(value, "one two ");
        edit(
            &mut s,
            &mut value,
            TextEdit::Move {
                movement: TextMove::Start,
                extend: false,
            },
        );
        edit(&mut s, &mut value, TextEdit::DeleteWord);
        assert_eq!(value, " two ");
        let same = value.clone();
        assert!(!edit(&mut s, &mut value, TextEdit::Replace(same)).content);
        edit(&mut s, &mut value, TextEdit::SelectAll);
        let same = value.clone();
        assert!(!edit(&mut s, &mut value, TextEdit::Insert(same)).content);
    }
}

#[cfg(test)]
mod input_method_edits {
    use super::*;
    #[test]
    fn surrounding_delete_and_commit_are_one_atomic_grapheme_safe_edit() {
        let mut text = "ab日c".to_owned();
        let mut selected = TextSelection::default();
        selected
            .apply(
                &mut text,
                TextEdit::Select {
                    anchor: 2,
                    caret: 2,
                },
                64,
            )
            .unwrap();
        selected
            .apply(
                &mut text,
                TextEdit::InputMethod {
                    before: 1,
                    after: 1,
                    commit: "語".into(),
                },
                64,
            )
            .unwrap();
        assert_eq!(text, "a語c");
        assert_eq!(selected.caret(), "a語".len());
    }
    #[test]
    fn rejected_input_method_batch_preserves_value_and_directional_selection() {
        let mut text = "αβγ".to_owned();
        let mut selected = TextSelection::default();
        selected
            .apply(
                &mut text,
                TextEdit::Select {
                    anchor: 4,
                    caret: 2,
                },
                64,
            )
            .unwrap();
        let before = selected;
        assert_eq!(
            selected.apply(
                &mut text,
                TextEdit::InputMethod {
                    before: 0,
                    after: 0,
                    commit: "too long".into()
                },
                6
            ),
            Err(TextEditError::ByteLimit)
        );
        assert_eq!(text, "αβγ");
        assert_eq!(selected, before);
        assert!(
            !format!(
                "{:?}",
                TextEdit::InputMethod {
                    before: 0,
                    after: 0,
                    commit: "private".into()
                }
            )
            .contains("private")
        );
    }
}
