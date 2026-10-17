use std::time::{Duration, Instant};

use calling::camera::{DEFAULT_CAMERA_LABEL, list_cameras};
use calling::devices::{DeviceLists, list_devices, shared_factory};
use calling::{
    CameraEffects, DeviceChoice, DeviceEntry, MicSettings, NoiseSuppression, camera_entries,
};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::rows::{Choice, caption, control_row, row, select, text_button, toggle_row};
use crate::app_state::AppState;
use crate::call::{BackgroundPick, CameraPreview, preview_backgrounds};
use crate::theme;
use crate::views::prejoin_view::demo_devices;

const FPS_TICK: Duration = Duration::from_millis(500);
const PREVIEW_WIDTH: f32 = 288.;
const PREVIEW_HEIGHT: f32 = 162.;
const PREVIEW_RADIUS: f32 = 10.;
const SOFT_FOCUS_LEVELS: [(&str, u8); 3] = [("Low", 25), ("Medium", 50), ("High", 75)];
const CAMERA_IN_USE: &str = "Camera is in use by the call";
const DEFAULT_DEVICE_LABEL: &str = "Default device";

struct Effect {
    id: &'static str,
    title: &'static str,
    read: fn(&CameraEffects) -> bool,
    flip: fn(&mut CameraEffects),
}

const EFFECTS: [Effect; 4] = [
    Effect {
        id: "effect-mirror",
        title: "Mirror my video",
        read: |effects| effects.mirror,
        flip: |effects| effects.mirror = !effects.mirror,
    },
    Effect {
        id: "effect-framing",
        title: "Auto framing",
        read: |effects| effects.auto_framing,
        flip: |effects| effects.auto_framing = !effects.auto_framing,
    },
    Effect {
        id: "effect-brightness",
        title: "Adjust brightness",
        read: |effects| effects.adjust_brightness,
        flip: |effects| effects.adjust_brightness = !effects.adjust_brightness,
    },
    Effect {
        id: "effect-soft-focus",
        title: "Soft focus",
        read: |effects| effects.soft_focus,
        flip: |effects| effects.soft_focus = !effects.soft_focus,
    },
];

fn camera_in_call(state: &AppState) -> bool {
    state.call.is_some()
}

fn label_of(entries: &[DeviceEntry], choice: &DeviceChoice, fallback: &str) -> String {
    entries
        .iter()
        .find(|entry| &entry.choice == choice)
        .map_or_else(|| fallback.to_owned(), |entry| entry.label.clone())
}

pub struct DevicesView {
    app: Entity<AppState>,
    preview: CameraPreview,
    cameras: Vec<DeviceEntry>,
    audio_devices: DeviceLists,
    camera: DeviceChoice,
    background: BackgroundPick,
    preview_blocked: bool,
    _app_subscription: Subscription,
    _release: Subscription,
}

impl DevicesView {
    pub fn new(app: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let app_subscription = cx.observe(&app, |view, _, cx| view.sync_preview_block(cx));
        let release = cx.on_release(|view, cx| view.preview.stop(cx));
        let (preview, mut updates) = CameraPreview::new();
        let state = app.read(cx);
        let demo = state.mode.demo;
        let camera = state.call_camera.clone();
        preview.set_effects(state.call_effects);
        let preview_blocked = camera_in_call(state);
        let background = Some(state.call_background.clone())
            .filter(|pick| {
                preview_backgrounds(&state.call_backgrounds)
                    .iter()
                    .any(|(known, _)| known == pick)
            })
            .unwrap_or_default();
        cx.spawn(async move |this, cx| {
            while updates.recv().await.is_some() {
                if this.update(cx, |view, cx| view.take_frames(cx)).is_err() {
                    return;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(FPS_TICK).await;
                let ticked = this.update(cx, |view, cx| {
                    view.preview.tick(Instant::now());
                    cx.notify();
                });
                if ticked.is_err() {
                    break;
                }
            }
        })
        .detach();
        let mut view = DevicesView {
            app,
            preview,
            cameras: camera_entries(&[]),
            audio_devices: if demo {
                demo_devices()
            } else {
                DeviceLists::default()
            },
            camera,
            background,
            preview_blocked,
            _app_subscription: app_subscription,
            _release: release,
        };
        view.load_devices(demo, cx);
        view.apply_background(cx);
        view.start_preview();
        view
    }

    fn take_frames(&mut self, cx: &mut Context<Self>) {
        self.preview.take_frames(cx);
        cx.notify();
    }

    fn load_devices(&mut self, demo: bool, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let (cameras, audio_devices) = cx
                .background_executor()
                .spawn(async move {
                    let audio_devices = (!demo).then(|| list_devices(&shared_factory()));
                    (list_cameras(), audio_devices)
                })
                .await;
            this.update(cx, |view, cx| {
                view.cameras = camera_entries(&cameras);
                if let Some(audio_devices) = audio_devices {
                    view.audio_devices = audio_devices;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn start_preview(&mut self) {
        if !self.preview_blocked {
            self.preview.start(&self.camera);
        }
    }

    fn sync_preview_block(&mut self, cx: &mut Context<Self>) {
        let blocked = camera_in_call(self.app.read(cx));
        if blocked == self.preview_blocked {
            return;
        }
        self.preview_blocked = blocked;
        if blocked {
            self.preview.stop(cx);
        } else {
            self.start_preview();
        }
        cx.notify();
    }

    fn select_camera(&mut self, camera: DeviceChoice, cx: &mut Context<Self>) {
        self.camera = camera.clone();
        self.app
            .update(cx, |state, cx| state.select_call_camera(camera, cx));
        self.start_preview();
        cx.notify();
    }

    fn select_background(&mut self, pick: BackgroundPick, cx: &mut Context<Self>) {
        self.background = pick.clone();
        self.app
            .update(cx, |state, cx| state.set_call_background(pick, cx));
        self.apply_background(cx);
        cx.notify();
    }

    fn apply_background(&self, cx: &App) {
        let state = self.app.read(cx);
        if let Some(choice) = state.call_backgrounds.choice_for(&self.background) {
            self.preview.set_background(choice);
        }
    }

    fn set_mic(&mut self, settings: MicSettings, cx: &mut Context<Self>) {
        self.app
            .update(cx, |state, cx| state.set_call_mic(settings, cx));
        cx.notify();
    }

    fn set_effects(&mut self, effects: CameraEffects, cx: &mut Context<Self>) {
        self.preview.set_effects(effects);
        self.app
            .update(cx, |state, cx| state.set_call_effects(effects, cx));
        cx.notify();
    }

    fn preview_status(&self) -> String {
        if self.preview_blocked {
            return CAMERA_IN_USE.to_owned();
        }
        match self.preview.failure() {
            Some(reason) => format!("Camera could not start: {reason}"),
            None => self
                .preview
                .fps()
                .map_or("Starting camera...".to_owned(), |fps| {
                    format!("{fps:.0} fps")
                }),
        }
    }

    fn audio_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let view = cx.entity();
        let state = self.app.read(cx);
        let (input, output, mic) = (
            state.call_input.clone(),
            state.call_output.clone(),
            state.call_mic,
        );
        let device_choices = |entries: Vec<DeviceEntry>, current: &DeviceChoice, output: bool| {
            entries
                .into_iter()
                .map(|entry| {
                    let (app, choice) = (self.app.clone(), entry.choice.clone());
                    Choice::new(entry.label, &entry.choice == current, move |cx| {
                        let choice = choice.clone();
                        app.update(cx, |state, cx| {
                            if output {
                                state.select_call_output(choice, cx)
                            } else {
                                state.select_call_input(choice, cx)
                            }
                        })
                    })
                })
                .collect::<Vec<_>>()
        };
        let (outputs, inputs) = (
            self.audio_devices.output_entries(),
            self.audio_devices.input_entries(),
        );
        let speaker_label = label_of(&outputs, &output, DEFAULT_DEVICE_LABEL);
        let microphone_label = label_of(&inputs, &input, DEFAULT_DEVICE_LABEL);
        let noise_choices = NoiseSuppression::ALL
            .iter()
            .map(|level| {
                let (view, level) = (view.clone(), *level);
                Choice::new(level.label(), level == mic.noise_suppression, move |cx| {
                    view.update(cx, |view, cx| {
                        view.set_mic(
                            MicSettings {
                                noise_suppression: level,
                                ..mic
                            },
                            cx,
                        )
                    })
                })
            })
            .collect();
        let music_view = view.clone();
        let call_app = self.app.clone();
        vec![
            control_row(
                "Speaker",
                "",
                select(
                    "settings-speaker",
                    speaker_label,
                    false,
                    device_choices(outputs, &output, true),
                ),
            )
            .into_any_element(),
            control_row(
                "Microphone",
                "",
                select(
                    "settings-microphone",
                    microphone_label,
                    false,
                    device_choices(inputs, &input, false),
                ),
            )
            .into_any_element(),
            control_row(
                "Noise suppression",
                if mic.music_mode {
                    "Off while music mode is on"
                } else {
                    "Echo, hum and background noise"
                },
                select(
                    "noise-suppression-select",
                    mic.noise_suppression.label(),
                    mic.music_mode,
                    noise_choices,
                ),
            )
            .into_any_element(),
            toggle_row(
                "music-mode-row",
                "Music mode",
                "Keeps echo cancellation, turns off the rest",
                theme::text_muted(),
                mic.music_mode,
            )
            .on_click(move |_, _, cx| {
                music_view.update(cx, |view, cx| {
                    view.set_mic(
                        MicSettings {
                            music_mode: !mic.music_mode,
                            ..mic
                        },
                        cx,
                    )
                })
            })
            .into_any_element(),
            control_row(
                "Make a test call",
                "Echo bot, checks your speaker and microphone",
                text_button("settings-test-call", "Make a test call").on_click(move |_, _, cx| {
                    call_app.update(cx, |state, cx| state.start_test_call(cx))
                }),
            )
            .into_any_element(),
        ]
    }

    fn camera_block(&self, cx: &mut Context<Self>) -> Div {
        let view = cx.entity();
        let camera_label = label_of(&self.cameras, &self.camera, DEFAULT_CAMERA_LABEL);
        let camera_choices = self
            .cameras
            .iter()
            .map(|entry| {
                let (view, choice) = (view.clone(), entry.choice.clone());
                Choice::new(
                    entry.label.clone(),
                    entry.choice == self.camera,
                    move |cx| view.update(cx, |view, cx| view.select_camera(choice.clone(), cx)),
                )
            })
            .collect();
        let backgrounds = preview_backgrounds(&self.app.read(cx).call_backgrounds);
        let background_label = backgrounds
            .iter()
            .find(|(pick, _)| *pick == self.background)
            .map_or("None".to_owned(), |(_, label)| label.clone());
        let background_choices = backgrounds
            .into_iter()
            .map(|(pick, label)| {
                let view = view.clone();
                let checked = pick == self.background;
                Choice::new(label, checked, move |cx| {
                    view.update(cx, |view, cx| view.select_background(pick.clone(), cx))
                })
            })
            .collect();
        let image = self.preview.image(Instant::now());
        v_flex()
            .w_full()
            .child(
                row()
                    .items_start()
                    .flex_wrap()
                    .child(
                        div()
                            .w(px(PREVIEW_WIDTH))
                            .h(px(PREVIEW_HEIGHT))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .overflow_hidden()
                            .rounded(px(PREVIEW_RADIUS))
                            .bg(theme::surface())
                            .children(image.map(|image| {
                                img(ImageSource::Render(image))
                                    .size_full()
                                    .object_fit(ObjectFit::Cover)
                                    .rounded(px(PREVIEW_RADIUS))
                            })),
                    )
                    .child(
                        v_flex()
                            .gap(px(12.))
                            .child(device_label("Camera"))
                            .child(select("camera-select", camera_label, false, camera_choices))
                            .child(device_label("Background"))
                            .child(select(
                                "background-select",
                                background_label,
                                false,
                                background_choices,
                            )),
                    ),
            )
            .child(
                div()
                    .px(px(super::rows::ROW_INSET))
                    .text_size(px(12.))
                    .text_color(theme::text_muted())
                    .child(self.preview_status()),
            )
    }

    fn effect_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let effects = self.app.read(cx).call_effects;
        let mut rows: Vec<AnyElement> = EFFECTS
            .iter()
            .map(|effect| {
                let view = cx.entity();
                let flip = effect.flip;
                toggle_row(
                    effect.id,
                    effect.title,
                    "",
                    theme::text_muted(),
                    (effect.read)(&effects),
                )
                .on_click(move |_, _, cx| {
                    view.update(cx, |view, cx| {
                        let mut changed = view.app.read(cx).call_effects;
                        flip(&mut changed);
                        view.set_effects(changed, cx)
                    })
                })
                .into_any_element()
            })
            .collect();
        if effects.soft_focus {
            rows.push(self.strength_row(effects, cx).into_any_element());
        }
        rows
    }

    fn strength_row(&self, effects: CameraEffects, cx: &mut Context<Self>) -> Div {
        let levels = SOFT_FOCUS_LEVELS.map(|(label, level)| {
            let view = cx.entity();
            text_button(label, label)
                .when(effects.soft_focus_strength == level, |button| {
                    button.bg(theme::accent()).text_color(theme::on_accent())
                })
                .on_click(move |_, _, cx| {
                    view.update(cx, |view, cx| {
                        let changed = CameraEffects {
                            soft_focus_strength: level,
                            ..view.app.read(cx).call_effects
                        };
                        view.set_effects(changed, cx)
                    })
                })
        });
        control_row(
            "Soft focus strength",
            "",
            h_flex().gap(px(8.)).children(levels),
        )
    }
}

fn device_label(text: &'static str) -> Div {
    div()
        .text_size(px(12.))
        .text_color(theme::text_muted())
        .child(text)
}

impl Render for DevicesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let report_on = self.app.read(cx).call_report_enabled;
        let report_app = self.app.clone();
        v_flex()
            .w_full()
            .child(caption("AUDIO DEVICES"))
            .children(self.audio_rows(cx))
            .child(caption("CAMERA"))
            .child(self.camera_block(cx))
            .children(self.effect_rows(cx))
            .child(caption("DIAGNOSTICS"))
            .child(
                toggle_row(
                    "call-report-row",
                    "Save a test report after each call",
                    "Pseudonymized, stays on this computer",
                    theme::text_muted(),
                    report_on,
                )
                .on_click(move |_, _, cx| {
                    report_app.update(cx, |state, cx| {
                        state.set_call_report_enabled(!report_on, cx)
                    })
                }),
            )
    }
}
