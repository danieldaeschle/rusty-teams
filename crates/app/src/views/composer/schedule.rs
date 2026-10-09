use chrono::{DateTime, Local, Utc};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::NOTES_CHAT_ID;

use super::{Composer, ComposerEvent};
use crate::schedule_time::{
    DayChoice, Preset, PresetKind, ScheduleError, custom_time, day_choices, default_custom_time,
    format_time, parse_time, presets, step_time, validate,
};
use crate::theme;
use crate::views::widgets::symbol;

const MENU_WIDTH: f32 = 190.;
const PICKER_WIDTH: f32 = 300.;
const ROW_HEIGHT: f32 = 32.;
const ICON_SIZE: f32 = 18.;
const DISABLED_OPACITY: f32 = 0.4;
const TIME_FIELD_WIDTH: f32 = 84.;
const BAD_TIME: &str = "Enter a time like 14:30.";

pub(super) enum Stage {
    Menu,
    Picker,
}

pub(super) struct CustomForm {
    day_index: usize,
    time_field: Entity<InputState>,
    _edits: Subscription,
}

pub(super) struct SchedulePopover {
    stage: Stage,
    presets: Vec<Preset>,
    highlighted: usize,
    custom: Option<CustomForm>,
}

impl SchedulePopover {
    fn custom_index(&self) -> usize {
        self.presets.len()
    }
}

fn preset_glyph(kind: PresetKind) -> &'static str {
    match kind {
        PresetKind::LaterToday => "schedule",
        PresetKind::TomorrowMorning => "wb_sunny",
        PresetKind::MondayMorning => "work",
    }
}

fn menu_row(id: ElementId, highlighted: bool) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(ROW_HEIGHT))
        .px(px(10.))
        .gap(px(10.))
        .items_center()
        .rounded(px(6.))
        .text_size(px(13.))
        .text_color(theme::text())
        .cursor_pointer()
        .when(highlighted, |row| row.bg(theme::row_hover()))
        .hover(|row| row.bg(theme::row_hover()))
}

impl Composer {
    pub(super) fn schedule_blocker(&self, cx: &App) -> Option<&'static str> {
        let state = self.app.read(cx);
        let conversation_id = self.conversation_id.as_deref().unwrap_or_default();
        let outgoing = self.compose(cx);
        if state.mode.read_only && !state.mode.demo {
            Some("Read-only mode")
        } else if conversation_id == NOTES_CHAT_ID {
            Some("Not available in your notes")
        } else if conversation_id.is_empty() {
            Some("Not available in a new chat yet")
        } else if self.editing.is_some() {
            Some("Can't schedule while editing")
        } else if !outgoing.mentions.is_empty() {
            Some("Can't schedule messages with mentions yet")
        } else if outgoing.has_attachments() || self.has_attachments(cx) {
            Some("Can't schedule messages with images or files yet")
        } else if self.reply.is_some() {
            Some("Can't schedule replies yet")
        } else if self.current_draft(cx).is_blank() {
            Some("Nothing to schedule yet")
        } else {
            None
        }
    }

    pub(super) fn open_schedule_menu(&mut self, cx: &mut Context<Self>) {
        self.schedule = Some(SchedulePopover {
            stage: Stage::Menu,
            presets: Vec::new(),
            highlighted: 0,
            custom: None,
        });
        cx.notify();
    }

    pub(super) fn open_schedule_picker(&mut self, cx: &mut Context<Self>) {
        if self.schedule_blocker(cx).is_some() || self.scheduling.is_some() {
            self.open_schedule_menu(cx);
            return;
        }
        self.schedule = Some(SchedulePopover {
            stage: Stage::Picker,
            presets: presets(Local::now()),
            highlighted: 0,
            custom: None,
        });
        cx.notify();
    }

    pub(super) fn close_schedule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.schedule.take().is_some() {
            self.focus(window, cx);
            cx.notify();
        }
    }

    pub(super) fn move_schedule_highlight(
        &mut self,
        delta: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(popover) = self.schedule.as_mut() else {
            return;
        };
        if !matches!(popover.stage, Stage::Picker) {
            return;
        }
        if popover.custom.is_some() {
            self.step_custom_time(delta, window, cx);
            return;
        }
        let count = popover.presets.len() + 1;
        popover.highlighted =
            (popover.highlighted as isize + delta).rem_euclid(count as isize) as usize;
        cx.notify();
    }

    pub(super) fn activate_schedule_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(popover) = self.schedule.as_ref() else {
            return;
        };
        let highlighted = popover.highlighted;
        let on_custom_row = highlighted == popover.custom_index();
        let custom_open = popover.custom.is_some();
        match popover.stage {
            Stage::Menu => {
                if self.schedule_blocker(cx).is_none() {
                    self.open_schedule_picker(cx);
                }
            }
            Stage::Picker if custom_open => self.confirm_custom(window, cx),
            Stage::Picker if on_custom_row => self.expand_custom(window, cx),
            Stage::Picker => self.schedule_preset(highlighted, window, cx),
        }
    }

    fn schedule_preset(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preset) = self
            .schedule
            .as_ref()
            .and_then(|popover| popover.presets.get(index))
        else {
            return;
        };
        let at = preset.at;
        if validate(at, Local::now()).is_err() {
            if let Some(popover) = self.schedule.as_mut() {
                popover.presets = presets(Local::now());
                popover.highlighted = 0;
            }
            cx.notify();
            return;
        }
        self.schedule_at(at, window, cx);
    }

    fn expand_custom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let time = format_time(default_custom_time(Local::now()));
        let time_field = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("HH:MM")
                .default_value(time)
        });
        time_field.update(cx, |field, cx| {
            field.focus(window, cx);
            field.select_all(window, cx);
        });
        let edits = cx.subscribe(&time_field, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        if let Some(popover) = self.schedule.as_mut() {
            popover.custom = Some(CustomForm {
                day_index: 0,
                time_field,
                _edits: edits,
            });
        }
        cx.notify();
    }

    fn step_custom_time(&mut self, steps: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self
            .schedule
            .as_ref()
            .and_then(|popover| popover.custom.as_ref())
            .map(|custom| custom.time_field.clone())
        else {
            return;
        };
        let current = parse_time(&field.read(cx).value()).unwrap_or_default();
        let stepped = format_time(step_time(current, -(steps as i32)));
        field.update(cx, |field, cx| field.set_value(&stepped, window, cx));
    }

    fn custom_choice(&self, cx: &App) -> Result<DateTime<Local>, &'static str> {
        let custom = self
            .schedule
            .as_ref()
            .and_then(|popover| popover.custom.as_ref())
            .ok_or(BAD_TIME)?;
        let now = Local::now();
        let day = day_choices(now.date_naive())
            .get(custom.day_index)
            .map(|choice| choice.date)
            .ok_or(BAD_TIME)?;
        let at = custom_time(day, &custom.time_field.read(cx).value()).ok_or(BAD_TIME)?;
        validate(at, now).map_err(ScheduleError::message)?;
        Ok(at)
    }

    fn confirm_custom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Ok(at) = self.custom_choice(cx) {
            self.schedule_at(at, window, cx);
        }
    }

    fn schedule_at(&mut self, at: DateTime<Local>, window: &mut Window, cx: &mut Context<Self>) {
        if self.scheduling.is_some() || self.schedule_blocker(cx).is_some() {
            return;
        }
        let outgoing = self.compose(cx);
        self.scheduling = Some(outgoing.html());
        self.close_schedule(window, cx);
        cx.emit(ComposerEvent::Schedule {
            outgoing: Box::new(outgoing),
            send_at: at.with_timezone(&Utc),
        });
    }

    pub fn finish_schedule(
        &mut self,
        conversation_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let scheduled = self.scheduling.take();
        if self.conversation_id.as_deref() != Some(conversation_id) {
            self.app.read(cx).store.delete_draft(conversation_id).ok();
            self.app
                .update(cx, |state, cx| state.refresh_local_previews(cx));
            return;
        }
        if scheduled.as_deref() != Some(self.compose(cx).html().as_str()) {
            return;
        }
        self.load_draft(
            teams_core::Draft::default(),
            gpui_kit::component::input::InputContent::new(""),
            window,
            cx,
        );
        self.mention_inputs.clear();
        self.tray.clear();
        self.clear_stored_draft(cx);
        self.reply = None;
        self.close_popup();
        cx.notify();
    }

    pub fn fail_schedule(&mut self, cx: &mut Context<Self>) {
        self.scheduling = None;
        cx.notify();
    }

    pub(super) fn render_schedule(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let popover = self.schedule.as_ref()?;
        let body = match popover.stage {
            Stage::Menu => self.render_schedule_menu(cx),
            Stage::Picker => self.render_schedule_picker(popover, cx),
        };
        Some(
            v_flex()
                .id("schedule-popover")
                .absolute()
                .bottom(relative(1.))
                .right_0()
                .mb(px(6.))
                .w(px(match popover.stage {
                    Stage::Menu => MENU_WIDTH,
                    Stage::Picker => PICKER_WIDTH,
                }))
                .p(px(4.))
                .rounded(px(8.))
                .border_1()
                .border_color(theme::border_strong())
                .bg(theme::surface_raised())
                .shadow_lg()
                .occlude()
                .on_mouse_down_out(
                    cx.listener(|this, _, window, cx| this.close_schedule(window, cx)),
                )
                .child(body)
                .into_any_element(),
        )
    }

    fn render_schedule_menu(&self, cx: &mut Context<Self>) -> Div {
        let blocker = self.schedule_blocker(cx);
        let row = menu_row("schedule-send-entry".into(), true)
            .child(symbol("schedule_send", ICON_SIZE, theme::text_soft()))
            .child("Schedule send");
        let row = match blocker {
            Some(reason) => row
                .opacity(DISABLED_OPACITY)
                .cursor_default()
                .tooltip(move |window, cx| Tooltip::new(reason).build(window, cx)),
            None => row.on_click(cx.listener(|this, _, _, cx| this.open_schedule_picker(cx))),
        };
        v_flex().child(row)
    }

    fn render_schedule_picker(&self, popover: &SchedulePopover, cx: &mut Context<Self>) -> Div {
        let mut list = v_flex();
        for (index, preset) in popover.presets.iter().enumerate() {
            list = list.child(
                menu_row(
                    ElementId::Name(format!("schedule-preset-{index}").into()),
                    index == popover.highlighted && popover.custom.is_none(),
                )
                .child(symbol(
                    preset_glyph(preset.kind),
                    ICON_SIZE,
                    theme::text_soft(),
                ))
                .child(div().flex_1().child(preset.label))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::text_faint())
                        .child(preset.time_label.clone()),
                )
                .on_click(
                    cx.listener(move |this, _, window, cx| this.schedule_preset(index, window, cx)),
                ),
            );
        }
        list = list
            .child(
                div()
                    .h(px(1.))
                    .mx(px(6.))
                    .my(px(4.))
                    .bg(theme::border_strong()),
            )
            .child(
                menu_row(
                    "schedule-custom".into(),
                    popover.highlighted == popover.custom_index() && popover.custom.is_none(),
                )
                .child(symbol("event", ICON_SIZE, theme::text_soft()))
                .child("Custom...")
                .on_click(cx.listener(|this, _, window, cx| this.expand_custom(window, cx))),
            );
        if let Some(custom) = popover.custom.as_ref() {
            list = list.child(self.render_custom_form(custom, cx));
        }
        list
    }

    fn render_custom_form(&self, custom: &CustomForm, cx: &mut Context<Self>) -> Div {
        let choice = self.custom_choice(cx);
        let days: Vec<DayChoice> = day_choices(Local::now().date_naive());
        let chips = days.into_iter().enumerate().map(|(index, day)| {
            let selected = index == custom.day_index;
            div()
                .id(ElementId::Name(format!("schedule-day-{index}").into()))
                .h(px(26.))
                .px(px(8.))
                .flex()
                .items_center()
                .rounded(px(6.))
                .border_1()
                .border_color(if selected {
                    theme::accent()
                } else {
                    theme::border_strong()
                })
                .text_size(px(12.))
                .cursor_pointer()
                .when(selected, |chip| {
                    chip.bg(theme::accent()).text_color(theme::on_accent())
                })
                .when(!selected, |chip| {
                    chip.text_color(theme::text_soft())
                        .hover(|chip| chip.bg(theme::row_hover()))
                })
                .child(day.label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(form) = this
                        .schedule
                        .as_mut()
                        .and_then(|popover| popover.custom.as_mut())
                    {
                        form.day_index = index;
                    }
                    cx.notify();
                }))
        });
        let valid = choice.is_ok();
        v_flex()
            .gap(px(8.))
            .p(px(10.))
            .child(h_flex().flex_wrap().gap(px(6.)).children(chips))
            .child(
                h_flex()
                    .gap(px(10.))
                    .items_center()
                    .child(
                        div()
                            .w(px(TIME_FIELD_WIDTH))
                            .h(px(28.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(theme::accent())
                            .bg(theme::surface())
                            .child(
                                Input::new(&custom.time_field)
                                    .appearance(false)
                                    .bordered(false),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("schedule-confirm")
                            .h(px(30.))
                            .px(px(14.))
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .bg(theme::accent())
                            .text_color(theme::on_accent())
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Schedule")
                            .when(!valid, |button| button.opacity(DISABLED_OPACITY))
                            .when(valid, |button| {
                                button.cursor_pointer().on_click(cx.listener(
                                    |this, _, window, cx| this.confirm_custom(window, cx),
                                ))
                            }),
                    ),
            )
            .children(choice.err().map(|message| {
                div()
                    .text_size(px(12.))
                    .text_color(theme::red_soft())
                    .child(message)
            }))
    }
}
