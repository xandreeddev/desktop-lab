//! Presentation ordering policy: a later explicit palette choice wins over
//! earlier wallpaper color work. No decoding or persistence happens here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Revision(u64);
#[derive(Clone)]
pub struct Extraction {
    request: u64,
    pub path: String,
}
#[derive(Default)]
pub struct Intent {
    manual: Revision,
    request: u64,
}
impl Intent {
    pub fn explicit(&mut self) {
        self.manual.0 += 1;
        self.request += 1;
    }
    pub fn wallpaper_colors(&mut self) -> Revision {
        self.request += 1;
        self.manual
    }
    pub fn extraction(&mut self, path: String) -> Extraction {
        self.explicit();
        Extraction {
            request: self.request,
            path,
        }
    }
    pub fn accepts_wallpaper(&self, revision: Revision) -> bool {
        self.manual == revision
    }
    pub fn accepts_extraction(&self, extraction: &Extraction, current_path: &str) -> bool {
        extraction.request == self.request && extraction.path == current_path
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_choices_supersede_automatic_colors_and_stale_extractions() {
        let mut intent = Intent::default();
        let first = intent.wallpaper_colors();
        let second = intent.wallpaper_colors();
        assert!(intent.accepts_wallpaper(first));
        assert!(intent.accepts_wallpaper(second));
        let extraction = intent.extraction("selected".into());
        assert!(!intent.accepts_wallpaper(first));
        assert!(intent.accepts_extraction(&extraction, "selected"));
        assert!(!intent.accepts_extraction(&extraction, "new selection"));
        intent.explicit();
        assert!(!intent.accepts_extraction(&extraction, "selected"));
        let extraction = intent.extraction("selected".into());
        intent.wallpaper_colors();
        assert!(!intent.accepts_extraction(&extraction, "selected"));
    }
}
