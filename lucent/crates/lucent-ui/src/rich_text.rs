//! Inline flow uses the same font advances as ordinary text. Links are opaque
//! client identities, with one focus target and separate wrapped pointer areas.
use super::*;

struct Run {
    span: usize,
    text: String,
    x: f32,
    width: f32,
    source: std::ops::Range<usize>,
    direction: Option<text::Direction>,
    shared: Option<(f32, f32)>,
}
struct Line {
    origin: usize,
    runs: Vec<Run>,
    width: f32,
    baseline: f32,
    height: f32,
}
pub(super) struct Flow {
    lines: Vec<Line>,
    pub width: f32,
    pub height: f32,
}
fn font<'a>(
    fonts: &'a [crate::text::Font],
    spans: &[TextSpan],
    span: usize,
    face: usize,
) -> &'a crate::text::Font {
    fonts
        .get(spans[span].face.unwrap_or(face))
        .unwrap_or(&fonts[0])
}
#[allow(clippy::too_many_arguments)]
fn add(
    line: &mut Line,
    spans: &[TextSpan],
    fonts: &[crate::text::Font],
    face: usize,
    size: f32,
    span: usize,
    value: &str,
    source: usize,
) {
    let font = font(fonts, spans, span, face);
    if let Some(run) = line.runs.last_mut().filter(|run| run.span == span) {
        let previous = run.width;
        run.text.push_str(value);
        run.source.end = source + value.len();
        run.width = text::width(font, &run.text, size);
        line.width += run.width - previous;
    } else {
        let width = text::width(font, value, size);
        line.runs.push(Run {
            span,
            source: source..source + value.len(),
            direction: None,
            shared: None,
            text: value.into(),
            x: line.width,
            width,
        });
        line.width += width;
    }
    let metrics = text::run_metrics(
        font,
        line.runs.last().map_or(value, |run| run.text.as_str()),
        size,
        None,
    );
    let descent = (line.height - line.baseline).max(metrics.height - metrics.baseline);
    line.baseline = line.baseline.max(metrics.baseline);
    line.height = line.baseline + descent;
}
fn line(fonts: &[crate::text::Font], face: usize, size: f32) -> Line {
    let metrics = text::line_metrics(fonts.get(face).unwrap_or(&fonts[0]), size);
    Line {
        origin: 0,
        runs: vec![],
        width: 0.,
        baseline: metrics.baseline,
        height: metrics.height,
    }
}
fn line_at(fonts: &[text::Font], face: usize, size: f32, origin: usize) -> Line {
    let mut line = line(fonts, face, size);
    line.origin = origin;
    line
}

pub(super) fn flow(
    spans: &[TextSpan],
    fonts: &[crate::text::Font],
    face: usize,
    size: f32,
    width: f32,
    limit: usize,
) -> Flow {
    if fonts.is_empty() || !size.is_normal() || size <= 0. || limit == 0 {
        return Flow {
            lines: vec![],
            width: 0.,
            height: 0.,
        };
    }
    let width = if width.is_finite() {
        width.max(0.)
    } else {
        f32::MAX
    };
    let width = width + width.abs().max(1.) * f32::EPSILON * 4.;
    // Preserve grapheme boundaries, including combining marks, when a long
    // word cannot fit. Style boundaries remain explicit client choices.
    let mut clusters = vec![];
    let mut source = String::new();
    for (index, span) in spans.iter().enumerate() {
        let mut boundaries = TextSelection::boundaries(&span.text);
        let mut start = boundaries.next().unwrap_or(0);
        for end in boundaries {
            clusters.push((index, &span.text[start..end], source.len() + start));
            start = end;
        }
        source.push_str(&span.text);
    }
    let mut lines = vec![line(fonts, face, size)];
    let mut index = 0;
    let mut truncated = false;
    while index < clusters.len() {
        let (_, value, source_at) = clusters[index];
        if value == "\n" || value == "\r\n" {
            if lines.len() == limit {
                truncated = index + 1 < clusters.len();
                break;
            }
            lines.push(line_at(fonts, face, size, source_at + value.len()));
            index += 1;
            continue;
        }
        // Try a whole word before breaking it. A style boundary inside the
        // word must not itself cause a wrap.
        let whitespace = value.chars().all(char::is_whitespace);
        let end = if whitespace {
            index + 1
        } else {
            (index + 1..clusters.len())
                .find(|&i| clusters[i].1.chars().any(char::is_whitespace))
                .unwrap_or(clusters.len())
        };
        let mut word = line(fonts, face, size);
        for &(span, value, at) in &clusters[index..end] {
            add(&mut word, spans, fonts, face, size, span, value, at);
        }
        if !whitespace
            && lines.last().unwrap().width > 0.
            && lines.last().unwrap().width + word.width > width
        {
            if lines.len() == limit {
                truncated = true;
                break;
            }
            lines.push(line_at(fonts, face, size, clusters[index].2));
        }
        for &(span, value, at) in &clusters[index..end] {
            if lines.last().unwrap().width == 0. && value.chars().all(char::is_whitespace) {
                continue;
            }
            let current = lines.last_mut().unwrap();
            let font = font(fonts, spans, span, face);
            let kern = current
                .runs
                .last()
                .filter(|run| run.span == span)
                .and_then(|run| run.text.chars().next_back())
                .zip(value.chars().next())
                .and_then(|(a, b)| font.horizontal_kern(a, b, size))
                .unwrap_or(0.);
            let advance =
                if value.is_ascii() && current.runs.last().is_none_or(|run| run.text.is_ascii()) {
                    text::width(font, value, size) + kern
                } else if let Some(run) = current.runs.last().filter(|run| run.span == span) {
                    text::width(font, &format!("{}{value}", run.text), size) - run.width
                } else {
                    text::width(font, value, size)
                };
            if current.width > 0. && current.width + advance > width {
                if lines.len() == limit {
                    truncated = true;
                    break;
                }
                lines.push(line_at(fonts, face, size, at));
                if value.chars().all(char::is_whitespace) {
                    continue;
                }
            }
            add(
                lines.last_mut().unwrap(),
                spans,
                fonts,
                face,
                size,
                span,
                value,
                at,
            );
        }
        if truncated {
            break;
        }
        index = end;
    }
    if truncated && !spans.is_empty() {
        let last = lines.last_mut().unwrap();
        let span = last.runs.last().map_or(0, |run| run.span);
        let ellipsis = text::width(font(fonts, spans, span, face), "…", size);
        while last.width + ellipsis > width && !last.runs.is_empty() {
            let run = last.runs.last_mut().unwrap();
            let at = TextSelection::boundaries(&run.text)
                .take_while(|&at| at < run.text.len())
                .last()
                .unwrap_or(0);
            run.text.truncate(at);
            run.source.end = run.source.start + at;
            let next = text::width(font(fonts, spans, run.span, face), &run.text, size);
            last.width -= run.width - next;
            run.width = next;
            if run.text.is_empty() {
                last.runs.pop();
            }
        }
        let at = last.runs.last().map_or(last.origin, |run| run.source.end);
        source.truncate(at);
        source.push('…');
        add(last, spans, fonts, face, size, span, "…", at);
    }
    let bidi = unicode_bidi::BidiInfo::new(&source, None);
    for line in &mut lines {
        reorder(line, spans, fonts, face, size, &bidi);
    }
    Flow {
        width: lines.iter().map(|line| line.width).fold(0., f32::max),
        height: lines.iter().map(|line| line.height).sum(),
        lines,
    }
}

fn reorder(
    line: &mut Line,
    spans: &[TextSpan],
    fonts: &[text::Font],
    default_face: usize,
    size: f32,
    bidi: &unicode_bidi::BidiInfo<'_>,
) {
    if bidi.text.is_ascii() {
        return; // Keep established ASCII style-boundary kerning geometry.
    }
    let Some(start) = line.runs.iter().map(|run| run.source.start).min() else {
        return;
    };
    let end = line.runs.iter().map(|run| run.source.end).max().unwrap();
    let Some(paragraph) = bidi
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.range.start <= start && paragraph.range.end >= end)
    else {
        return;
    };
    let (levels, visual) = bidi.visual_runs(paragraph, start..end);
    let old = std::mem::take(&mut line.runs);
    line.width = 0.;
    let base = text::line_metrics(fonts.get(default_face).unwrap_or(&fonts[0]), size);
    line.height = base.height;
    line.baseline = base.baseline;
    for visual in visual {
        let direction = Some(if levels[visual.start].is_rtl() {
            text::Direction::RightToLeft
        } else {
            text::Direction::LeftToRight
        });
        let mut parts = old
            .iter()
            .filter_map(|run| {
                let start = run.source.start.max(visual.start);
                let end = run.source.end.min(visual.end);
                (end > start).then_some((run.span, start..end))
            })
            .collect::<Vec<_>>();
        if levels[visual.start].is_rtl() {
            parts.reverse();
        }
        // Styles do not divide a shaping buffer. Contiguous spans selecting the
        // same font share one complete buffer, including cross-span GSUB/GPOS.
        let face = |span: usize| {
            let index = spans[span].face.unwrap_or(default_face);
            if index < fonts.len() { index } else { 0 }
        };
        let mut first = 0;
        while first < parts.len() {
            let mut last = first + 1;
            while last < parts.len() && face(parts[last].0) == face(parts[first].0) {
                last += 1;
            }
            let group = &parts[first..last];
            let from = group.iter().map(|(_, range)| range.start).min().unwrap();
            let to = group.iter().map(|(_, range)| range.end).max().unwrap();
            let value = &bidi.text[from..to];
            let font = font(fonts, spans, group[0].0, default_face);
            let shaped = text::shape_directed(font, value, size, direction);
            let metrics = text::run_metrics(font, value, size, direction);
            let descent = (line.height - line.baseline).max(metrics.height - metrics.baseline);
            line.baseline = line.baseline.max(metrics.baseline);
            line.height = line.baseline + descent;
            let origin = line.width;
            let mut fragments = vec![];
            for (span, range) in group {
                // A style starting inside a grapheme does not split that
                // grapheme. Its first scalar's style owns its visual coverage.
                let owned = shaped
                    .segments
                    .iter()
                    .filter(|(segment, _, _)| {
                        range.start <= from + segment.start && from + segment.start < range.end
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                for (a, b) in text::selection_fragments(&owned, 0..value.len()) {
                    fragments.push((a, b, *span, range.clone()));
                }
            }
            fragments.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (a, b, span, source) in fragments {
                line.runs.push(Run {
                    span,
                    text: value.to_owned(),
                    x: origin + a,
                    width: b - a,
                    source,
                    direction,
                    shared: Some((origin, shaped.width)),
                });
            }
            line.width += shaped.width;
            first = last;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn paint<M: Clone>(
    scene: &mut Scene<M>,
    e: &Element<M>,
    spans: &[TextSpan],
    fonts: &[crate::text::Font],
    inner: Rect,
    clip: Clip,
    alpha: f32,
    interaction: &Interaction<M>,
    now: f64,
    scrolls: &Arc<[Arc<ScrollHit<M>>]>,
    focus_clips: &Arc<[FocusClip]>,
    id: &str,
) {
    let result = flow(
        spans,
        fonts,
        e.style.font_face,
        e.style.font_size,
        inner.w,
        e.style.max_lines.unwrap_or(usize::MAX),
    );
    let mut y = inner.y;
    let mut links: Vec<(&str, Vec<Rect>)> = vec![];
    for line in result.lines {
        let x = inner.x + align_offset(e.style.align, inner.w - line.width);
        for run in line.runs {
            let span = &spans[run.span];
            let face = span.face.unwrap_or(e.style.font_face);
            let metrics = text::run_metrics_with_layout(
                fonts.get(face).unwrap_or(&fonts[0]),
                &run.text,
                e.style.font_size,
                text::LayoutOptions {
                    direction: run.direction,
                    ..Default::default()
                },
            );
            let rect = Rect::new(
                x + run.x,
                y + line.baseline - metrics.baseline,
                run.width,
                metrics.height,
            );
            let color = span.color.unwrap_or(e.style.foreground);
            let link = span.link.as_deref().filter(|_| e.link.is_some());
            if let Some(link) = link {
                let index = links
                    .iter()
                    .position(|(key, _)| *key == link)
                    .unwrap_or_else(|| {
                        links.push((link, vec![]));
                        links.len() - 1
                    });
                links[index].1.push(rect);
                let link_id = format!("{id}/link/{link}");
                let focused =
                    interaction.focus_visible && interaction.focus.as_deref() == Some(&link_id);
                let amount = if focused {
                    1.
                } else {
                    interaction.hover_amount(&link_id, now)
                };
                if e.style.hover_feedback && amount > 0. && e.style.hover_color.3 > 0. {
                    scene.paint.push(Paint::Shape {
                        rect,
                        clip: clip.clone(),
                        color: e
                            .style
                            .hover_color
                            .alpha(e.style.hover_color.3 * alpha * amount),
                        radius: 0.,
                    });
                }
                if focused && e.style.focus_outline && e.style.focus_width > 0. {
                    scene.paint.push(Paint::Outline {
                        rect,
                        clip: clip.clone(),
                        color: e.style.focus_color.alpha(e.style.focus_color.3 * alpha),
                        radius: 0.,
                        width: e.style.focus_width,
                    });
                }
            }
            let (text_rect, text_clip) = if let Some((origin, width)) = run.shared {
                let full = Rect::new(x + origin, rect.y, width, rect.h);
                let left = if run.x == origin {
                    clip.bounds.x
                } else {
                    rect.x
                };
                let right = if run.x + run.width
                    >= origin + width - f32::EPSILON * width.abs().max(1.) * 4.
                {
                    clip.bounds.x + clip.bounds.w
                } else {
                    rect.x + rect.w
                };
                (
                    full,
                    clip.intersect(Rect::new(
                        left,
                        clip.bounds.y,
                        (right - left).max(0.),
                        clip.bounds.h,
                    )),
                )
            } else {
                (rect, clip.clone())
            };
            scene.paint.push(Paint::Text {
                layout: text::LayoutOptions {
                    direction: run.direction,
                    metrics: Some(metrics),
                    ..Default::default()
                },
                rect: text_rect,
                clip: text_clip,
                text: run.text,
                size: e.style.font_size,
                face,
                color: color.alpha(color.3 * alpha),
                align: Align::Start,
            });
            if span.underline
                && e.style.text_underline_width.is_finite()
                && e.style.text_underline_width > 0.
            {
                scene.paint.push(Paint::Shape {
                    rect: Rect::new(
                        rect.x,
                        y + line.baseline + (-metrics.descent * 0.5),
                        rect.w,
                        e.style.text_underline_width,
                    ),
                    clip: clip.clone(),
                    color: color.alpha(color.3 * alpha),
                    radius: 0.,
                });
            }
        }
        y += line.height;
    }
    for (key, parts) in links {
        if parts.is_empty() {
            continue;
        }
        let rect = parts.iter().skip(1).fold(parts[0], |a, b| {
            Rect::new(
                a.x.min(b.x),
                a.y.min(b.y),
                (a.x + a.w).max(b.x + b.w) - a.x.min(b.x),
                (a.y + a.h).max(b.y + b.h) - a.y.min(b.y),
            )
        });
        for part in &parts {
            scene.regions.push(clip.intersect(*part));
        }
        scene.hits.push(Hit {
            id: format!("{id}/link/{key}"),
            rect,
            parts: Some(parts.into()),
            clip: clip.clone(),
            radius: 0.,
            click: Some(e.link.as_ref().unwrap()(key)),
            secondary_click: None,
            middle_click: None,
            wheel: None,
            submit: None,
            hover: None,
            input: None,
            edit: None,
            text_edit: None,
            text: None,
            value: String::new(),
            drag: None,
            slider: None,
            scroll: None,
            scroll_ancestors: scrolls.clone(),
            focus_clips: focus_clips.clone(),
            autofocus: e.autofocus,
            hover_transition: e.style.hover_transition,
            tab_stop: e.style.tab_stop,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fonts() -> Arc<Vec<crate::text::Font>> {
        Arc::new(vec![
            crate::text::Font::from_bytes(
                include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf") as &[u8],
                fontdue::FontSettings::default(),
            )
            .unwrap(),
        ])
    }
    #[test]
    fn inline_style_boundaries_do_not_split_words_and_graphemes_stay_whole() {
        let fonts = fonts();
        let spans = [
            TextSpan::new("one "),
            TextSpan::new("word").underline(true),
            TextSpan::new("tail"),
        ];
        let width = text::width(&fonts[0], "wordtail", 14.) + 0.01;
        let result = flow(&spans, &fonts, 0, 14., width, 8);
        assert_eq!(result.lines.len(), 2);
        assert_eq!(
            result.lines[1]
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            "wordtail"
        );
        let spans = [TextSpan::new("e\u{301}e\u{301}e\u{301}")];
        let result = flow(&spans, &fonts, 0, 14., 1., 8);
        assert_eq!(result.lines.len(), 3);
        assert!(
            result
                .lines
                .iter()
                .all(|line| line.runs[0].text == "e\u{301}")
        );
        let empty = flow(&spans, &fonts, 0, 14., 50., 0);
        assert_eq!((empty.width, empty.height), (0., 0.));
    }
    #[test]
    fn wrapped_link_has_one_keyboard_target_and_disjoint_pointer_coverage() {
        let fonts = fonts();
        let layout = Layout::new(fonts.clone());
        let tree = Element::rich_text(vec![
            TextSpan::new("start "),
            TextSpan::new("a long wrapped link")
                .link("details")
                .underline(true),
            TextSpan::new(" tail"),
        ])
        .id("body")
        .on_link(str::to_owned)
        .width(Length::Fill)
        .font(14.)
        .wrap(20);
        let mut input = Interaction::default();
        let scene = layout.build(&tree, 90., 240., &input, 0.);
        assert_eq!(scene.hits.len(), 1);
        let hit = &scene.hits[0];
        assert!(hit.parts.as_ref().unwrap().len() > 1);
        let first = hit.parts.as_ref().unwrap()[0];
        input.press(&scene, first.x + first.w / 2., first.y + first.h / 2.);
        assert_eq!(
            input.release(first.x + first.w / 2., first.y + first.h / 2.),
            Some("details".into())
        );
        input.key(&scene, &Key::Tab);
        assert_eq!(input.key(&scene, &Key::Enter), ["details"]);
        assert!(hit.parts.as_ref().unwrap().iter().any(|part| part.x > 0.));
        assert!(!hit.contains(0., first.y + first.h / 2.));
        let changed = layout.build(&tree, 60., 240., &input, 0.);
        input.press(&scene, first.x + first.w / 2., first.y + first.h / 2.);
        input.synchronize(&changed);
        assert!(
            input
                .release(first.x + first.w / 2., first.y + first.h / 2.)
                .is_none()
        );
        let mapped = tree.map(|key| ("child", key));
        let scene = layout.build(&mapped, 90., 240., &Interaction::default(), 0.);
        assert_eq!(scene.hits[0].click, Some(("child", "details".into())));
    }
    #[test]
    fn long_text_is_painted_per_line_and_truncation_does_not_remove_earlier_links() {
        let fonts = fonts();
        let layout = Layout::new(fonts.clone());
        let spans = vec![
            TextSpan::new("open").link("open"),
            TextSpan::new(format!("\n{}", "more content\n".repeat(100))),
        ];
        let tree = Element::rich_text(spans.clone())
            .on_link(str::to_owned)
            .font(14.)
            .width(Length::Fill);
        let scene = layout.build(&tree, 180., 4000., &Interaction::default(), 0.);
        assert!(
            scene
                .paint
                .iter()
                .filter(|paint| matches!(paint, Paint::Text { .. }))
                .count()
                > 100
        );
        assert!(
            scene
                .paint
                .iter()
                .all(|paint| !matches!(paint,Paint::Text{rect,..} if rect.h>100.))
        );
        let result = flow(&spans, &fonts, 0, 14., 180., 2);
        assert_eq!(result.lines.len(), 2);
        assert!(
            result
                .lines
                .last()
                .unwrap()
                .runs
                .last()
                .unwrap()
                .text
                .ends_with('…')
        );
        assert_eq!(result.lines[0].runs[0].text, "open");
    }
    #[test]
    fn a_visible_bounding_box_gap_is_not_a_reachable_keyboard_link() {
        let layout = Layout::new(fonts());
        let tree =
            Element::rich_text(vec![TextSpan::new("link").link("details")]).on_link(str::to_owned);
        let mut scene = layout.build(&tree, 120., 100., &Interaction::default(), 0.);
        let hit = &mut scene.hits[0];
        hit.rect = Rect::new(0., 0., 100., 60.);
        hit.parts = Some(Arc::from([
            Rect::new(70., 0., 30., 20.),
            Rect::new(0., 40., 30., 20.),
        ]));
        hit.clip = Rect::new(0., 0., 40., 20.).into();
        let mut input = Interaction::default();
        input.key(&scene, &Key::Tab);
        assert!(input.focus.is_none());
        scene.hits[0].clip = Rect::new(0., 40., 40., 20.).into();
        input.key(&scene, &Key::Tab);
        assert!(input.focus.is_some());
    }
}

#[cfg(test)]
mod international_paragraphs {
    use super::*;
    fn fonts() -> Arc<Vec<text::Font>> {
        Arc::new(vec![
            text::Font::from_bytes(
                include_bytes!("../../../apps/lucent-desktop/assets/LucentSans.ttf").as_slice(),
                Default::default(),
            )
            .unwrap(),
        ])
    }
    #[test]
    fn paragraph_bidi_reorders_across_style_and_link_boundaries() {
        let fonts = fonts();
        let spans = [
            TextSpan::new("abc "),
            TextSpan::new("אב").link("first"),
            TextSpan::new("ג").underline(true),
            TextSpan::new(" def"),
        ];
        let result = flow(&spans, &fonts, 0, 20., 600., 10);
        let line = &result.lines[0];
        let second = line.runs.iter().find(|run| run.span == 2).unwrap();
        let first = line.runs.iter().find(|run| run.span == 1).unwrap();
        assert!(
            second.x < first.x,
            "style runs must share one paragraph resolution"
        );
        assert_eq!(first.direction, Some(text::Direction::RightToLeft));
        assert!((line.width - text::width(&fonts[0], "abc אבג def", 20.)).abs() < 0.01);
        let scene = Layout::new(fonts).build(
            &Element::rich_text(spans.to_vec())
                .on_link(|_| ())
                .id("body")
                .size(600., 80.)
                .font(20.),
            600.,
            80.,
            &Interaction::default(),
            0.,
        );
        let first = scene
            .hits
            .iter()
            .find(|hit| hit.id == "body/link/first")
            .unwrap();
        assert!(first.parts.as_ref().unwrap().iter().all(|rect| rect.w > 0.));
    }
    #[test]
    fn arabic_styles_share_complete_shaping_buffer() {
        let fonts = fonts();
        let spans = [
            TextSpan::new("سل").color(Color::hex(0xff0000)),
            TextSpan::new("ام").color(Color::hex(0x0000ff)),
        ];
        let result = flow(&spans, &fonts, 0, 24., 600., 10);
        let expected = text::shape(&fonts[0], "سلام", 24.)
            .glyphs
            .iter()
            .map(|glyph| glyph.id)
            .collect::<Vec<_>>();
        let line = &result.lines[0];
        assert_eq!(line.runs.len(), 2);
        let mut edge = 0.;
        for run in &line.runs {
            let shaped = text::shape_directed(&fonts[0], &run.text, 24., run.direction);
            assert_eq!(
                shaped
                    .glyphs
                    .iter()
                    .map(|glyph| glyph.id)
                    .collect::<Vec<_>>(),
                expected,
                "all colors must use the same complete GSUB buffer"
            );
            assert!(
                (run.x - edge).abs() < 0.01,
                "style clips partition the full visual run"
            );
            edge = run.x + run.width;
            assert_eq!(run.shared, Some((0., line.width)));
        }
        assert!((edge - line.width).abs() < 0.01);
    }
    #[test]
    fn truncating_an_empty_later_line_preserves_earlier_rtl_paragraphs() {
        let fonts = fonts();
        let result = flow(&[TextSpan::new("אבג\n\nmore")], &fonts, 0, 20., 200., 2);
        assert_eq!(result.lines.len(), 2);
        assert_eq!(
            result.lines[0].runs[0].direction,
            Some(text::Direction::RightToLeft)
        );
        assert_eq!(result.lines[1].runs[0].text, "…");
    }
}
