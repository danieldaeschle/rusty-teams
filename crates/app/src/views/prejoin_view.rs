use std::time::{Duration, Instant};

use calling::camera::list_cameras;
use calling::devices::{AudioDevice, DeviceLists, list_devices, shared_factory};
use calling::{DeviceChoice, DeviceEntry, camera_entries};
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::person_avatar;
use super::call_extras::{CameraMenu, background_items};
use super::call_view::{device_items, round_control};
use crate::app_state::AppState;
use crate::call::{AudioChoice, CameraPreview, JoinOptions, PendingJoin};
use crate::theme;

const CONTENT_WIDTH: f32 = 560.;
const PREVIEW_HEIGHT: f32 = 315.;
const PREVIEW_RADIUS: f32 = 12.;
const AVATAR_SIZE: f32 = 96.;
const SELECT_WIDTH: f32 = 176.;
const SECTION_GAP: f32 = 12.;
const PANE_PADDING: f32 = 12.;
const PREVIEW_MIN_HEIGHT: f32 = 60.;
const FRAME_TICK: Duration = Duration::from_millis(100);
const FPS_TICK: Duration = Duration::from_millis(500);
const CAMERA_OFF_TEXT: &str = "Your camera is off";
const DEFAULT_LABEL: &str = "Default device";
const DEMO_MICROPHONES: [&str; 2] = ["Headset microphone", "Laptop microphone"];
const DEMO_SPEAKERS: [&str; 2] = ["Headset earphones", "Laptop speakers"];

pub struct PrejoinView {
    app: Entity<AppState>,
    pending_id: u64,
    demo: bool,
    preview: CameraPreview,
    options: JoinOptions,
    cameras: Vec<DeviceEntry>,
    devices: DeviceLists,
    camera: DeviceChoice,
    input: DeviceChoice,
    output: DeviceChoice,
    applied_background: crate::call::BackgroundPick,
    tick: u32,
    _subscription: Subscription,
    _release: Subscription,
}

struct DeviceSelect {
    id: &'static str,
    label: &'static str,
    entries: Vec<DeviceEntry>,
    current: DeviceChoice,
    enabled: bool,
}

pub(crate) fn demo_devices() -> DeviceLists {
    let device = |index: u16, name: &str| AudioDevice {
        index,
        guid: format!("demo-{name}"),
        name: name.to_owned(),
    };
    DeviceLists {
        inputs: DEMO_MICROPHONES
            .iter()
            .enumerate()
            .map(|(index, name)| device(index as u16, name))
            .collect(),
        outputs: DEMO_SPEAKERS
            .iter()
            .enumerate()
            .map(|(index, name)| device(index as u16, name))
            .collect(),
    }
}

fn label_of(entries: &[DeviceEntry], choice: &DeviceChoice) -> String {
    entries
        .iter()
        .find(|entry| &entry.choice == choice)
        .map_or_else(|| DEFAULT_LABEL.to_owned(), |entry| entry.label.clone())
}

impl PrejoinView {
    pub fn new(
        app: Entity<AppState>,
        pending: &PendingJoin,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe(&app, |view, _, cx| view.sync_background(cx));
        let release = cx.on_release(|view, cx| view.preview.stop(cx));
        let (preview, mut updates) = CameraPreview::new();
        let state = app.read(cx);
        preview.set_effects(state.call_effects);
        let demo = state.mode.demo;
        let mut options = state.join_options.clone();
        options.camera_on = options.camera_on && !state.mode.read_only;
        let camera = state.call_camera.clone();
        let (input, output) = (state.call_input.clone(), state.call_output.clone());
        let mut view = PrejoinView {
            pending_id: pending.id,
            demo,
            preview,
            options,
            cameras: camera_entries(&[]),
            devices: if demo {
                demo_devices()
            } else {
                DeviceLists::default()
            },
            camera,
            input,
            output,
            applied_background: crate::call::BackgroundPick::None,
            tick: 0,
            _subscription: subscription,
            _release: release,
            app,
        };
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
                if this
                    .update(cx, |view, cx| {
                        view.preview.tick(Instant::now());
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        if demo {
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(FRAME_TICK).await;
                    if this.update(cx, |view, cx| view.demo_frame(cx)).is_err() {
                        break;
                    }
                }
            })
            .detach();
        } else {
            view.load_devices(cx);
        }
        view.sync_background(cx);
        view.restart_preview();
        view
    }

    pub fn pending_id(&self) -> u64 {
        self.pending_id
    }

    fn take_frames(&mut self, cx: &mut Context<Self>) {
        self.preview.take_frames(cx);
        cx.notify();
    }

    fn demo_frame(&mut self, cx: &mut Context<Self>) {
        if self.options.camera_on {
            self.tick = self.tick.wrapping_add(1);
            self.preview.publish_demo_frame(self.tick);
            self.take_frames(cx);
        }
    }

    fn load_devices(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let (cameras, devices) = cx
                .background_executor()
                .spawn(async { (list_cameras(), list_devices(&shared_factory())) })
                .await;
            this.update(cx, |view, cx| {
                view.cameras = camera_entries(&cameras);
                view.devices = devices;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn restart_preview(&mut self) {
        if self.options.camera_on && !self.demo {
            self.preview.start(&self.camera);
        }
    }

    fn sync_background(&mut self, cx: &mut Context<Self>) {
        let state = self.app.read(cx);
        let pick = state.call_background.clone();
        if pick == self.applied_background {
            return;
        }
        if let Some(choice) = state.call_backgrounds.choice_for(&pick) {
            self.preview.set_background(choice);
            self.applied_background = pick;
        }
        cx.notify();
    }

    fn toggle_camera(&mut self, cx: &mut Context<Self>) {
        self.options.camera_on = !self.options.camera_on;
        if self.options.camera_on {
            self.restart_preview();
        } else {
            self.preview.stop(cx);
        }
        cx.notify();
    }

    fn toggle_mic(&mut self, cx: &mut Context<Self>) {
        self.options.mic_on = !self.options.mic_on;
        cx.notify();
    }

    fn set_audio(&mut self, audio: AudioChoice, cx: &mut Context<Self>) {
        self.options.audio = audio;
        cx.notify();
    }

    fn pick_camera(&mut self, choice: DeviceChoice, cx: &mut Context<Self>) {
        self.camera = choice.clone();
        self.app
            .update(cx, |state, cx| state.select_call_camera(choice, cx));
        self.restart_preview();
        cx.notify();
    }

    fn pick_input(&mut self, choice: DeviceChoice, cx: &mut Context<Self>) {
        self.input = choice.clone();
        self.app
            .update(cx, |state, cx| state.select_call_input(choice, cx));
        cx.notify();
    }

    fn pick_output(&mut self, choice: DeviceChoice, cx: &mut Context<Self>) {
        self.output = choice.clone();
        self.app
            .update(cx, |state, cx| state.select_call_output(choice, cx));
        cx.notify();
    }

    fn join(&mut self, cx: &mut Context<Self>) {
        self.preview.stop(cx);
        let options = self.options.clone();
        self.app
            .update(cx, |state, cx| state.confirm_prejoin(options, cx));
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.app.update(cx, |state, cx| state.cancel_prejoin(cx));
    }

    fn preview_status(&self) -> String {
        match self.preview.failure() {
            Some(reason) => format!("Camera could not start: {reason}"),
            None => "Starting camera...".to_owned(),
        }
    }

    fn preview_box(&self, cx: &mut Context<Self>) -> Div {
        let state = self.app.read(cx);
        let image = self
            .options
            .camera_on
            .then(|| self.preview.image(Instant::now()))
            .flatten();
        let me = state.directory.me.clone();
        let face = match (&image, self.options.camera_on) {
            (Some(image), _) => img(ImageSource::Render(image.clone()))
                .size_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            (None, true) => centered_note(&self.preview_status()),
            (None, false) => v_flex()
                .items_center()
                .gap(px(10.))
                .child(person_avatar(
                    &state.directory,
                    me.as_ref().map(|me| me.user_id.as_str()),
                    me.as_ref().map_or("You", |me| me.display_name.as_str()),
                    AVATAR_SIZE,
                ))
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(theme::text_muted())
                        .child(CAMERA_OFF_TEXT),
                )
                .into_any_element(),
        };
        div()
            .debug_selector(|| "shell/islands/main/prejoin/preview".into())
            .relative()
            .w_full()
            .h(px(PREVIEW_HEIGHT))
            .flex_shrink(1.)
            .min_h(px(PREVIEW_MIN_HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .rounded(px(PREVIEW_RADIUS))
            .bg(theme::background())
            .child(face)
    }

    fn controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let camera_view = view.clone();
        let mic_view = view.clone();
        let (camera_glyph, camera_bg, camera_fg) = if self.options.camera_on {
            (IconName::Video, theme::accent(), theme::on_accent())
        } else {
            (IconName::VideoOff, theme::surface_raised(), theme::text())
        };
        let (mic_glyph, mic_bg, mic_fg) = if self.options.mic_on {
            (IconName::Mic, theme::surface_raised(), theme::text())
        } else {
            (IconName::MicOff, theme::red(), theme::white())
        };
        let audio_on = self.options.audio == AudioChoice::Computer;
        let backgrounds = CameraMenu::of(self.app.read(cx));
        let background_app = self.app.clone();
        h_flex()
            .debug_selector(|| "shell/islands/main/prejoin/controls".into())
            .flex_none()
            .gap(px(10.))
            .items_center()
            .justify_center()
            .child(
                round_control("prejoin-camera", camera_glyph, camera_bg, camera_fg).on_click(
                    move |_, _, cx| camera_view.update(cx, |view, cx| view.toggle_camera(cx)),
                ),
            )
            .child(
                round_control("prejoin-mic", mic_glyph, mic_bg, mic_fg)
                    .when(!audio_on, |button| button.opacity(0.4).cursor_default())
                    .when(audio_on, |button| {
                        button.on_click(move |_, _, cx| {
                            mic_view.update(cx, |view, cx| view.toggle_mic(cx))
                        })
                    }),
            )
            .child(
                Button::new("prejoin-background")
                    .outline()
                    .label("Background")
                    .icon(IconName::ChevronDown)
                    .dropdown_menu(move |menu: PopupMenu, _, _| {
                        background_items(menu, &background_app, &backgrounds)
                    }),
            )
    }

    fn select(
        &self,
        spec: DeviceSelect,
        pick: impl Fn(&mut PrejoinView, DeviceChoice, &mut Context<PrejoinView>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Div {
        let DeviceSelect {
            id,
            label,
            entries,
            current,
            enabled,
        } = spec;
        let view = cx.entity();
        let face = label_of(&entries, &current);
        let items_entries = entries;
        let items_current = current;
        let button = Button::new(id)
            .outline()
            .w_full()
            .label(face)
            .icon(IconName::ChevronDown)
            .disabled(!enabled)
            .dropdown_menu(move |menu, _, _| {
                let view = view.clone();
                let pick = pick.clone();
                device_items(items_entries.clone(), &items_current, move |choice, cx| {
                    let pick = pick.clone();
                    view.update(cx, |view, cx| pick(view, choice, cx));
                })
                .into_iter()
                .fold(menu, |menu, item| menu.item(item))
            });
        v_flex()
            .flex_1()
            .min_w_0()
            .max_w(px(SELECT_WIDTH))
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::text_muted())
                    .child(label),
            )
            .child(div().w_full().overflow_hidden().child(button))
    }

    fn audio_choice(&self, choice: AudioChoice, cx: &mut Context<Self>) -> Stateful<Div> {
        let selected = self.options.audio == choice;
        let view = cx.entity();
        h_flex()
            .id(match choice {
                AudioChoice::Computer => "prejoin-audio-computer",
                AudioChoice::NoAudio => "prejoin-audio-none",
            })
            .h(px(32.))
            .px(px(14.))
            .items_center()
            .rounded(px(8.))
            .border_1()
            .border_color(if selected {
                theme::accent()
            } else {
                theme::border_strong()
            })
            .when(selected, |option| option.bg(theme::surface_raised()))
            .cursor_pointer()
            .text_size(px(13.))
            .text_color(theme::text())
            .child(choice.label())
            .on_click(move |_, _, cx| view.update(cx, |view, cx| view.set_audio(choice, cx)))
    }
}

fn centered_note(text: &str) -> AnyElement {
    div()
        .text_size(px(13.))
        .text_color(theme::text_muted())
        .child(text.to_owned())
        .into_any_element()
}

impl Render for PrejoinView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self
            .app
            .read(cx)
            .prejoin
            .as_ref()
            .map(|pending| pending.join.title.clone())
            .unwrap_or_default();
        let audio_on = self.options.audio == AudioChoice::Computer;
        let (cameras, camera) = (self.cameras.clone(), self.camera.clone());
        let (inputs, input) = (self.devices.input_entries(), self.input.clone());
        let (outputs, output) = (self.devices.output_entries(), self.output.clone());
        let devices = h_flex()
            .debug_selector(|| "shell/islands/main/prejoin/devices".into())
            .flex_none()
            .w_full()
            .gap(px(10.))
            .child(self.select(
                DeviceSelect {
                    id: "prejoin-camera-select",
                    label: "Camera",
                    entries: cameras,
                    current: camera,
                    enabled: true,
                },
                |view, choice, cx| view.pick_camera(choice, cx),
                cx,
            ))
            .child(self.select(
                DeviceSelect {
                    id: "prejoin-mic-select",
                    label: "Microphone",
                    entries: inputs,
                    current: input,
                    enabled: audio_on,
                },
                |view, choice, cx| view.pick_input(choice, cx),
                cx,
            ))
            .child(self.select(
                DeviceSelect {
                    id: "prejoin-speaker-select",
                    label: "Speaker",
                    entries: outputs,
                    current: output,
                    enabled: audio_on,
                },
                |view, choice, cx| view.pick_output(choice, cx),
                cx,
            ));
        let audio_row = h_flex()
            .debug_selector(|| "shell/islands/main/prejoin/audio".into())
            .flex_none()
            .flex_wrap()
            .gap(px(8.))
            .child(self.audio_choice(AudioChoice::Computer, cx))
            .child(self.audio_choice(AudioChoice::NoAudio, cx));
        let cancel_view = cx.entity();
        let join_view = cx.entity();
        let actions = h_flex()
            .debug_selector(|| "shell/islands/main/prejoin/actions".into())
            .flex_none()
            .gap(px(8.))
            .justify_end()
            .child(
                Button::new("prejoin-cancel")
                    .ghost()
                    .label("Cancel")
                    .on_click(move |_, _, cx| cancel_view.update(cx, |view, cx| view.cancel(cx))),
            )
            .child(
                Button::new("prejoin-join")
                    .primary()
                    .label("Join now")
                    .on_click(move |_, _, cx| join_view.update(cx, |view, cx| view.join(cx))),
            );
        v_flex()
            .debug_selector(|| "shell/islands/main".into())
            .size_full()
            .p(px(PANE_PADDING))
            .items_center()
            .justify_center()
            .rounded(px(10.))
            .bg(theme::surface())
            .overflow_hidden()
            .child(
                v_flex()
                    .debug_selector(|| "shell/islands/main/prejoin".into())
                    .w(px(CONTENT_WIDTH))
                    .max_w_full()
                    .max_h_full()
                    .min_h_0()
                    .gap(px(SECTION_GAP))
                    .child(
                        v_flex()
                            .flex_none()
                            .gap(px(2.))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(20.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::text_strong())
                                    .child(title),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.))
                                    .text_color(theme::text_muted())
                                    .child("Choose your video and audio settings, then join"),
                            ),
                    )
                    .child(self.preview_box(cx))
                    .child(self.controls(cx))
                    .child(devices)
                    .child(audio_row)
                    .child(actions),
            )
    }
}
