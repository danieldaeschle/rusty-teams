use std::rc::Rc;

use gpui_kit::component::h_flex;
use gpui_kit::*;

use crate::rows::MessageRow;
use crate::theme;
use crate::translation::TranslationLine;

const LINE_SIZE: f32 = 11.;
const LINE_GAP: f32 = 10.;

pub type LineAction = Rc<dyn Fn(&mut App)>;

#[derive(Clone)]
pub struct TranslationActions {
    pub translate: LineAction,
    pub never: Option<LineAction>,
}

fn line_link(id: ElementId, label: String, color: Hsla, action: LineAction) -> Stateful<Div> {
    div()
        .id(id)
        .cursor_pointer()
        .text_color(color)
        .hover(|link| link.underline())
        .child(label)
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            action(cx)
        })
}

pub fn translation_line(
    row: &MessageRow,
    index: usize,
    actions: Option<&TranslationActions>,
) -> Option<AnyElement> {
    let line = row.translation.as_ref()?;
    let link_id = |name: &str| ElementId::Name(format!("translation-{name}-{index}").into());
    let base = h_flex()
        .gap(px(LINE_GAP))
        .text_size(px(LINE_SIZE))
        .text_color(theme::text_muted());
    let element = match line {
        TranslationLine::Offer { language, .. } => base
            .children(actions.map(|actions| {
                line_link(
                    link_id("translate"),
                    "Translate".to_owned(),
                    theme::accent_text(),
                    actions.translate.clone(),
                )
            }))
            .children(actions.and_then(|actions| {
                actions.never.clone().map(|never| {
                    line_link(
                        link_id("never"),
                        format!("Never translate {language}"),
                        theme::text_muted(),
                        never,
                    )
                })
            })),
        TranslationLine::Working => base.child("Translating..."),
        TranslationLine::Failed => {
            base.child("Couldn't translate.")
                .children(actions.map(|actions| {
                    line_link(
                        link_id("retry"),
                        "Retry".to_owned(),
                        theme::accent_text(),
                        actions.translate.clone(),
                    )
                }))
        }
        TranslationLine::Translated {
            language,
            showing_original,
        } => {
            let summary = match (language, showing_original) {
                (_, true) => "Showing original".to_owned(),
                (Some(language), false) => format!("Translated from {language}"),
                (None, false) => "Translated".to_owned(),
            };
            let toggle = if *showing_original {
                "See translation"
            } else {
                "See original"
            };
            base.child(
                h_flex()
                    .gap(px(4.))
                    .child(summary)
                    .child("\u{b7}")
                    .children(actions.map(|actions| {
                        line_link(
                            link_id("toggle"),
                            toggle.to_owned(),
                            theme::accent_text(),
                            actions.translate.clone(),
                        )
                    })),
            )
        }
    };
    Some(element.into_any_element())
}
