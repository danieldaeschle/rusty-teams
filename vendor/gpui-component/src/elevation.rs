//! Fluent 2 dark shadows as Teams paints them. GPUI's shader takes the blur as
//! the gaussian's sigma while CSS takes twice that, so every CSS blur is halved.

use std::collections::HashMap;

use gpui::{App, BoxShadow, Global, Hsla, Window, WindowId, hsla, point, px};

use crate::ActiveTheme as _;

const AMBIENT_INK: f32 = 0.24;
const KEY_INK: f32 = 0.28;

#[derive(Clone, Debug, PartialEq)]
pub struct Elevation {
    pub fill: Hsla,
    pub hover: Hsla,
    pub shadow: Vec<BoxShadow>,
}

impl Elevation {
    pub fn shadow_at(&self, strength: f32) -> Vec<BoxShadow> {
        let strength = strength.clamp(0., 1.);
        self.shadow
            .iter()
            .cloned()
            .map(|mut layer| {
                layer.color = layer.color.alpha(layer.color.a * strength);
                layer
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ElevationTokens {
    pub flyouts: Vec<Elevation>,
    pub dialog: Elevation,
    pub toast: Elevation,
}

impl Global for ElevationTokens {}

fn fluent_shadow(
    ambient_blur: f32,
    key_offset: f32,
    key_blur: f32,
    ambient_ink: f32,
    key_ink: f32,
) -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: hsla(0., 0., 0., ambient_ink),
            offset: point(px(0.), px(0.)),
            blur_radius: px(ambient_blur / 2.),
            spread_radius: px(0.),
            inset: false,
        },
        BoxShadow {
            color: hsla(0., 0., 0., key_ink),
            offset: point(px(0.), px(key_offset)),
            blur_radius: px(key_blur / 2.),
            spread_radius: px(0.),
            inset: false,
        },
    ]
}

pub fn shadow8() -> Vec<BoxShadow> {
    fluent_shadow(2., 4., 8., AMBIENT_INK, KEY_INK)
}

pub fn shadow16() -> Vec<BoxShadow> {
    fluent_shadow(2., 8., 16., AMBIENT_INK, KEY_INK)
}

pub fn shadow28() -> Vec<BoxShadow> {
    fluent_shadow(8., 14., 28., AMBIENT_INK, KEY_INK)
}

pub fn shadow64() -> Vec<BoxShadow> {
    fluent_shadow(8., 32., 64., AMBIENT_INK, KEY_INK)
}

impl ElevationTokens {
    pub fn from_theme(cx: &App) -> Self {
        let theme = cx.theme();
        let flyout = Elevation {
            fill: theme.popover,
            hover: theme.sidebar_accent,
            shadow: shadow16(),
        };
        let nested = Elevation {
            fill: theme.sidebar_accent,
            hover: theme.secondary_hover,
            shadow: shadow16(),
        };
        Self {
            flyouts: vec![flyout.clone(), nested],
            dialog: Elevation {
                shadow: shadow64(),
                ..flyout.clone()
            },
            toast: Elevation {
                shadow: shadow8(),
                ..flyout
            },
        }
    }

    pub fn flyout_at(&self, modal_layers: usize) -> &Elevation {
        let last = self.flyouts.len().saturating_sub(1);
        &self.flyouts[modal_layers.min(last)]
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
struct ModalLayers {
    app: usize,
    framework: usize,
}

impl ModalLayers {
    fn total(self) -> usize {
        self.app + self.framework
    }
}

#[derive(Default)]
struct WindowModalLayers(HashMap<WindowId, ModalLayers>);

impl Global for WindowModalLayers {}

fn update_layers(window: &Window, cx: &mut App, update: impl FnOnce(&mut ModalLayers)) {
    let window_id = window.window_handle().window_id();
    let current = cx
        .try_global::<WindowModalLayers>()
        .and_then(|layers| layers.0.get(&window_id).copied())
        .unwrap_or_default();
    let mut next = current;
    update(&mut next);
    if next == current {
        return;
    }
    let layers = cx.default_global::<WindowModalLayers>();
    if next == ModalLayers::default() {
        layers.0.remove(&window_id);
    } else {
        layers.0.insert(window_id, next);
    }
}

pub fn set_app_modal_layers(window: &Window, count: usize, cx: &mut App) {
    update_layers(window, cx, |layers| layers.app = count);
}

pub(crate) fn set_framework_modal_layers(window: &Window, count: usize, cx: &mut App) {
    update_layers(window, cx, |layers| layers.framework = count);
}

pub fn modal_layers(window: &Window, cx: &App) -> usize {
    let window_id = window.window_handle().window_id();
    cx.try_global::<WindowModalLayers>()
        .and_then(|layers| layers.0.get(&window_id).copied())
        .map(ModalLayers::total)
        .unwrap_or(0)
}

pub fn tokens(cx: &App) -> ElevationTokens {
    cx.try_global::<ElevationTokens>()
        .cloned()
        .unwrap_or_else(|| ElevationTokens::from_theme(cx))
}

pub fn flyout(window: &Window, cx: &App) -> Elevation {
    tokens(cx).flyout_at(modal_layers(window, cx)).clone()
}

pub fn dialog(cx: &App) -> Elevation {
    tokens(cx).dialog
}

pub fn toast(cx: &App) -> Elevation {
    tokens(cx).toast
}
