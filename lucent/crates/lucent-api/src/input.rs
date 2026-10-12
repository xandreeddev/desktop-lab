//! Edit intents for a field whose actual value stays in its component.
//! Debug output never includes the transient inserted text.
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub enum InputEdit {
    Insert(String),
    Replace(String),
    Backspace,
    Clear,
}
impl fmt::Debug for InputEdit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Insert(_) => "Insert(..)",
            Self::Replace(_) => "Replace(..)",
            Self::Backspace => "Backspace",
            Self::Clear => "Clear",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Element, Kind};
    #[test]
    fn masked_contract_and_mapping_keep_only_count_and_redacted_edits() {
        let field = Element::masked_input(12, "Password", |edit| edit).map(|edit| ("child", edit));
        assert!(matches!(field.kind, Kind::MaskedInput { length: 12, .. }));
        assert!(field.input.is_none());
        let result = field.edit.unwrap()(InputEdit::Insert("fixture-value".into()));
        assert_eq!(result, ("child", InputEdit::Insert("fixture-value".into())));
        assert!(!format!("{result:?}").contains("fixture-value"));
        assert!(matches!(
            Element::masked_input(usize::MAX, "", |edit| edit).kind,
            Kind::MaskedInput { length: 4096, .. }
        ));
    }
}
