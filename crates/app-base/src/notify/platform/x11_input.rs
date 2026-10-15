use gpui_kit::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xproto::ClipOrdering;
use x11rb::rust_connection::RustConnection;

fn x11_window_id(window: &Window) -> Option<u32> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Xcb(handle) => Some(handle.window.get()),
        RawWindowHandle::Xlib(handle) => u32::try_from(handle.window).ok(),
        _ => None,
    }
}

pub fn empty_input_region(display: Option<&str>, window: u32) -> Result<(), Box<dyn std::error::Error>> {
    let (connection, _) = RustConnection::connect(display)?;
    connection
        .shape_rectangles(SO::SET, SK::INPUT, ClipOrdering::UNSORTED, window, 0, 0, &[])?
        .check()?;
    Ok(())
}

pub fn make_input_transparent(window: &Window) {
    if let Some(id) = x11_window_id(window) {
        let _ = empty_input_region(None, id);
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Child, Command, Stdio};
    use std::time::Duration;

    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, WindowClass};
    use x11rb::COPY_DEPTH_FROM_PARENT;

    use super::*;

    struct Server(Child);

    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.0.kill();
        }
    }

    fn start_server(display: &str) -> Option<Server> {
        let child = Command::new("Xvfb")
            .args([display, "-screen", "0", "320x200x24", "-nolisten", "tcp", "-noreset"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let server = Server(child);
        for _ in 0..50 {
            if RustConnection::connect(Some(display)).is_ok() {
                return Some(server);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        None
    }

    #[test]
    fn the_border_window_gets_an_empty_input_region() {
        let display = ":87";
        let Some(_server) = start_server(display) else {
            return;
        };
        let (connection, screen_number) = RustConnection::connect(Some(display)).unwrap();
        let root = connection.setup().roots[screen_number].root;
        let window = connection.generate_id().unwrap();
        connection
            .create_window(COPY_DEPTH_FROM_PARENT, window, root, 0, 0, 100, 100, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new())
            .unwrap();
        connection.flush().unwrap();
        let before = connection.shape_get_rectangles(window, SK::INPUT).unwrap().reply().unwrap();
        assert_eq!(before.rectangles.len(), 1);
        empty_input_region(Some(display), window).unwrap();
        let after = connection.shape_get_rectangles(window, SK::INPUT).unwrap().reply().unwrap();
        assert!(after.rectangles.is_empty());
    }
}
