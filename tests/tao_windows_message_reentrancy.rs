//! Regression coverage for tao's Windows keyboard/IME message reentrancy fix.
//!
//! The original tao implementation called `PeekMessageW` from inside the
//! `KEY_EVENT_BUILDERS` mutex critical section. Win32 may dispatch a pending
//! sent message while peeking, so a second callback on the same UI thread
//! attempted to acquire that non-recursive mutex and hung forever. The child
//! process test below sends two concurrent key messages and has a hard parent
//! timeout so the old implementation fails as a bounded test failure instead
//! of wedging the whole test run.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

const TAO_FIX_REV: &str = "c704261c519c58cfdd0bc2d58ba24e06a0b71c92";

#[cfg(windows)]
fn tao_checkout() -> PathBuf {
    let cargo_home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(|profile| PathBuf::from(profile).join(".cargo")))
        .expect("CARGO_HOME or USERPROFILE must be available for the tao source contract test");
    let checkouts = cargo_home.join("git").join("checkouts");
    let entries = fs::read_dir(&checkouts).unwrap_or_else(|error| {
        panic!(
            "cannot inspect tao git checkouts at {}: {error}",
            checkouts.display()
        )
    });

    for entry in entries.flatten() {
        let path = entry.path();
        if !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("tao-"))
        {
            continue;
        }
        let Ok(revisions) = fs::read_dir(&path) else {
            continue;
        };
        for revision in revisions.flatten() {
            let candidate = revision.path();
            if candidate.join("Cargo.toml").is_file()
                && candidate
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        TAO_FIX_REV.starts_with(name) || name.starts_with(&TAO_FIX_REV[..7])
                    })
            {
                return candidate;
            }
        }
    }

    panic!(
        "tao checkout for revision {TAO_FIX_REV} was not found under {}",
        checkouts.display()
    );
}

#[cfg(windows)]
fn section<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_at = source
        .find(start)
        .unwrap_or_else(|| panic!("tao source is missing section marker {start:?}"));
    let body = &source[start_at..];
    let end_at = body
        .find(end)
        .unwrap_or_else(|| panic!("tao source section {start:?} has no end marker {end:?}"));
    &body[..end_at]
}

fn read_tao_file(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read tao source file {}: {error}", path.display()))
}

#[test]
fn tao_dependency_is_pinned_to_the_reentrancy_fix() {
    let lock = read_tao_file(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock"));
    let source = format!("git+https://github.com/tauri-apps/tao?rev={TAO_FIX_REV}#{TAO_FIX_REV}");
    assert!(
        lock.contains(&source),
        "Cargo.lock must resolve tao from the audited fix revision {TAO_FIX_REV}"
    );
}

#[test]
#[cfg(windows)]
fn tao_keyboard_and_ime_peeks_are_outside_reentrant_locks() {
    let root = tao_checkout()
        .join("src")
        .join("platform_impl")
        .join("windows");
    let event_loop = read_tao_file(root.join("event_loop.rs"));
    let keyboard = read_tao_file(root.join("keyboard.rs"));

    let keyboard_callback = section(
        &event_loop,
        "let keyboard_callback = || {",
        "let ime_callback = || {",
    );
    let peek = keyboard_callback
        .find("next_key_message_for_keyboard(window, msg, wparam)")
        .expect("keyboard callback must prefetch the next message");
    let lock = keyboard_callback
        .find("KEY_EVENT_BUILDERS.lock()")
        .expect("keyboard callback must still protect its builder map");
    assert!(
        peek < lock,
        "keyboard message peeking must happen before KEY_EVENT_BUILDERS.lock()"
    );
    assert!(
        keyboard_callback.contains("process_message(msg, wparam, lparam, next_key_message"),
        "prefetched message must be passed into the locked key-event builder"
    );
    assert!(
        !keyboard.contains("PeekMessageW("),
        "keyboard.rs must not call the reentrant Win32 peek API itself"
    );

    let ime_callback = section(
        &event_loop,
        "let ime_callback = || {",
        "// I decided to bind",
    );
    let ime_peek = ime_callback
        .find("more_ime_char_coming(window, msg)")
        .expect("IME callback must prefetch the next character outside its lock");
    let ime_lock = ime_callback
        .find("subclass_input.window_state.lock()")
        .expect("IME callback must protect window state");
    assert!(
        ime_peek < ime_lock,
        "IME message peeking must happen before window_state.lock()"
    );
}

#[cfg(windows)]
mod windows_runtime {
    use super::*;
    use std::{
        process::{Child, Command},
        sync::{Arc, Barrier},
        thread,
        time::{Duration, Instant},
    };

    use tao::{
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoopBuilder},
        platform::windows::{EventLoopBuilderExtWindows, WindowExtWindows},
        window::WindowBuilder,
    };
    use windows::Win32::{
        Foundation::{HWND, WPARAM},
        UI::WindowsAndMessaging::{SendMessageW, WM_KEYDOWN},
    };

    fn wait_with_timeout(child: &mut Child, timeout: Duration) -> std::process::ExitStatus {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = child
                .try_wait()
                .expect("failed to poll tao regression child")
            {
                return status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("tao reentrancy child did not exit within {timeout:?}; likely deadlocked");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn windows_message_reentrancy_regression_is_bounded() {
        let mut child = Command::new(env::current_exe().expect("test executable path"))
            .args([
                "--exact",
                "windows_runtime::tao_windows_message_reentrancy_child",
                "--nocapture",
            ])
            .env("GOLD_BAND_TAO_REENTRANCY_CHILD", "1")
            .spawn()
            .expect("failed to start tao reentrancy child");
        let status = wait_with_timeout(&mut child, Duration::from_secs(10));
        assert!(
            status.success(),
            "tao reentrancy child failed with {status}"
        );
    }

    #[test]
    fn tao_windows_message_reentrancy_child() {
        if env::var_os("GOLD_BAND_TAO_REENTRANCY_CHILD").is_none() {
            // The harness discovers this function in the parent process too;
            // only the explicitly spawned child may create a native event loop.
            return;
        }

        let mut event_loop_builder = EventLoopBuilder::new();
        event_loop_builder.with_any_thread(true);
        let event_loop = event_loop_builder.build();
        let window = WindowBuilder::new()
            .with_visible(false)
            .with_title("Gold Band tao reentrancy regression")
            .build(&event_loop)
            .expect("failed to create tao regression window");
        let hwnd = window.hwnd();

        let barrier = Arc::new(Barrier::new(3));
        for _ in 0..2 {
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                unsafe {
                    // Two simultaneous sent messages leave one pending while tao handles
                    // the other. Old tao dispatched that pending message from PeekMessageW
                    // while holding KEY_EVENT_BUILDERS, recursively acquiring the mutex.
                    let _ = SendMessageW(
                        HWND(hwnd as *mut _),
                        WM_KEYDOWN,
                        Some(WPARAM(usize::from(b'A'))),
                        None,
                    );
                }
            });
        }
        barrier.wait();

        let mut key_events = 0_u8;
        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;
            if let Event::WindowEvent {
                event: WindowEvent::KeyboardInput { .. },
                ..
            } = event
            {
                key_events = key_events.saturating_add(1);
                if key_events >= 2 {
                    std::process::exit(0);
                }
            }
        });
    }
}
