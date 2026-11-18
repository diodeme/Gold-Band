//! Save themes through the production dispatcher on Windows' default 1 MiB stack.
//! An abort stays inside a subprocess instead of taking down the full test suite.

use tauri::ipc::InvokeBody;

const CHILD_ENV: &str = "GOLD_BAND_IPC_STACK_TEST_CHILD";

#[test]
fn theme_preferences_fit_windows_ui_stack() {
    if std::env::var_os(CHILD_ENV).is_none() {
        let output = gold_band::process::background_command(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ipc_stack_tests::theme_preferences_fit_windows_ui_stack",
                "--nocapture",
            ])
            .env(CHILD_ENV, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success()
                && String::from_utf8_lossy(&output.stderr).contains("native IPC assertions passed"),
            "native IPC child failed: {}\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        return;
    }

    std::thread::Builder::new()
        .name("desktop-main-default-stack".into())
        .stack_size(1024 * 1024)
        .spawn(run_native_ipc)
        .unwrap()
        .join()
        .unwrap();
}

fn run_native_ipc() {
    use tauri::Manager;
    let workspace = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(workspace.path().to_path_buf()).unwrap();
    let context = super::state::DesktopContext::from_workspace(root).unwrap();
    let app = tauri::Builder::default()
        .any_thread()
        .register_uri_scheme_protocol("stack-test", |_, _| {
            tauri::http::Response::builder()
                .header("Content-Type", "text/html")
                .body(b"<!doctype html><html><body>IPC regression</body></html>".to_vec())
                .unwrap()
        })
        .manage(super::state::DesktopState::new(context))
        .invoke_handler(|invoke| {
            if invoke.message.command() == "ipc_test_done" {
                let success = matches!(invoke.message.payload(), InvokeBody::Json(value) if value["success"] == true);
                eprintln!("native IPC result: {:?}", invoke.message.payload());
                if success { eprintln!("native IPC assertions passed"); }
                invoke.message.webview().app_handle().exit(if success { 0 } else { 1 });
                invoke.resolver.respond(Ok(()));
                return true;
            }
            super::desktop_invoke_handler(invoke)
        })
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let profile = tempfile::tempdir().unwrap();
    let _window = tauri::WebviewWindowBuilder::new(
        &app,
        "main",
        tauri::WebviewUrl::CustomProtocol("stack-test://localhost".parse().unwrap()),
    )
    // Match the production composition-mode WebView2 callback stack. An opaque
    // window misses the regression even with the same thread stack reservation.
    .transparent(true)
    .decorations(false)
    .visible(false)
    .data_directory(profile.path().to_path_buf())
    .on_page_load(|window, event| {
        if event.event() == tauri::webview::PageLoadEvent::Finished {
            window.eval(r#"
                (async () => {
                    const invoke = window.__TAURI_INTERNALS__.invoke;
                    try {
                        const bootstrap = await invoke('get_app_bootstrap');
                        let preferences = bootstrap.preferences;
                        // The first save must parse a cold catalog; do not initialize
                        // builtin_theme_catalog in the test process before this IPC.
                        for (const themeId of ['builtin.meadow', 'builtin.vista', 'builtin.tech-neutral', 'builtin.gold-band']) {
                            const appearance = {...preferences.appearance, themeId};
                            preferences = await invoke('save_desktop_preferences', {...preferences, appearance});
                            if (preferences.appearance.themeId !== themeId) throw new Error('theme was not saved');
                            if (preferences.appearance.colorScheme !== appearance.colorScheme) throw new Error('color scheme changed');
                            const reloaded = await invoke('get_app_bootstrap');
                            if (reloaded.preferences.appearance.themeId !== themeId) throw new Error('saved theme was not reloaded');
                        }
                        try {
                            await invoke('save_desktop_preferences', {...preferences,
                                appearance: {...preferences.appearance, themeId: 'builtin.missing'}});
                            throw new Error('accepted an unknown theme');
                        } catch (error) {
                            if (error.code !== 'theme.active-package-missing') throw error;
                        }
                        await invoke('ipc_test_done', {success: true});
                    } catch (error) { await invoke('ipc_test_done', {success: false, error: String(error)}); }
                })();
            "#).unwrap();
        }
    })
    .build()
    .unwrap();
    let handle = app.handle().clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let watchdog = std::thread::spawn(move || {
        if done_rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .is_err()
        {
            handle.exit(2);
        }
    });
    assert_eq!(app.run_return(|_, _| {}), 0);
    let _ = done_tx.send(());
    watchdog.join().unwrap();
}
