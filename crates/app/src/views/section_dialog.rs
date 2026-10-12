use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{Dialog, DialogFooter};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::*;

use crate::app_state::AppState;

const DIALOG_WIDTH: f32 = 380.;

#[derive(Clone)]
pub enum SectionNameTarget {
    New,
    Rename { folder_id: String, name: String },
}

impl SectionNameTarget {
    fn title(&self) -> &'static str {
        match self {
            SectionNameTarget::New => "New section",
            SectionNameTarget::Rename { .. } => "Rename section",
        }
    }

    fn confirm_label(&self) -> &'static str {
        match self {
            SectionNameTarget::New => "Create",
            SectionNameTarget::Rename { .. } => "Rename",
        }
    }
}

pub fn clean_section_name(raw: &str) -> Option<String> {
    let name = raw.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

struct SectionNameForm {
    state: Entity<AppState>,
    target: SectionNameTarget,
    name: Entity<InputState>,
    _subscription: Subscription,
}

impl SectionNameForm {
    fn new(
        state: Entity<AppState>,
        target: SectionNameTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let current = match &target {
            SectionNameTarget::New => String::new(),
            SectionNameTarget::Rename { name, .. } => name.clone(),
        };
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Section name")
                .default_value(current)
        });
        let subscription = cx.subscribe_in(&name, window, Self::on_enter);
        SectionNameForm {
            state,
            target,
            name,
            _subscription: subscription,
        }
    }

    fn on_enter(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::PressEnter { .. }) && self.submit(cx) {
            window.close_dialog(cx);
        }
    }

    fn submit(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(name) = clean_section_name(&self.name.read(cx).value()) else {
            return false;
        };
        let target = self.target.clone();
        self.state.update(cx, |state, cx| match target {
            SectionNameTarget::New => state.create_section(&name, cx),
            SectionNameTarget::Rename { folder_id, .. } => {
                state.rename_section(&folder_id, &name, cx)
            }
        });
        true
    }
}

impl Render for SectionNameForm {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex().child(Input::new(&self.name))
    }
}

pub fn open_section_name_dialog(
    state: Entity<AppState>,
    target: SectionNameTarget,
    window: &mut Window,
    cx: &mut App,
) {
    let (title, confirm_label) = (target.title(), target.confirm_label());
    let form = cx.new(|cx| SectionNameForm::new(state, target, window, cx));
    let field = form.read(cx).name.clone();
    window.open_dialog(cx, move |dialog: Dialog, _, _| {
        let submit_form = form.clone();
        let footer = DialogFooter::new()
            .child(
                Button::new("section-name-cancel")
                    .label("Cancel")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                Button::new("section-name-confirm")
                    .primary()
                    .label(confirm_label)
                    .on_click(move |_, window, cx| {
                        if submit_form.update(cx, |form, cx| form.submit(cx)) {
                            window.close_dialog(cx);
                        }
                    }),
            );
        dialog
            .title(title)
            .w(px(DIALOG_WIDTH))
            .child(form.clone())
            .footer(footer)
    });
    window.defer(cx, move |window, cx| {
        field.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::clean_section_name;

    #[test]
    fn section_names_are_trimmed_and_never_empty() {
        assert_eq!(clean_section_name("  Todo "), Some("Todo".to_owned()));
        assert_eq!(clean_section_name("   "), None);
    }
}
