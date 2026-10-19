mod proxy;

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

fn secret() -> String {
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            use tauri::Manager;
            let proxy = proxy::Proxy::new(secret());
            let base = proxy::start(proxy.clone());
            app.manage(ProxyState { proxy, base });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![set_server])
        .run(tauri::generate_context!())
        .expect("error while running Kellyfin");
}
