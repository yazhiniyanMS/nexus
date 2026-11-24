//! Desktop notifications, used only to say that something is waiting for the user while the
//! window is in the background. Failures are ignored: a notification is never the only signal.

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_notification::NotificationExt;

/// Show a notification unless the NEXUS window already has the user's attention.
///
/// Never call this while holding a lock the main thread may want (the database above all):
/// checking focus waits for the main thread, so that would deadlock the app.
pub fn attention<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    // Without the plugin (unit tests on a mock app) there is nothing to show a notification with.
    if app.try_state::<tauri_plugin_notification::Notification<R>>().is_none() {
        return;
    }
    let focused = app.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
    if focused {
        return;
    }
    let _ = app.notification().builder().title(title).body(body).show();
}
