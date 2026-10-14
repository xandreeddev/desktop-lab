//! Bounded notification XML subset. This is content policy, not HTML rendering.
//! Unsupported tags have no authority; malformed nesting falls back to text.
use lucent_domain::ExternalUri;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BodyStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub link: Option<ExternalUri>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodySpan {
    pub text: String,
    pub style: BodyStyle,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BodyBlock {
    Text(Vec<BodySpan>),
    Image { source: String, alt: String },
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotificationBody {
    pub blocks: Vec<BodyBlock>,
}
const DEPTH_LIMIT: usize = 16;
const PART_LIMIT: usize = 128;
const IMAGE_LIMIT: usize = 4;

fn entities(value: &str) -> String {
    let mut result = String::new();
    let mut rest = value;
    while let Some(at) = rest.find('&') {
        result.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|&end| end <= 16) else {
            result.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|v| u32::from_str_radix(v, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|v| v.parse().ok()))
                .and_then(char::from_u32)
                .filter(|c| !c.is_control() || matches!(c, '\n' | '\t')),
        };
        if let Some(c) = decoded {
            result.push(c);
        } else {
            result.push_str(&rest[..=end]);
        }
        rest = &rest[end + 1..];
    }
    result.push_str(rest);
    result
}
fn attributes(mut rest: &str) -> Option<Vec<(&str, String)>> {
    let mut attrs = vec![];
    while !rest.trim().is_empty() {
        rest = rest.trim_start();
        let end = rest.find(|c: char| c.is_whitespace() || c == '=')?;
        let name = &rest[..end];
        if name.is_empty() || attrs.iter().any(|(key, _)| *key == name) || attrs.len() >= 8 {
            return None;
        }
        rest = rest[end..].trim_start().strip_prefix('=')?.trim_start();
        let quote = rest.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
        rest = &rest[1..];
        let end = rest.find(quote)?;
        attrs.push((name, entities(&rest[..end])));
        rest = &rest[end + 1..];
    }
    Some(attrs)
}
fn push(blocks: &mut Vec<BodyBlock>, value: &str, style: &BodyStyle) {
    if value.is_empty() {
        return;
    }
    if !matches!(blocks.last(), Some(BodyBlock::Text(_))) {
        blocks.push(BodyBlock::Text(vec![]));
    }
    let Some(BodyBlock::Text(spans)) = blocks.last_mut() else {
        unreachable!()
    };
    if let Some(previous) = spans.last_mut().filter(|span| span.style == *style) {
        previous.text.push_str(value);
    } else {
        spans.push(BodySpan {
            text: value.into(),
            style: style.clone(),
        });
    }
}
fn parse(value: &str) -> Option<NotificationBody> {
    let mut blocks = vec![];
    let mut stack: Vec<(&str, BodyStyle)> = vec![];
    let mut style = BodyStyle::default();
    let mut rest = value;
    let mut parts = 0;
    let mut images = 0;
    while let Some(at) = rest.find('<') {
        push(&mut blocks, &entities(&rest[..at]), &style);
        let end = rest[at..].find('>')? + at;
        let tag = rest[at + 1..end].trim();
        rest = &rest[end + 1..];
        parts += 1;
        if parts > PART_LIMIT {
            return None;
        }
        if let Some(name) = tag.strip_prefix('/') {
            let name = name.trim();
            if matches!(name, "b" | "i" | "u" | "a") {
                let (opening, previous) = stack.pop()?;
                if opening != name {
                    return None;
                }
                style = previous;
            }
            continue;
        }
        let tag = tag.strip_suffix('/').unwrap_or(tag).trim();
        let (name, attrs) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
        match name {
            "b" | "i" | "u" | "a" => {
                if stack.len() >= DEPTH_LIMIT {
                    return None;
                }
                let attrs = attributes(attrs)?;
                stack.push((name, style.clone()));
                match name {
                    "b" => style.bold = true,
                    "i" => style.italic = true,
                    "u" => style.underline = true,
                    "a" => {
                        style.link = attrs
                            .iter()
                            .find(|(key, _)| *key == "href")
                            .and_then(|(_, value)| ExternalUri::new(value.clone()).ok())
                    }
                    _ => unreachable!(),
                }
            }
            "img" => {
                let attrs = attributes(attrs)?;
                let alt = attrs
                    .iter()
                    .find(|(key, _)| *key == "alt")
                    .map(|(_, value)| value.clone())
                    .unwrap_or_else(|| "Image".into());
                let source = attrs
                    .iter()
                    .find(|(key, _)| *key == "src")
                    .map(|(_, value)| value.as_str())
                    .unwrap_or("");
                // Notification body images are local files, never remote fetches.
                if images < IMAGE_LIMIT
                    && (source.starts_with('/') || source.starts_with("file:///"))
                    && source.len() <= 2048
                    && !source.chars().any(char::is_control)
                {
                    images += 1;
                    blocks.push(BodyBlock::Image {
                        source: source.into(),
                        alt,
                    });
                } else {
                    push(&mut blocks, &alt, &style);
                }
            }
            _ => {}
        }
    }
    push(&mut blocks, &entities(rest), &style);
    if !stack.is_empty() {
        return None;
    }
    Some(NotificationBody { blocks })
}
/// The caller supplies the bounded raw body; no parser IO or renderer types.
pub fn notification_body(value: &str) -> NotificationBody {
    let mut end = value.len().min(super::BODY_LIMIT);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let value = &value[..end];
    parse(value).unwrap_or_else(|| NotificationBody {
        blocks: vec![BodyBlock::Text(vec![BodySpan {
            text: entities(value),
            style: BodyStyle::default(),
        }])],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_formatting_entities_and_links_are_content_values() {
        let body = notification_body(
            "Hello <b>bold <i>both</i></b> <u>line</u> <a href='https://example.test/?a=1&amp;b=2'>open</a> &lt;b&gt; &#233;",
        );
        let BodyBlock::Text(spans) = &body.blocks[0] else {
            panic!()
        };
        assert!(
            spans
                .iter()
                .any(|span| span.text == "both" && span.style.bold && span.style.italic)
        );
        assert!(spans.iter().any(|span| span.style.underline));
        assert_eq!(
            spans
                .iter()
                .find_map(|span| span.style.link.as_ref())
                .unwrap()
                .value(),
            "https://example.test/?a=1&b=2"
        );
        assert!(spans.last().unwrap().text.ends_with("<b> é"));
    }
    #[test]
    fn untrusted_uris_and_remote_images_have_no_external_authority() {
        let body = notification_body(
            "<a href='javascript:x'>safe label</a><img src='https://example.test/a.png' alt='remote image'/><img src='/tmp/local.png' alt='local'/>",
        );
        let BodyBlock::Text(spans) = &body.blocks[0] else {
            panic!()
        };
        assert!(spans.iter().all(|span| span.style.link.is_none()));
        assert_eq!(
            spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
            "safe labelremote image"
        );
        assert!(
            matches!(&body.blocks[1],BodyBlock::Image{source,alt} if source=="/tmp/local.png"&&alt=="local")
        );
    }
    #[test]
    fn malformed_or_excessive_markup_falls_back_without_losing_content() {
        for value in [
            "<b>unfinished",
            "<b><i>wrong</b></i>",
            "<a href=unquoted>label</a>",
        ] {
            let body = notification_body(value);
            let BodyBlock::Text(spans) = &body.blocks[0] else {
                panic!()
            };
            assert_eq!(spans[0].text, value);
            assert_eq!(spans[0].style, BodyStyle::default());
        }
        let body = notification_body(&"<b>".repeat(20));
        assert!(
            matches!(&body.blocks[0],BodyBlock::Text(spans) if spans[0].style==BodyStyle::default())
        );
        let body = notification_body(&"é".repeat(5000));
        assert!(
            matches!(&body.blocks[0],BodyBlock::Text(spans) if spans[0].text.len()==super::super::BODY_LIMIT)
        );
    }
}
