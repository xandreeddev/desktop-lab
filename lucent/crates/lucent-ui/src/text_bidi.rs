//! Preserve hard-paragraph resolution while reordering individual soft lines.
use super::Direction;
use std::{ops::Range, sync::Arc};

/// Resolved visual run order for one displayed line. The UI creates this from
/// the complete hard paragraph, including embeddings and isolates. Its offsets
/// are relative to that displayed line; renderers must not resolve it afresh.
#[derive(Clone, PartialEq)]
pub struct ResolvedBidiLine {
    source: Arc<str>,
    runs: Arc<[(Range<usize>, Direction)]>,
}

impl std::fmt::Debug for ResolvedBidiLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The text cache already keys the actual row bytes separately.
        f.debug_struct("ResolvedBidiLine")
            .field("bytes", &self.source.len())
            .field("runs", &self.runs)
            .finish()
    }
}

impl ResolvedBidiLine {
    /// Check the exact displayed bytes before reusing resolved order or a mask.
    pub fn is_for(&self, line: &str) -> bool {
        self.source.as_ref() == line
    }
    pub(crate) fn from_range(
        bidi: &unicode_bidi::BidiInfo<'_>,
        range: Range<usize>,
        source: Arc<str>,
    ) -> Option<Self> {
        if range.is_empty() || bidi.text.get(range.clone()) != Some(source.as_ref()) {
            return None;
        }
        let index = bidi
            .paragraphs
            .partition_point(|paragraph| paragraph.range.end <= range.start);
        let paragraph = bidi.paragraphs.get(index).filter(|paragraph| {
            paragraph.range.start <= range.start && paragraph.range.end >= range.end
        })?;
        Some(Self {
            source,
            runs: line_runs(bidi, paragraph, range).into(),
        })
    }

    pub(super) fn runs(&self, line: &str) -> Option<&[(Range<usize>, Direction)]> {
        self.is_for(line).then_some(self.runs.as_ref())
    }

    pub(super) fn is_ltr(&self, line: &str) -> bool {
        self.runs(line).is_some_and(|runs| {
            runs.len() == 1 && runs[0].0 == (0..line.len()) && runs[0].1 == Direction::LeftToRight
        })
    }
}

pub(super) fn standalone_runs(
    line: &str,
    direction: Option<Direction>,
) -> Vec<(Range<usize>, Direction)> {
    let bidi = unicode_bidi::BidiInfo::new(
        line,
        direction.map(|direction| match direction {
            Direction::LeftToRight => unicode_bidi::Level::ltr(),
            Direction::RightToLeft => unicode_bidi::Level::rtl(),
        }),
    );
    bidi.paragraphs
        .iter()
        .flat_map(|paragraph| {
            let start = paragraph.range.start;
            line_runs(&bidi, paragraph, paragraph.range.clone())
                .into_iter()
                .map(move |(range, direction)| (range.start + start..range.end + start, direction))
        })
        .collect()
}

fn line_runs(
    bidi: &unicode_bidi::BidiInfo<'_>,
    paragraph: &unicode_bidi::ParagraphInfo,
    range: Range<usize>,
) -> Vec<(Range<usize>, Direction)> {
    // visual_runs clones its BidiInfo levels. Supply only this already-resolved
    // line, retaining the original hard-paragraph base level and character
    // classes. Upstream still performs L1/L2; work is bounded by line bytes,
    // rather than copying the complete document once per wrapped row.
    let local = unicode_bidi::BidiInfo {
        text: &bidi.text[range.clone()],
        original_classes: bidi.original_classes[range.clone()].to_vec(),
        levels: bidi.levels[range.clone()].to_vec(),
        paragraphs: vec![unicode_bidi::ParagraphInfo {
            range: 0..range.len(),
            level: paragraph.level,
        }],
    };
    let (levels, runs) = local.visual_runs(&local.paragraphs[0], 0..range.len());
    runs.into_iter()
        .map(|run| {
            let direction = if levels[run.start].is_rtl() {
                Direction::RightToLeft
            } else {
                Direction::LeftToRight
            };
            (run, direction)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn line_local_l1_l2_matches_upstream_full_document_reference() {
        for source in [
            "abc אבג xyz",
            "abc \u{202e}abc DEF\u{202c} tail",
            "abc \u{2067}xyz אבג\u{2069} tail",
            "abc אבג\tדהו xyz",
            "first\r\nאבג xyz\nnext",
        ] {
            let bidi = unicode_bidi::BidiInfo::new(source, None);
            for paragraph in &bidi.paragraphs {
                let mut boundaries = source[paragraph.range.clone()]
                    .char_indices()
                    .map(|(at, _)| paragraph.range.start + at)
                    .collect::<Vec<_>>();
                boundaries.push(paragraph.range.end);
                for (first, start) in boundaries.iter().enumerate() {
                    for end in boundaries.iter().skip(first + 1) {
                        let range = *start..*end;
                        let actual = ResolvedBidiLine::from_range(
                            &bidi,
                            range.clone(),
                            Arc::from(&source[range.clone()]),
                        )
                        .unwrap();
                        let (levels, runs) = bidi.visual_runs(paragraph, range.clone());
                        let expected = runs
                            .into_iter()
                            .map(|run| {
                                let direction = if levels[run.start].is_rtl() {
                                    Direction::RightToLeft
                                } else {
                                    Direction::LeftToRight
                                };
                                (run.start - range.start..run.end - range.start, direction)
                            })
                            .collect::<Vec<_>>();
                        assert_eq!(
                            actual.runs.as_ref(),
                            expected.as_slice(),
                            "{source:?} {range:?}"
                        );
                    }
                }
            }
        }
    }
}
