mod proxy;
mod update;

use rand::RngCore;
use tauri::State;

struct ProxyState {
    proxy: proxy::Proxy,
    base: String,
}

/// Points the local proxy at a server; returns the base URL the web UI should use instead.
#[tauri::command]
fn set_server(state: State<ProxyState>, url: String, headers: Vec<(String, String)>) -> String {
    *state.proxy.target.write().unwrap() = Some(proxy::Target { url, headers });
    state.base.clone()
}

/// Screen brightness 0..1 for the player's swipe (iOS; Android uses its JavaScript bridge).
#[tauri::command]
async fn get_brightness(app: tauri::AppHandle) -> f64 {
    #[cfg(target_os = "ios")]
    {
        // UIKit belongs to the main thread; async commands run elsewhere, so hop over and wait.
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = app.run_on_main_thread(move || {
            let _ = tx.send(ios::brightness());
        });
        return rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap_or(1.0);
    }
    #[cfg(not(target_os = "ios"))]
    {
        let _ = app;
        1.0
    }
}

#[tauri::command]
async fn set_brightness(app: tauri::AppHandle, value: f64) {
    #[cfg(target_os = "ios")]
    let _ = app.run_on_main_thread(move || ios::set_brightness(value.clamp(0.0, 1.0)));
    #[cfg(not(target_os = "ios"))]
    let _ = (app, value);
}

#[cfg(target_os = "ios")]
mod ios {
    use objc2::{class, msg_send, runtime::AnyObject};

    pub fn brightness() -> f64 {
        unsafe {
            let screen: *mut AnyObject = msg_send![class!(UIScreen), mainScreen];
            msg_send![screen, brightness]
        }
    }

    pub fn set_brightness(v: f64) {
        unsafe {
            let screen: *mut AnyObject = msg_send![class!(UIScreen), mainScreen];
            let _: () = msg_send![screen, setBrightness: v];
        }
    }
}

/// Newest release if it is newer than this app (None when up to date).
#[tauri::command]
async fn update_check(app: tauri::AppHandle) -> Result<Option<update::UpdateInfo>, String> {
    update::check(&app).await
}

/// Desktop: download, verify, install and restart; progress is (bytes so far, total).
#[tauri::command]
async fn update_install(app: tauri::AppHandle, on_progress: tauri::ipc::Channel<(u64, u64)>) -> Result<(), String> {
    update::install(&app, on_progress).await
}

/// Opens a Kellyfin release link in the system browser (Android downloads the APK there).
#[tauri::command]
fn open_release(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    if !url.starts_with("https://github.com/datlt4/Kellyfin/") {
        return Err("Only Kellyfin release links can be opened".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

fn secret() -> String {
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            #[cfg(desktop)]
            app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
            use tauri::Manager;
            let proxy = proxy::Proxy::new(secret());
            let base = proxy::start(proxy.clone());
            app.manage(ProxyState { proxy, base });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![set_server, get_brightness, set_brightness, update_check, update_install, open_release])
        .run(tauri::generate_context!())
        .expect("error while running Kellyfin");
}
