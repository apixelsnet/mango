//! Mango as a desktop app: a window and a tray icon around the `mango` binary.
//!
//! The shell starts `mango` from the folder it sits in, waits until it answers, and
//! shows it. Closing the window leaves Mango running in the tray, so downloads and
//! library updates carry on; Quit in the tray menu stops it. A Mango that is running
//! already is shown, not started twice.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tao::{
    dpi::LogicalSize,
    event::{Event, StartCause, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::{Window, WindowBuilder},
};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use wry::{WebContext, WebViewBuilder};

const CORE: &str = if cfg!(windows) { "mango.exe" } else { "mango" };
/// How long Mango gets to answer after it was started.
const START_SECS: u64 = 60;
/// How long it gets to stop by itself before it is killed.
const STOP_SECS: u64 = 8;

/// What reaches the window from outside its own events.
enum Note {
    /// Mango answers at this address.
    Ready(String),
    Failed(String),
    Menu(MenuId),
    Tray(TrayIconEvent),
}

/// The folder the shell keeps things in: the browser's data, the log and, unless told otherwise, Mango's data.
fn home() -> PathBuf {
    let var = |name: &str| env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        var("APPDATA").unwrap_or_default().join("Mango")
    } else if cfg!(target_os = "macos") {
        var("HOME").unwrap_or_default().join("Library/Application Support/Mango")
    } else {
        var("XDG_DATA_HOME").unwrap_or_else(|| var("HOME").unwrap_or_default().join(".local/share")).join("mango")
    }
}

/// The folder the shell is in. `mango` and the extension host's jars are beside it.
fn folder() -> Result<PathBuf, String> {
    let exe = env::current_exe().map_err(|e| format!("Cannot tell where this program is: {e}"))?;
    Ok(exe.parent().map(Path::to_path_buf).unwrap_or_default())
}

/// A call of the `mango` binary, with the settings a desktop Mango runs under.
fn core(folder: &Path) -> Command {
    let mut command = Command::new(folder.join(CORE));
    command.current_dir(folder);
    // A `mango.toml` beside the program, or a data folder named from outside, is the
    // owner's choice. Otherwise the data goes where this system keeps an app's data.
    let chosen = env::var_os("MANGO_DATA").is_some() || env::var_os("MANGO_CONFIG").is_some() || folder.join("mango.toml").exists();
    if !chosen {
        command.env("MANGO_DATA", home().join("data"));
    }
    #[cfg(windows)]
    {
        // No console window for a program that has a window of its own.
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

/// Runs `mango <word>` and answers with what it printed.
fn ask(folder: &Path, word: &str) -> Result<String, String> {
    let output = core(folder).arg(word).stdin(Stdio::null()).output().map_err(|e| format!("Cannot run {CORE} beside this program: {e}"))?;
    match output.status.success() {
        true => Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned()),
        false => Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
    }
}

/// Starts Mango unless one answers already, and waits until it does. The address it answers at.
fn start(running: &Mutex<Option<Child>>) -> Result<String, String> {
    let folder = folder()?;
    if !folder.join(CORE).is_file() {
        return Err(format!("{CORE} is not in {}. It belongs beside this program.", folder.display()));
    }
    let address = ask(&folder, "address")?;
    if ask(&folder, "health").is_ok() {
        return Ok(address);
    }
    let log = home().join("mango.log");
    let _ = fs::create_dir_all(home());
    let out = fs::File::create(&log).map_err(|e| format!("Cannot write {}: {e}", log.display()))?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    // Mango stops when this pipe closes: on Quit, and when the shell dies.
    let child = core(&folder).env("MANGO_WATCH_STDIN", "1").stdin(Stdio::piped()).stdout(out).stderr(err).spawn();
    *running.lock().unwrap_or_else(|e| e.into_inner()) = Some(child.map_err(|e| format!("Cannot start {CORE}: {e}"))?);

    for _ in 0..START_SECS * 4 {
        thread::sleep(Duration::from_millis(250));
        let gone = running.lock().unwrap_or_else(|e| e.into_inner()).as_mut().is_some_and(|child| !matches!(child.try_wait(), Ok(None)));
        if gone {
            let said = fs::read_to_string(&log).unwrap_or_default();
            let last: Vec<&str> = said.lines().rev().take(6).collect();
            return Err(format!("Mango stopped while starting.\n{}", last.into_iter().rev().collect::<Vec<_>>().join("\n")));
        }
        if ask(&folder, "health").is_ok() {
            return Ok(address);
        }
    }
    Err(format!("Mango did not answer after {START_SECS} seconds. Its log is {}.", log.display()))
}

/// Lets go of Mango and waits for it to finish what it is writing.
fn stop(running: &Mutex<Option<Child>>) {
    let Some(mut child) = running.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
    drop(child.stdin.take());
    for _ in 0..STOP_SECS * 10 {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Opens an address in the system's browser.
fn browse(address: &str) {
    if !(address.starts_with("http://") || address.starts_with("https://")) {
        return;
    }
    let mut command = if cfg!(windows) {
        let mut command = Command::new("rundll32");
        command.args(["url.dll,FileProtocolHandler", address]);
        command
    } else {
        let mut command = Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" });
        command.arg(address);
        command
    };
    let _ = command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

/// Mango's mark: a spot of colour inside an ink ring. `size` pixels square, as RGBA.
fn mark(size: u32) -> Vec<u8> {
    let (centre, outer) = ((size as f32 - 1.0) / 2.0, size as f32 / 2.0 - 1.0);
    let inner = outer - (size as f32 / 8.0).max(2.0);
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let far = ((x as f32 - centre).powi(2) + (y as f32 - centre).powi(2)).sqrt();
            pixels.extend_from_slice(match far {
                far if far <= inner => &[0xFF, 0xC4, 0x00, 0xFF],
                far if far <= outer => &[0x14, 0x14, 0x14, 0xFF],
                _ => &[0, 0, 0, 0],
            });
        }
    }
    pixels
}

/// A page the window shows while there is no Mango to show, as an address that carries it.
fn page(title: &str, text: &str) -> String {
    let safe = |text: &str| text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let html = format!(
        "<!doctype html><meta charset=utf-8><title>Mango</title>\
         <body style='margin:0;display:grid;place-items:center;height:100vh;background:#F4EFE4;color:#141414;font:16px system-ui,sans-serif'>\
         <div style='max-width:560px;padding:24px'>\
         <div style='width:36px;height:36px;border:4px solid #141414;border-radius:50%;background:#FFC400'></div>\
         <h1 style='font-size:28px;margin:16px 0 8px'>{}</h1><p style='white-space:pre-wrap;line-height:1.5'>{}</p></div>",
        safe(title),
        safe(text),
    );
    let mut address = String::from("data:text/html;charset=utf-8,");
    for byte in html.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => address.push(byte as char),
            _ => address.push_str(&format!("%{byte:02X}")),
        }
    }
    address
}

fn show(window: &Window) {
    window.set_visible(true);
    window.set_minimized(false);
    window.set_focus();
}

fn main() {
    let event_loop = EventLoopBuilder::<Note>::with_user_event().build();
    let icon = |size| tao::window::Icon::from_rgba(mark(size), size, size).ok();
    let window = WindowBuilder::new()
        .with_title("Mango")
        .with_inner_size(LogicalSize::new(1280.0, 860.0))
        .with_min_inner_size(LogicalSize::new(360.0, 480.0))
        .with_window_icon(icon(64))
        .build(&event_loop)
        .expect("a window");

    // The browser inside the window keeps its cookies here, so a sign-in lasts.
    let mut context = WebContext::new(Some(home().join("webview")));
    let builder = WebViewBuilder::with_web_context(&mut context)
        .with_url(page("Starting Mango", "One moment."))
        // A link that wants a window of its own belongs in the system's browser.
        .with_new_window_req_handler(|address| {
            browse(&address);
            false
        });
    #[cfg(not(target_os = "linux"))]
    let webview = builder.build(&window).expect("a web view");
    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().expect("the window's box")).expect("a web view")
    };

    let running: Arc<Mutex<Option<Child>>> = Arc::default();
    {
        let (running, notes) = (running.clone(), event_loop.create_proxy());
        thread::spawn(move || {
            let _ = notes.send_event(start(&running).map_or_else(Note::Failed, Note::Ready));
        });
    }
    let notes = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = notes.send_event(Note::Menu(event.id));
    }));
    let notes = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = notes.send_event(Note::Tray(event));
    }));

    let open = MenuItem::new("Open Mango", true, None);
    let in_browser = MenuItem::new("Open in the browser", true, None);
    let quit = MenuItem::new("Quit", true, None);
    let mut tray: Option<TrayIcon> = None;
    let mut address: Option<String> = None;

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            // The tray icon can only be made once the system's event loop runs.
            Event::NewEvents(StartCause::Init) => {
                let menu = Menu::new();
                let _ = menu.append_items(&[&open, &in_browser, &PredefinedMenuItem::separator(), &quit]);
                let icon = tray_icon::Icon::from_rgba(mark(32), 32, 32).ok();
                tray = icon.and_then(|icon| TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("Mango").with_icon(icon).build().ok());
            }
            Event::UserEvent(Note::Ready(at)) => {
                let _ = webview.load_url(&at);
                address = Some(at);
            }
            Event::UserEvent(Note::Failed(why)) => {
                let _ = webview.load_url(&page("Mango could not start", &why));
                show(&window);
            }
            Event::UserEvent(Note::Menu(id)) if id == *open.id() => show(&window),
            Event::UserEvent(Note::Menu(id)) if id == *in_browser.id() => {
                if let Some(address) = &address {
                    browse(address);
                }
            }
            Event::UserEvent(Note::Menu(id)) if id == *quit.id() => {
                stop(&running);
                *flow = ControlFlow::Exit;
            }
            Event::UserEvent(Note::Tray(TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. })) => show(&window),
            // With a tray icon the window only hides; without one there would be no way back.
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => match tray {
                Some(_) => window.set_visible(false),
                None => {
                    stop(&running);
                    *flow = ControlFlow::Exit;
                }
            },
            _ => {}
        }
    });
}
