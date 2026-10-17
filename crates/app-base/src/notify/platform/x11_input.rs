use std::sync::OnceLock;

use gpui_kit::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::connection::Connection as _;
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xproto::{AtomEnum, ClipOrdering, ConnectionExt as _, PropMode};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

const WINDOW_ICON_SIZES: [u32; 3] = [32, 64, 128];

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

pub fn compositor_running(display: Option<&str>) -> Result<bool, Box<dyn std::error::Error>> {
    let (connection, screen_number) = RustConnection::connect(display)?;
    let selection = format!("_NET_WM_CM_S{screen_number}");
    let atom = connection.intern_atom(false, selection.as_bytes())?.reply()?.atom;
    let owner = connection.get_selection_owner(atom)?.reply()?.owner;
    Ok(owner != x11rb::NONE)
}

pub fn window_transparency_available(window: &Window) -> bool {
    static COMPOSITED: OnceLock<bool> = OnceLock::new();
    match HasWindowHandle::window_handle(window).map(|handle| handle.as_raw()) {
        Ok(RawWindowHandle::Xcb(_) | RawWindowHandle::Xlib(_)) => {
            *COMPOSITED.get_or_init(|| compositor_running(None).unwrap_or(false))
        }
        _ => true,
    }
}

pub fn window_icon_property(png: &[u8]) -> Option<Vec<u32>> {
    let decoded = image::load_from_memory(png).ok()?;
    let mut property = Vec::new();
    for size in WINDOW_ICON_SIZES {
        let resized = decoded
            .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        property.extend([size, size]);
        property.extend(resized.pixels().map(|pixel| {
            let [red, green, blue, alpha] = pixel.0;
            u32::from_le_bytes([blue, green, red, alpha])
        }));
    }
    Some(property)
}

pub fn write_window_icon(
    display: Option<&str>,
    window: u32,
    property: &[u32],
) -> Result<(), Box<dyn std::error::Error>> {
    let (connection, _) = RustConnection::connect(display)?;
    let atom = connection.intern_atom(false, b"_NET_WM_ICON")?.reply()?.atom;
    connection
        .change_property32(PropMode::REPLACE, window, atom, AtomEnum::CARDINAL, property)?
        .check()?;
    connection.flush()?;
    Ok(())
}

pub fn set_window_icon(window: &Window, png: &[u8]) {
    if let (Some(id), Some(property)) = (x11_window_id(window), window_icon_property(png)) {
        let _ = write_window_icon(None, id, &property);
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;

    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{CreateWindowAux, WindowClass};
    use x11rb::COPY_DEPTH_FROM_PARENT;

    use super::*;

    const SERVER_START_TIMEOUT: Duration = Duration::from_secs(5);

    struct Server(Child);

    impl Drop for Server {
        fn drop(&mut self) {
            // SIGKILL leaves /tmp/.X<n>-lock behind, and the next run then cannot claim that display.
            let _ = Command::new("kill").arg(self.0.id().to_string()).status();
            let _ = self.0.wait();
        }
    }

    fn start_server() -> Option<(Server, String)> {
        let mut child = Command::new("Xvfb")
            .args(["-displayfd", "1", "-screen", "0", "320x200x24", "-nolisten", "tcp", "-noreset"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let stdout = child.stdout.take()?;
        let server = Server(child);
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut number = String::new();
            let _ = BufReader::new(stdout).read_line(&mut number);
            let _ = sender.send(number);
        });
        let number = receiver.recv_timeout(SERVER_START_TIMEOUT).ok()?;
        let display = format!(":{}", number.trim());
        RustConnection::connect(Some(&display)).ok()?;
        Some((server, display))
    }

    #[test]
    fn the_border_window_gets_an_empty_input_region() {
        let Some((_server, display)) = start_server() else {
            return;
        };
        let (connection, screen_number) = RustConnection::connect(Some(&display)).unwrap();
        let root = connection.setup().roots[screen_number].root;
        let window = connection.generate_id().unwrap();
        connection
            .create_window(COPY_DEPTH_FROM_PARENT, window, root, 0, 0, 100, 100, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new())
            .unwrap();
        connection.flush().unwrap();
        let before = connection.shape_get_rectangles(window, SK::INPUT).unwrap().reply().unwrap();
        assert_eq!(before.rectangles.len(), 1);
        empty_input_region(Some(&display), window).unwrap();
        let after = connection.shape_get_rectangles(window, SK::INPUT).unwrap().reply().unwrap();
        assert!(after.rectangles.is_empty());
    }

    #[test]
    fn transparency_needs_a_compositor_owning_the_cm_selection() {
        let Some((_server, display)) = start_server() else {
            return;
        };
        assert!(!compositor_running(Some(&display)).unwrap());
        let (connection, screen_number) = RustConnection::connect(Some(&display)).unwrap();
        let root = connection.setup().roots[screen_number].root;
        let window = connection.generate_id().unwrap();
        connection
            .create_window(COPY_DEPTH_FROM_PARENT, window, root, 0, 0, 1, 1, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new())
            .unwrap();
        let selection = format!("_NET_WM_CM_S{screen_number}");
        let atom = connection.intern_atom(false, selection.as_bytes()).unwrap().reply().unwrap().atom;
        connection.set_selection_owner(window, atom, x11rb::CURRENT_TIME).unwrap().check().unwrap();
        assert!(compositor_running(Some(&display)).unwrap());
    }

    #[test]
    fn the_window_icon_property_holds_every_size_in_argb() {
        let png = crate::appearance::AppIcon::MicrosoftTeams.png();
        let property = window_icon_property(png).unwrap();
        let expected: usize = WINDOW_ICON_SIZES
            .iter()
            .map(|size| 2 + (size * size) as usize)
            .sum();
        assert_eq!(property.len(), expected);
        assert_eq!(&property[..2], &[32, 32]);
        assert!(window_icon_property(b"not a png").is_none());
    }

    #[test]
    fn the_window_icon_lands_in_net_wm_icon() {
        let Some((_server, display)) = start_server() else {
            return;
        };
        let (connection, screen_number) = RustConnection::connect(Some(&display)).unwrap();
        let root = connection.setup().roots[screen_number].root;
        let window = connection.generate_id().unwrap();
        connection
            .create_window(COPY_DEPTH_FROM_PARENT, window, root, 0, 0, 100, 100, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new())
            .unwrap();
        connection.flush().unwrap();
        let property = window_icon_property(crate::appearance::AppIcon::RustyTeams.png()).unwrap();
        write_window_icon(Some(&display), window, &property).unwrap();
        let atom = connection.intern_atom(false, b"_NET_WM_ICON").unwrap().reply().unwrap().atom;
        let reply = connection
            .get_property(false, window, atom, AtomEnum::CARDINAL, 0, u32::MAX)
            .unwrap()
            .reply()
            .unwrap();
        let stored: Vec<u32> = reply.value32().unwrap().collect();
        assert_eq!(stored, property);
    }
}
