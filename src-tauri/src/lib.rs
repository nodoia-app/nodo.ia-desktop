use tauri::{WebviewUrl, WebviewWindowBuilder};

/// Where the shell loads Nodo.
///
/// Priority:
/// 1. `NODO_APP_URL` env (staging deploy, custom host, etc.)
/// 2. Debug / `tauri dev` → local Next (`localhost:3000`) — uses your `.env.local` / staging backend
/// 3. Release installer → production
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
        "https://www.nodoia.app/agent".into()
    }
}

fn fallback_url() -> &'static str {
    if cfg!(debug_assertions) {
        "http://localhost:3000/agent"
    } else {
        "https://www.nodoia.app/agent"
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let url = app_url();
            let parsed = url
                .parse()
                .unwrap_or_else(|_| fallback_url().parse().unwrap());

            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(parsed))
                .title("Nodo")
                .inner_size(1280.0, 840.0)
                .min_inner_size(900.0, 600.0)
                .resizable(true)
                .build()?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Nodo");
}
