//! Declarative inline text. Styles and link identities are presentation data.
use crate::Color;
#[derive(Clone, Debug, PartialEq)]
pub struct TextSpan {
    pub text: String,
    /// Registered application face; None inherits the element's face.
    pub face: Option<usize>,
    pub color: Option<Color>,
    pub underline: bool,
    /// Stable client action identity. The engine does not interpret it as a URI.
    pub link: Option<String>,
}
impl TextSpan {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            face: None,
            color: None,
            underline: false,
            link: None,
        }
    }
    pub fn face(mut self, face: usize) -> Self {
        self.face = Some(face);
        self
    }
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
    pub fn underline(mut self, enabled: bool) -> Self {
        self.underline = enabled;
        self
    }
    pub fn link(mut self, id: impl Into<String>) -> Self {
        self.link = Some(id.into());
        self
    }
}
