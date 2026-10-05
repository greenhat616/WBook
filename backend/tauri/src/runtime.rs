use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use wbook_core::{Params, Wbook};

pub struct AppRuntime {
    pub core: Arc<Wbook>,
    pub port: u16,
    pub exiting: AtomicBool,
    pub finished: AtomicBool,
    server: Mutex<Option<ServerTask>>,
}

struct ServerTask {
    stop: oneshot::Sender<()>,
    task: JoinHandle<io::Result<()>>,
}

impl AppRuntime {
    pub async fn start(params: Params, port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
        let port = listener.local_addr()?.port();
        let core = Arc::new(Wbook::new(params));
        let (stop, stopped) = oneshot::channel();
        let router = crate::rpc::router(core.clone());
        let server = tokio::spawn(server::serve(listener, router, async {
            let _ = stopped.await;
        }));
        tracing::info!("RPC endpoint: http://127.0.0.1:{port}/bridge/rpc");
        Ok(Self {
            core,
            port,
            exiting: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            server: Mutex::new(Some(ServerTask { stop, task: server })),
        })
    }

    pub async fn shutdown(&self) {
        let server = self.server.lock().unwrap().take();
        if let Some(server) = server {
            let _ = server.stop.send(());
            // Cancel core work before waiting for HTTP requests that own receipts.
            for (session, report) in self.core.shutdown().await {
                for warning in &report.cleanup_failures {
                    tracing::warn!(?session, path = ?warning.path, "{}", warning.message);
                }
            }
            match server.task.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::error!("RPC server failed: {error}"),
                Err(error) => tracing::error!("RPC server task failed: {error}"),
            }
            self.finished.store(true, Ordering::Release);
        }
    }
}
