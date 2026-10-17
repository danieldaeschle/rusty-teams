use gpui_kit::*;

pub fn long_press_right_click() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        |_, _, window, _| {
            window.on_mouse_event(|event: &LongPressEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || event.phase != TouchPhase::Started
                    || window.default_prevented()
                {
                    return;
                }
                window.prevent_default();
                let position = event.start_position;
                window.defer(cx, move |window, cx| {
                    window.dispatch_event(
                        PlatformInput::MouseDown(MouseDownEvent {
                            button: MouseButton::Right,
                            position,
                            modifiers: Modifiers::default(),
                            click_count: 1,
                            first_mouse: false,
                        }),
                        cx,
                    );
                    window.dispatch_event(
                        PlatformInput::MouseUp(MouseUpEvent {
                            button: MouseButton::Right,
                            position,
                            modifiers: Modifiers::default(),
                            click_count: 1,
                        }),
                        cx,
                    );
                });
            });
        },
    )
}
