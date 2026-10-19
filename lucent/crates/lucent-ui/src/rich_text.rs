//! Inline flow uses the same font advances as ordinary text. Links are opaque
//! client identities, with one focus target and separate wrapped pointer areas.
use super::*;

struct Run {
    span: usize,
    text: String,
    x: f32,
    width: f32,
}
struct Line {
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
fn add(
    line: &mut Line,
    spans: &[TextSpan],
    fonts: &[crate::text::Font],
    face: usize,
    size: f32,
    span: usize,
    value: &str,
) {
    let font = font(fonts, spans, span, face);
    if let Some(run) = line.runs.last_mut().filter(|run| run.span == span) {
        let previous = run.width;
        run.text.push_str(value);
        run.width = text::width(font, &run.text, size);
        line.width += run.width - previous;
    } else {
        let width = text::width(font, value, size);
        line.runs.push(Run {
            span,
            text: value.into(),
            x: line.width,
            width,
        });
        line.width += width;
    }
    let metrics = text::line_metrics(font, size);
    let descent = (line.height - line.baseline).max(metrics.height - metrics.baseline);
    line.baseline = line.baseline.max(metrics.baseline);
    line.height = line.baseline + descent;
}
fn line(fonts: &[crate::text::Font], face: usize, size: f32) -> Line {
    let metrics = text::line_metrics(fonts.get(face).unwrap_or(&fonts[0]), size);
    Line {
        runs: vec![],
        width: 0.,
        baseline: metrics.baseline,
        height: metrics.height,
    }
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
    for (index, span) in spans.iter().enumerate() {
        let mut boundaries = TextSelection::boundaries(&span.text);
        let mut start = boundaries.next().unwrap_or(0);
        for end in boundaries {
            clusters.push((index, &span.text[start..end]));
            start = end;
        }
    }
    let mut lines = vec![line(fonts, face, size)];
    let mut index = 0;
    let mut truncated = false;
    while index < clusters.len() {
        let (_, value) = clusters[index];
        if value == "\n" || value == "\r\n" {
            if lines.len() == limit {
                truncated = index + 1 < clusters.len();
                break;
            }
            lines.push(line(fonts, face, size));
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
        for &(span, value) in &clusters[index..end] {
            add(&mut word, spans, fonts, face, size, span, value);
        }
        if !whitespace
            && lines.last().unwrap().width > 0.
            && lines.last().unwrap().width + word.width > width
        {
            if lines.len() == limit {
                truncated = true;
                break;
            }
            lines.push(line(fonts, face, size));
        }
        for &(span, value) in &clusters[index..end] {
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
                lines.push(line(fonts, face, size));
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
            let next = text::width(font(fonts, spans, run.span, face), &run.text, size);
            last.width -= run.width - next;
            run.width = next;
            if run.text.is_empty() {
                last.runs.pop();
            }
        }
        add(last, spans, fonts, face, size, span, "…");
    }
    Flow {
        width: lines.iter().map(|line| line.width).fold(0., f32::max),
        height: lines.iter().map(|line| line.height).sum(),
        lines,
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
            let metrics =
                text::line_metrics(fonts.get(face).unwrap_or(&fonts[0]), e.style.font_size);
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
            scene.paint.push(Paint::Text {
                rect,
                clip: clip.clone(),
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
