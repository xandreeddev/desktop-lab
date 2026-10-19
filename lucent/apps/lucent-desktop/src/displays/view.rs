//! Design-system recipes over borrowed draft state. No native calls in a view.
use super::*;
use lucent_design::{
    Theme,
    component::{displays as token, scroll},
    font, radius, space,
};
fn label(value: impl Into<String>, theme: Theme) -> Element<Message> {
    Element::text(value)
        .font(font::BODY)
        .color(theme.on_surface)
        .width(Length::Fill)
        .wrap(token::STATUS_LINES as usize)
}
fn button(
    value: impl Into<String>,
    id: impl Into<String>,
    message: Message,
    enabled: bool,
    theme: Theme,
) -> Element<Message> {
    theme
        .button(value, if enabled { message } else { Message::Ignore })
        .id(id)
        .width(Length::Fill)
        .tab_stop(enabled)
        .wrap(token::TITLE_LINES as usize)
        .opacity(if enabled {
            1.
        } else {
            lucent_design::opacity::MUTED
        })
}
fn mode_name(mode: DisplayMode) -> String {
    format!(
        "{} × {} · {:.2} Hz",
        mode.width(),
        mode.height(),
        f64::from(mode.millihertz()) / 1000.
    )
}
impl Displays {
    fn content(&self, theme: Theme) -> Vec<Element<Message>> {
        let Some(snapshot) = &self.snapshot else {
            return vec![label(
                if self.observation_error.is_empty() {
                    "Connecting to displays…"
                } else {
                    &self.observation_error
                },
                theme,
            )];
        };
        let mut content=vec![label("Edit the draft, then Apply. Keep saves it; Revert, closing this panel or a 15-second timeout restores the captured settings.",theme).font(font::CAPTION)];
        for output in &snapshot.outputs {
            let id = output.current.output.clone();
            let selected = self.selected.as_ref() == Some(&id);
            content.push(
                theme
                    .choice_button(id.as_str(), Message::Select(id.clone()), selected)
                    .id(format!("display-select-{}", id.as_str()))
                    .width(Length::Fill),
            );
        }
        let Some(id) = &self.selected else {
            return content;
        };
        let Some(row) = self.rows.iter().find(|r| &r.configuration.output == id) else {
            return content;
        };
        let Some(output) = snapshot.outputs.iter().find(|o| &o.current.output == id) else {
            return content;
        };
        let editable = !self.pending() && output.editable;
        content.push(label(&output.description, theme).font(font::CAPTION));
        if !output.editable {
            content.push(label("This display is mirrored. Its richer native configuration is preserved; edit it in the user monitor configuration.",theme));
        }
        content.push(button(
            if row.configuration.enabled {
                "Enabled — click to disable"
            } else {
                "Disabled — click to enable"
            },
            "display-enabled",
            Message::Enable(id.clone()),
            editable,
            theme,
        ));
        for (index, title) in [
            "Horizontal position (logical pixels)",
            "Vertical position (logical pixels)",
            "Scale",
        ]
        .into_iter()
        .enumerate()
        {
            content.push(label(title, theme).font(font::CAPTION));
            if editable {
                let revision = self.revision;
                let id = id.clone();
                content.push(
                    theme
                        .text_input(
                            &row.values[index],
                            row.fields[index].selection(),
                            if index == 2 { "1.000" } else { "0" },
                            move |edit| Message::Edit(revision, id.clone(), index, edit),
                        )
                        .id(format!("display-field-{index}-{revision}"))
                        .on_submit(Message::Apply),
                );
            } else {
                content.push(label(&row.values[index], theme));
            }
        }
        content.push(button(
            format!(
                "Rotation: {}°",
                u32::from(row.configuration.transform % 4) * 90
            ),
            "display-rotation",
            Message::Rotate(id.clone()),
            editable,
            theme,
        ));
        content.push(label("Resolution and refresh rate", theme).font(font::TITLE));
        for (index, mode) in output.modes.iter().enumerate() {
            let enabled = editable && row.configuration.enabled;
            content.push(
                theme
                    .choice_button(
                        mode_name(*mode),
                        if enabled {
                            Message::Mode(id.clone(), *mode)
                        } else {
                            Message::Ignore
                        },
                        row.configuration.mode == *mode,
                    )
                    .id(format!("display-mode-{index}"))
                    .width(Length::Fill)
                    .tab_stop(enabled)
                    .opacity(if enabled {
                        1.
                    } else {
                        lucent_design::opacity::MUTED
                    }),
            );
        }
        content
    }
    pub fn view(&self, cx: &ViewContext, theme: Theme) -> Element<Message> {
        if !self.mounted {
            return Element::empty();
        }
        let mut children = vec![
            Element::row(vec![
                label("Displays", theme).font(font::TITLE),
                button("Close", "display-close", Message::Close, true, theme).width(Length::Shrink),
            ])
            .height(Length::Fixed(token::HEADER_HEIGHT))
            .align(Align::Center),
        ];
        if let Some(transaction) = self.snapshot.as_ref().and_then(|s| s.transaction.as_ref()) {
            let status = match &transaction.phase {
                DisplayTransactionPhase::Preparing => "Preparing the display preview…".into(),
                DisplayTransactionPhase::AwaitingConfirmation { remaining_seconds } => {
                    format!("Keep these settings? Reverting in {remaining_seconds} seconds.")
                }
                DisplayTransactionPhase::Kept => "Display settings saved.".into(),
                DisplayTransactionPhase::Reverted { reason }
                | DisplayTransactionPhase::Failed { reason } => reason.clone(),
            };
            children.push(label(status, theme).font(font::CAPTION));
            if matches!(
                transaction.phase,
                DisplayTransactionPhase::AwaitingConfirmation { .. }
            ) {
                children.push(
                    Element::row(vec![
                        button(
                            "Revert",
                            "display-revert",
                            Message::Revert,
                            self.active.is_none(),
                            theme,
                        ),
                        button(
                            "Keep",
                            "display-keep",
                            Message::Keep,
                            self.active.is_none(),
                            theme,
                        ),
                    ])
                    .gap(space::SM),
                );
            }
        }
        children.push(
            theme
                .scroll(
                    ScrollSpec::new(self.scroll, scroll::LINE_STEP)
                        .expect("bounded display scroll"),
                    Element::column(self.content(theme))
                        .width(Length::Fill)
                        .gap(space::MD),
                    Message::Scroll,
                )
                .id("display-scroll"),
        );
        for error in [&self.observation_error, &self.error] {
            if !error.is_empty() {
                children.push(label(error, theme).font(font::CAPTION).color(theme.error));
            }
        }
        children.push(
            Element::row(vec![
                button(
                    "Reset draft",
                    "display-reset",
                    Message::Refresh,
                    !self.pending(),
                    theme,
                ),
                button(
                    "Apply",
                    "display-apply",
                    Message::Apply,
                    self.dirty && !self.pending() && self.observation_error.is_empty(),
                    theme,
                ),
            ])
            .gap(space::SM),
        );
        let rect = crate::shell_layout::display_panel((cx.width, cx.height));
        let progress = self.motion.value(cx.now);
        let panel = Element::column(children)
            .size(rect.w, rect.h)
            .padding(space::XL)
            .gap(space::MD)
            .background(theme.surface)
            .radius(radius::PANEL)
            .shadow()
            .clip()
            .at(rect.x, rect.y - (1. - progress) * token::SLIDE_DISTANCE)
            .opacity(progress)
            .on_click(Message::Ignore)
            .hover_feedback(false)
            .tab_stop(false)
            .id("display-panel");
        Element::stack(vec![
            Element::empty()
                .fill()
                .on_click(Message::Close)
                .tab_stop(false)
                .id("display-backdrop"),
            panel,
        ])
        .fill()
    }
}
