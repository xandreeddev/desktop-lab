//! Pure single-line editing. The component owns its String; this state owns no text.
use std::{fmt, ops::Range};
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
    Backspace,
    BackspaceWord,
    Delete,
    DeleteWord,
    Clear,
    SelectAll,
    Move { movement: TextMove, extend: bool },
    Select { anchor: usize, caret: usize },
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
            Self::Backspace => f.write_str("Backspace"),
            Self::BackspaceWord => f.write_str("BackspaceWord"),
            Self::Delete => f.write_str("Delete"),
            Self::DeleteWord => f.write_str("DeleteWord"),
            Self::Clear => f.write_str("Clear"),
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
    /// values above the admission budget remain readable/deletable.
    pub fn apply(
        &mut self,
        text: &mut String,
        edit: TextEdit,
        max_bytes: usize,
    ) -> Result<TextChange, TextEditError> {
        let before = *self;
        let mut selection = self.reconciled(text);
        let range = selection.range(text);
        let mut content = false;
        match edit {
            TextEdit::Copy | TextEdit::Cut | TextEdit::Paste | TextEdit::ClipboardReply(..) => {
                return Ok(TextChange::default());
            }
            TextEdit::Replace(value) => {
                let value = value.replace(['\n', '\r'], "");
                if value.len() > max_bytes {
                    return Err(TextEditError::ByteLimit);
                }
                content = *text != value;
                *text = value;
                selection = Self::at_end(text);
            }
            TextEdit::Insert(insert) => {
                let insert = insert.replace(['\n', '\r'], "");
                if insert.is_empty() {
                    return Ok(TextChange::default());
                }
                let retained = text.len() - range.len();
                if insert.len() > max_bytes.saturating_sub(retained) || retained > max_bytes {
                    return Err(TextEditError::ByteLimit);
                }
                content = text.get(range.clone()) != Some(insert.as_str());
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
        Ok(TextChange {
            content,
            selection: before != selection,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn edit(s: &mut TextSelection, value: &mut String, e: TextEdit) -> TextChange {
        s.apply(value, e, 8192).unwrap()
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
