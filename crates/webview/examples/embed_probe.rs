#[cfg(not(windows))]
fn main() {
    eprintln!("embed_probe runs on Windows only");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() {
    probe::run();
}

#[cfg(windows)]
mod probe {
    use std::time::{Duration, Instant};

    use tokio::sync::mpsc::UnboundedReceiver;
    use webview::{EmbedBounds, EmbedEvent, EmbedHost, EmbedSpec, MainEmbed};
    use windows::Win32::Foundation::{HINSTANCE, HWND, RECT};
    use windows::Win32::Graphics::Gdi::{CreateRectRgn, DeleteObject, GetWindowRgn, HGDIOBJ};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::core::{PCWSTR, w};

    const GLOBAL_TIMEOUT: Duration = Duration::from_secs(20);
    const STEP_TIMEOUT: Duration = Duration::from_secs(15);
    const PAGE_URL: &str = "data:text/html,%3Ctitle%3Eprobe%3C%2Ftitle%3E%3Ch1%3Eprobe%3C%2Fh1%3E";
    const PARENT_CLASS: PCWSTR = w!("EmbedProbeParent");
    const CONTAINER_CLASS: PCWSTR = w!("RustyTeamsEmbedContainer");
    const ERROR_REGION: i32 = 0;
    const COMPLEX_REGION: i32 = 3;

    struct Probe {
        started: Instant,
        events: UnboundedReceiver<EmbedEvent>,
        seen: Vec<EmbedEvent>,
    }

    impl Probe {
        fn wait_for(
            &mut self,
            label: &str,
            mut done: impl FnMut(&[EmbedEvent]) -> bool,
        ) -> Result<Duration, String> {
            let step_start = Instant::now();
            let deadline = step_start + STEP_TIMEOUT;
            loop {
                pump_messages();
                while let Ok(event) = self.events.try_recv() {
                    self.seen.push(event);
                }
                if self.seen.contains(&EmbedEvent::Closed) {
                    return Err(format!("{label}: embed closed itself"));
                }
                if done(&self.seen) {
                    return Ok(step_start.elapsed());
                }
                if self.started.elapsed() > GLOBAL_TIMEOUT || Instant::now() > deadline {
                    return Err(format!("{label}: timed out"));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        fn settle(&mut self, label: &str, duration: Duration) -> Result<(), String> {
            let end = Instant::now() + duration;
            self.wait_for(label, |_| Instant::now() >= end).map(|_| ())
        }
    }

    pub fn run() {
        let started = Instant::now();
        let outcome = execute(started);
        match outcome {
            Ok(()) => {
                println!("PASS total={:?}", started.elapsed());
                std::process::exit(0);
            }
            Err(reason) => {
                println!("FAIL {reason} total={:?}", started.elapsed());
                std::process::exit(1);
            }
        }
    }

    fn execute(started: Instant) -> Result<(), String> {
        let parent = create_hidden_parent().map_err(|error| format!("parent window: {error}"))?;
        let folder = std::env::temp_dir().join("rusty-teams-embed-probe-data");
        let host = EmbedHost::new(folder);
        let (embed, events) = host
            .open(EmbedSpec {
                parent: parent.0 as isize,
                url: PAGE_URL.to_owned(),
                background: 0x1f1f1f,
            })
            .ok_or("open returned no embed")?;
        let mut probe = Probe {
            started,
            events,
            seen: Vec::new(),
        };
        let container = find_container(parent).ok_or("container window missing after open")?;
        embed.place(bounds(0, 0, 640, 480), true);
        let loaded = probe.wait_for("navigation", |seen| seen.contains(&EmbedEvent::Loaded))?;
        println!("navigation_completed after {loaded:?}");

        embed.place(bounds(10, 20, 800, 600), true);
        probe.settle("resize settle", Duration::from_millis(200))?;
        let rect = window_rect(container);
        expect(
            (rect.right - rect.left, rect.bottom - rect.top) == (800, 600),
            format!(
                "resize: container is {}x{}",
                rect.right - rect.left,
                rect.bottom - rect.top
            ),
        )?;
        println!("resize ok 800x600");

        embed.place(bounds(10, 20, 800, 600), false);
        probe.settle("hide settle", Duration::from_millis(100))?;
        expect(
            !style_visible(container),
            "visibility: container still shown after hide".into(),
        )?;
        embed.place(bounds(10, 20, 800, 600), true);
        probe.settle("show settle", Duration::from_millis(100))?;
        expect(
            style_visible(container),
            "visibility: container hidden after show".into(),
        )?;
        println!("visibility toggle ok");

        embed.set_cutouts(vec![bounds(500, 400, 300, 200)]);
        expect(
            region_kind(container) == COMPLEX_REGION,
            format!(
                "region: expected complex region, got {}",
                region_kind(container)
            ),
        )?;
        embed.set_cutouts(Vec::new());
        expect(
            region_kind(container) == ERROR_REGION,
            format!(
                "region: expected cleared region, got {}",
                region_kind(container)
            ),
        )?;
        println!("region apply/clear ok");

        close(&embed, &mut probe, parent)?;
        println!("close ok");
        unsafe {
            let _ = DestroyWindow(parent);
        }
        Ok(())
    }

    fn close(embed: &MainEmbed, probe: &mut Probe, parent: HWND) -> Result<(), String> {
        embed.close();
        probe.settle("close settle", Duration::from_millis(100))?;
        expect(
            find_container(parent).is_none(),
            "close: container still exists".into(),
        )
    }

    fn expect(condition: bool, failure: String) -> Result<(), String> {
        condition.then_some(()).ok_or(failure)
    }

    fn bounds(x: i32, y: i32, width: i32, height: i32) -> EmbedBounds {
        EmbedBounds {
            x,
            y,
            width,
            height,
        }
    }

    fn pump_messages() {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    fn create_hidden_parent() -> windows::core::Result<HWND> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(None) }?.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(parent_procedure),
            hInstance: instance,
            lpszClassName: PARENT_CLASS,
            ..Default::default()
        };
        unsafe { RegisterClassW(&class) };
        unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                PARENT_CLASS,
                w!("embed probe"),
                WS_OVERLAPPEDWINDOW,
                -32000,
                -32000,
                1000,
                800,
                None,
                None,
                Some(instance),
                None,
            )
        }
    }

    extern "system" fn parent_procedure(
        window: HWND,
        message: u32,
        wparam: windows::Win32::Foundation::WPARAM,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> windows::Win32::Foundation::LRESULT {
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    fn find_container(parent: HWND) -> Option<HWND> {
        unsafe { FindWindowExW(Some(parent), None, CONTAINER_CLASS, PCWSTR::null()) }
            .ok()
            .filter(|window| !window.is_invalid())
    }

    fn window_rect(window: HWND) -> RECT {
        let mut rect = RECT::default();
        unsafe {
            let _ = GetWindowRect(window, &mut rect);
        }
        rect
    }

    fn style_visible(window: HWND) -> bool {
        unsafe { GetWindowLongPtrW(window, GWL_STYLE) as u32 & WS_VISIBLE.0 != 0 }
    }

    fn region_kind(window: HWND) -> i32 {
        unsafe {
            let probe_region = CreateRectRgn(0, 0, 0, 0);
            let kind = GetWindowRgn(window, probe_region);
            let _ = DeleteObject(HGDIOBJ(probe_region.0));
            kind.0
        }
    }
}
