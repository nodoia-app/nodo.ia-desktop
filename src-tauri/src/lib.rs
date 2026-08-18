mod lan_printer;

use tauri::webview::NewWindowResponse;
use tauri::{WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
async fn scan_lan_printers() -> Result<lan_printer::ScanReport, String> {
    tauri::async_runtime::spawn_blocking(lan_printer::scan_lan_printers_sync)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn confirm_lan_printer(address: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || lan_printer::confirm_lan_printer_sync(address))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn print_lan(address: String, data: Vec<u8>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || lan_printer::print_lan_sync(address, data))
        .await
        .map_err(|e| e.to_string())?
}

/// Where the shell loads Nodo.
///
/// Priority:
/// 1. `NODO_APP_URL` env (staging deploy, custom host, etc.)
/// 2. Debug / `tauri dev` → local Next (`localhost:3000`) — uses your `.env.local` / staging backend
/// 3. Release installer → production product host
fn app_url() -> String {
    if let Ok(url) = std::env::var("NODO_APP_URL") {
        let trimmed = url.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }

    if cfg!(debug_assertions) {
        "http://localhost:3000/agent".into()
    } else {
        "https://login.nodoia.app".into()
    }
}

fn fallback_url() -> &'static str {
    if cfg!(debug_assertions) {
        "http://localhost:3000/agent"
    } else {
        "https://login.nodoia.app"
    }
}

fn is_private_lan(host: &str) -> bool {
    if host == "localhost" || host == "127.0.0.1" {
        return true;
    }
    if let Some(rest) = host.strip_prefix("10.") {
        return rest.split('.').count() == 3;
    }
    if let Some(rest) = host.strip_prefix("192.168.") {
        return rest.split('.').count() == 2;
    }
    if let Some(rest) = host.strip_prefix("172.") {
        let mut parts = rest.split('.');
        let second = parts.next().and_then(|p| p.parse::<u8>().ok());
        return matches!(second, Some(16..=31)) && parts.count() == 2;
    }
    false
}

fn is_oauth_host(host: &str) -> bool {
    host == "appleid.apple.com"
        || host == "account.apple.com"
        || host.ends_with(".apple.com")
        || host == "accounts.google.com"
        || host.ends_with(".google.com")
}

fn configured_app_host() -> Option<String> {
    std::env::var("NODO_APP_URL")
        .ok()
        .and_then(|raw| raw.parse::<url::Url>().ok())
        .and_then(|parsed| parsed.host_str().map(str::to_string))
}

/// Stay inside the Tauri window only for the product host, local dev, and
/// Apple/Google sign-in. Everything else (docs, landing, social) opens in
/// the system browser.
fn is_webview_url(url: &url::Url) -> bool {
    match url.scheme() {
        "tauri" | "data" | "about" | "blob" => return true,
        "http" | "https" => {}
        _ => return false,
    }

    let host = url.host_str().unwrap_or("");
    if host == "login.nodoia.app" || is_private_lan(host) || is_oauth_host(host) {
        return true;
    }
    configured_app_host().is_some_and(|allowed| allowed == host)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            scan_lan_printers,
            confirm_lan_printer,
            print_lan
        ])
        .setup(|app| {
            let url = app_url();
            let parsed = url
                .parse()
                .unwrap_or_else(|_| fallback_url().parse().unwrap());
            let handle = app.handle().clone();
            let opener_handle = handle.clone();

            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(parsed))
                .title("Nodo")
                .inner_size(1280.0, 840.0)
                .min_inner_size(900.0, 600.0)
                .resizable(true)
                .on_navigation({
                    let handle = handle.clone();
                    move |url| {
                        if is_webview_url(&url) {
                            return true;
                        }
                        let _ = handle.opener().open_url(url.as_str(), None::<&str>);
                        false
                    }
                })
                .on_new_window(move |url, _features| {
                    if is_webview_url(&url) {
                        return NewWindowResponse::Allow;
                    }
                    let _ = opener_handle.opener().open_url(url.as_str(), None::<&str>);
                    NewWindowResponse::Deny
                })
                .build()?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Nodo");
}
