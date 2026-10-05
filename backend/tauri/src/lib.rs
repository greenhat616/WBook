use std::sync::{atomic::Ordering, Arc};

use tauri::Manager;
use tauri_plugin_sentry::{minidump, sentry};

use wbook_core::{types::Port, Params};

pub mod bindings;
mod commands;
mod errors;
mod preview;
mod rpc;
mod runtime;
mod subscriptions;

#[cfg(test)]
mod tests;

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

    // Everything after here runs in only the app process
    let app = tauri::Builder::default()
        .manage(_guard)
        .invoke_handler(commands::builder().1.invoke_handler())
        .setup(|app| {
            let port =
                std::env::var("WBOOK_RPC_PORT").map_or(Ok(0), |value| value.parse::<u16>())?;
            let params = Params {
                data_dir: camino::Utf8PathBuf::from_path_buf(app.path().app_data_dir()?)
                    .map_err(|_| std::io::Error::other("App data path must be UTF-8"))?,
                config_dir: camino::Utf8PathBuf::from_path_buf(app.path().app_config_dir()?)
                    .map_err(|_| std::io::Error::other("App config path must be UTF-8"))?,
            };
            let runtime = Arc::new(tauri::async_runtime::block_on(runtime::AppRuntime::start(
                params, port,
            ))?);
            app.manage(runtime.core.clone());
            app.manage(Port(runtime.port));
            app.manage(runtime);
            Ok(())
        })
        .plugin(tauri_plugin_sentry::init(&client))
        .build(tauri::generate_context!())
        .expect("error while running tauri application");
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
            let runtime = app.state::<Arc<runtime::AppRuntime>>().inner().clone();
            if runtime.finished.load(Ordering::Acquire) {
                return;
            }
            api.prevent_exit();
            if !runtime.exiting.swap(true, Ordering::AcqRel) {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    runtime.shutdown().await;
                    app.exit(code.unwrap_or(0));
                });
            }
        }
    });
}
