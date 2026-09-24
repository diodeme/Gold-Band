//! Same-document location changes (`history.pushState`, `replaceState`, back/forward,
//! hash) do not create a new document, so page-load events never see them. The address
//! bar still has to follow the engine's current top-level URL. The page itself is not
//! a trusted reporter: it has no Tauri IPC, and a script-supplied URL could disagree
//! with the document the engine is actually showing.

use tauri::{AppHandle, Manager};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DocumentLocationDecision {
    pub publish_url: Option<String>,
    pub record_visit: bool,
}

pub(crate) fn decide_document_location(
    new_document: bool,
    display_url: &str,
    previous_location: Option<&str>,
    previous_visit: Option<&str>,
) -> DocumentLocationDecision {
    let skip = DocumentLocationDecision {
        publish_url: None,
        record_visit: false,
    };
    if new_document {
        return skip;
    }
    let display_url = display_url.trim();
    if display_url.is_empty() {
        return skip;
    }
    let Ok(parsed) = Url::parse(display_url) else {
        return skip;
    };
    if !is_reflectable_document_url(&parsed) {
        return skip;
    }
    let canonical = parsed.to_string();
    if previous_location == Some(canonical.as_str()) {
        return skip;
    }
    let visit = crate::browser_history::http_visit_url(&parsed);
    let record_visit = visit
        .as_deref()
        .is_some_and(|next| previous_visit != Some(next));
    DocumentLocationDecision {
        publish_url: Some(canonical),
        record_visit,
    }
}

pub(crate) fn document_href_from_script_result(result: &str) -> Option<String> {
    let result = result.trim();
    if result.is_empty() || result == "null" {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(result).ok()?;
    let href = value.as_str()?.trim();
    if href.is_empty() {
        None
    } else {
        Some(href.to_string())
    }
}

fn is_reflectable_document_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "file")
        || crate::browser::is_browser_local_file_url(url)
}

pub(crate) fn attach_document_location_watch(app: &AppHandle, page_id: &str, label: &str) {
    #[cfg(windows)]
    attach_windows_document_location_watch(app, page_id, label);
    #[cfg(target_os = "macos")]
    attach_macos_document_location_watch(app, page_id, label);
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    attach_unix_document_location_watch(app, page_id, label);
    #[cfg(not(any(
        windows,
        target_os = "macos",
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )))]
    let _ = (app, page_id, label);
}

pub(crate) fn detach_document_location_watch(app: &AppHandle, label: &str) {
    #[cfg(target_os = "macos")]
    detach_macos_document_location_watch(app, label);
    #[cfg(not(target_os = "macos"))]
    let _ = (app, label);
}

#[cfg(windows)]
fn attach_windows_document_location_watch(app: &AppHandle, page_id: &str, label: &str) {
    use tauri::webview::PlatformWebview;
    use webview2_com::SourceChangedEventHandler;
    use windows::core::{BOOL, PWSTR};

    let Some(webview) = app.get_webview(label) else {
        return;
    };
    let app = app.clone();
    let page_id = page_id.to_string();
    let page_label = label.to_string();
    let result = webview.with_webview(move |platform: PlatformWebview| {
        let core = match unsafe { platform.controller().CoreWebView2() } {
            Ok(core) => core,
            Err(error) => {
                crate::browser::log_document_location_failure(&page_label, &error);
                return;
            }
        };
        let mut token = 0_i64;
        let source_app = app.clone();
        let source_page = page_id.clone();
        let handler = SourceChangedEventHandler::create(Box::new(move |webview, args| {
            let Some(webview) = webview else {
                return Ok(());
            };
            let Some(args) = args else {
                return Ok(());
            };
            let mut is_new = BOOL::default();
            unsafe { args.IsNewDocument(&mut is_new)? };
            let mut source = PWSTR::null();
            unsafe { webview.Source(&mut source)? };
            let raw = take_pwstr(source);
            crate::browser::publish_same_document_location(
                &source_app,
                &source_page,
                &raw,
                is_new.as_bool(),
                None,
            );
            Ok(())
        }));
        if let Err(error) = unsafe { core.add_SourceChanged(&handler, &mut token) } {
            crate::browser::log_document_location_failure(&page_label, &error);
        }
        // Source stays on the last full document load for many history.pushState
        // navigations, so SourceChanged never carries /zh/documentation. HistoryChanged
        // still runs; location.href is the document URL the page is actually on.
        let history_app = app.clone();
        let history_page = page_id.clone();
        let history_label = page_label.clone();
        let history =
            webview2_com::HistoryChangedEventHandler::create(Box::new(move |webview, _| {
                let Some(webview) = webview else {
                    return Ok(());
                };
                let Some(probe) = crate::browser::begin_location_probe(&history_app, &history_page)
                else {
                    return Ok(());
                };
                let app = history_app.clone();
                let page_id = history_page.clone();
                let script = windows::core::HSTRING::from("location.href");
                let completed = webview2_com::ExecuteScriptCompletedHandler::create(Box::new(
                    move |error, result| {
                        if error.is_ok() {
                            if let Some(href) = document_href_from_script_result(&result) {
                                crate::browser::publish_same_document_location(
                                    &app,
                                    &page_id,
                                    &href,
                                    false,
                                    Some(probe),
                                );
                            }
                        }
                        Ok(())
                    },
                ));
                if let Err(error) = unsafe { webview.ExecuteScript(&script, &completed) } {
                    crate::browser::log_document_location_failure(&history_label, &error);
                }
                Ok(())
            }));
        let mut history_token = 0_i64;
        if let Err(error) = unsafe { core.add_HistoryChanged(&history, &mut history_token) } {
            crate::browser::log_document_location_failure(&page_label, &error);
        }
    });
    if let Err(error) = result {
        crate::browser::log_document_location_failure(label, &error);
    }
}

#[cfg(windows)]
fn take_pwstr(ptr: windows::core::PWSTR) -> String {
    use windows::Win32::System::Com::CoTaskMemFree;

    if ptr.is_null() {
        return String::new();
    }
    let value = unsafe { ptr.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(ptr.0.cast())) };
    value
}

#[cfg(target_os = "macos")]
fn attach_macos_document_location_watch(app: &AppHandle, page_id: &str, label: &str) {
    use std::ffi::c_void;

    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2_foundation::{NSKeyValueObservingOptions, NSString};
    use tauri::webview::PlatformWebview;

    let Some(webview) = app.get_webview(label) else {
        return;
    };
    let app = app.clone();
    let page_id = page_id.to_string();
    let page_label = label.to_string();
    let result = webview.with_webview(move |platform: PlatformWebview| {
        let Some(view) = macos_webkit_view(&platform) else {
            crate::browser::log_document_location_failure(&page_label, "missing wkwebview");
            return;
        };
        let observer = LocationObserver::new(app, page_id);
        let key = NSString::from_str("URL");
        unsafe {
            let () = msg_send![
                view,
                addObserver: &*observer,
                forKeyPath: &*key,
                options: NSKeyValueObservingOptions::New,
                context: std::ptr::null_mut::<c_void>()
            ];
        }
        store_macos_location_observer(&page_label, observer);
    });
    if let Err(error) = result {
        crate::browser::log_document_location_failure(label, &error);
    }
}

#[cfg(target_os = "macos")]
fn detach_macos_document_location_watch(app: &AppHandle, label: &str) {
    use std::ffi::c_void;

    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2_foundation::NSString;
    use tauri::webview::PlatformWebview;

    let Some(raw) = take_macos_location_observer(label) else {
        return;
    };
    let Some(webview) = app.get_webview(label) else {
        drop(unsafe { Retained::from_raw(raw) });
        return;
    };
    let result = webview.with_webview(move |platform: PlatformWebview| {
        let observer = unsafe { Retained::from_raw(raw) };
        let Some(observer) = observer else {
            return;
        };
        let Some(view) = macos_webkit_view(&platform) else {
            return;
        };
        let key = NSString::from_str("URL");
        unsafe {
            let () = msg_send![
                view,
                removeObserver: &*observer,
                forKeyPath: &*key,
                context: std::ptr::null_mut::<c_void>()
            ];
        }
        drop(observer);
    });
    if let Err(error) = result {
        crate::browser::log_document_location_failure(label, &error);
    }
}

#[cfg(target_os = "macos")]
fn macos_webkit_view(
    platform: &tauri::webview::PlatformWebview,
) -> Option<&objc2_web_kit::WKWebView> {
    let inner = platform.inner();
    if inner.is_null() {
        return None;
    }
    Some(unsafe { &*inner.cast::<objc2_web_kit::WKWebView>() })
}

#[cfg(target_os = "macos")]
fn store_macos_location_observer(label: &str, observer: objc2::rc::Retained<LocationObserver>) {
    let ptr = objc2::rc::Retained::into_raw(observer) as usize;
    if let Ok(mut watches) = macos_location_observers().lock() {
        watches.insert(label.to_string(), ptr);
    }
}

#[cfg(target_os = "macos")]
fn take_macos_location_observer(label: &str) -> Option<*mut LocationObserver> {
    let mut watches = macos_location_observers().lock().ok()?;
    let ptr = watches.remove(label)?;
    Some(ptr as *mut LocationObserver)
}

#[cfg(target_os = "macos")]
fn macos_location_observers() -> &'static std::sync::Mutex<std::collections::HashMap<String, usize>>
{
    use std::sync::OnceLock;
    static WATCHES: OnceLock<std::sync::Mutex<std::collections::HashMap<String, usize>>> =
        OnceLock::new();
    WATCHES.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

#[cfg(target_os = "macos")]
struct LocationObserverIvars {
    app: AppHandle,
    page_id: String,
}

#[cfg(target_os = "macos")]
objc2::define_class!(
    #[unsafe(super(objc2_foundation::NSObject))]
    #[ivars = LocationObserverIvars]
    struct LocationObserver;

    impl LocationObserver {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe_document_location(
            &self,
            _key_path: Option<&objc2_foundation::NSString>,
            object: Option<&objc2::runtime::AnyObject>,
            _change: Option<
                &objc2_foundation::NSDictionary<
                    objc2_foundation::NSKeyValueChangeKey,
                    objc2::runtime::AnyObject,
                >,
            >,
            _context: *mut std::ffi::c_void,
        ) {
            let Some(object) = object else {
                return;
            };
            let loading: bool = unsafe { objc2::msg_send![object, isLoading] };
            let url: Option<objc2::rc::Retained<objc2_foundation::NSURL>> =
                unsafe { objc2::msg_send![object, URL] };
            let Some(url) = url.and_then(|value| value.absoluteString()) else {
                return;
            };
            let ivars = self.ivars();
            crate::browser::publish_same_document_location(
                &ivars.app,
                &ivars.page_id,
                &url.to_string(),
                loading,
                None,
            );
        }
    }

    unsafe impl objc2::runtime::NSObjectProtocol for LocationObserver {}
);

#[cfg(target_os = "macos")]
impl LocationObserver {
    fn new(app: AppHandle, page_id: String) -> objc2::rc::Retained<Self> {
        use objc2::ClassType;
        use objc2::msg_send;
        use objc2::rc::Retained;

        let this = Self::alloc().set_ivars(LocationObserverIvars { app, page_id });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn attach_unix_document_location_watch(app: &AppHandle, page_id: &str, label: &str) {
    use tauri::webview::PlatformWebview;
    use webkit2gtk::WebViewExt;

    let Some(webview) = app.get_webview(label) else {
        return;
    };
    let app = app.clone();
    let page_id = page_id.to_string();
    let result = webview.with_webview(move |platform: PlatformWebview| {
        let view = platform.inner();
        let app = app.clone();
        let page_id = page_id.clone();
        view.connect_uri_notify(move |view| {
            let Some(uri) = view.uri() else {
                return;
            };
            // A full document load also changes `uri`, but `is_loading` is set for that
            // load. pushState/replaceState/hash update `uri` while the view is idle.
            crate::browser::publish_same_document_location(
                &app,
                &page_id,
                uri.as_str(),
                view.is_loading(),
                None,
            );
        });
    });
    if let Err(error) = result {
        crate::browser::log_document_location_failure(label, &error);
    }
}

#[cfg(test)]
mod tests {
    use super::decide_document_location;

    #[test]
    fn same_document_path_change_updates_location_without_recording_hash_only_edits() {
        let path = decide_document_location(
            false,
            "https://gold-band.dion.blue/zh/documentation",
            Some("https://gold-band.dion.blue/zh/demo"),
            Some("https://gold-band.dion.blue/zh/demo"),
        );
        assert_eq!(
            path.publish_url.as_deref(),
            Some("https://gold-band.dion.blue/zh/documentation")
        );
        assert!(path.record_visit);

        let hash = decide_document_location(
            false,
            "https://gold-band.dion.blue/zh/demo#install",
            Some("https://gold-band.dion.blue/zh/demo"),
            Some("https://gold-band.dion.blue/zh/demo"),
        );
        assert_eq!(
            hash.publish_url.as_deref(),
            Some("https://gold-band.dion.blue/zh/demo#install")
        );
        assert!(!hash.record_visit);

        let duplicate = decide_document_location(
            false,
            "https://gold-band.dion.blue/zh/documentation",
            Some("https://gold-band.dion.blue/zh/documentation"),
            Some("https://gold-band.dion.blue/zh/documentation"),
        );
        assert!(duplicate.publish_url.is_none());
        assert!(!duplicate.record_visit);

        let full_document = decide_document_location(
            true,
            "https://gold-band.dion.blue/zh/documentation",
            Some("https://gold-band.dion.blue/zh/demo"),
            Some("https://gold-band.dion.blue/zh/demo"),
        );
        assert!(full_document.publish_url.is_none());
        assert!(!full_document.record_visit);

        let blank = decide_document_location(
            false,
            "about:blank",
            Some("https://gold-band.dion.blue/zh/demo"),
            Some("https://gold-band.dion.blue/zh/demo"),
        );
        assert!(blank.publish_url.is_none());
        assert!(!blank.record_visit);
    }

    #[test]
    fn webview_script_result_is_the_document_href() {
        assert_eq!(
            super::document_href_from_script_result(
                "\"https://gold-band.dion.blue/zh/documentation\""
            )
            .as_deref(),
            Some("https://gold-band.dion.blue/zh/documentation")
        );
        assert!(super::document_href_from_script_result("null").is_none());
        assert!(super::document_href_from_script_result("").is_none());
    }
}
