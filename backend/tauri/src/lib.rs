use tauri_plugin_sentry::{minidump, sentry};

use wbook_core::types::Port;

mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let client = sentry::init((
        "https://7ff49cd7135abc2325007303e0a39a37@o4505576204992512.ingest.sentry.io/4506191549300736",
        sentry::ClientOptions {
            release: sentry::release_name!(),
            ..Default::default()
        },
    ));

    // Everything before here runs in both app and crash reporter processes
    let _guard = minidump::init(&client);

    let port = portpicker::pick_unused_port().expect("failed to find unused port");
    tauri::async_runtime::spawn(server::start(port));
    // Everything after here runs in only the app process
    tauri::Builder::default()
        .manage(_guard)
        .manage(Port(port))
        .invoke_handler(tauri::generate_handler![commands::get_port])
        .plugin(tauri_plugin_sentry::init(&client))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
