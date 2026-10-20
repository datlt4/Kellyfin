mod proxy;
mod update;

use rand::RngCore;
use tauri::State;

struct ProxyState {
    proxy: proxy::Proxy,
    /// Base URL and serving task; replaced when the listener has died.
    live: tokio::sync::Mutex<(String, tauri::async_runtime::JoinHandle<()>)>,
}

/// Points the local proxy at a server; returns the base URL the web UI should use instead.
/// A new base means the old listener had died (iOS, after a suspension).
#[tauri::command]
async fn set_server(state: State<'_, ProxyState>, url: String, headers: Vec<(String, String)>) -> Result<String, String> {
    *state.proxy.target.write().unwrap() = Some(proxy::Target { url, headers });
    let mut live = state.live.lock().await;
    if !proxy::alive(&live.0).await {
        live.1.abort();
        *live = proxy::start(state.proxy.clone()).map_err(|e| e.to_string())?;
    }
    Ok(live.0.clone())
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
            let live = proxy::start(proxy.clone())?;
            app.manage(ProxyState { proxy, live: tokio::sync::Mutex::new(live) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![set_server, update_check, update_install, open_release])
        .run(tauri::generate_context!())
        .expect("error while running Kellyfin");
}
