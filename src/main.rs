mod config;
mod error;
mod geo;
mod http_utils;
mod proxy;
mod routes;
mod server;
mod stats;
mod sync;
mod validation;

use std::io::Write;
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn logs_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("logs")
}

fn main() {
    let log_dir = logs_dir();
    let _ = std::fs::create_dir_all(&log_dir);
    let crash_log = log_dir.join("crash.log");

    // Write panics to crash.log (append mode) so errors are never a silent flash on Windows
    let hook = std::panic::take_hook();
    let crash_log_path = crash_log.clone();
    std::panic::set_hook(Box::new(move |info| {
        hook(info);
        let msg = format!(
            "{:?}\n",
            info.payload()
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.as_str()))
                .unwrap_or("unknown panic")
        );
        // Append to crash.log instead of overwriting; ignore errors (best-effort)
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&crash_log_path)
        {
            let _ = f.write_all(msg.as_bytes());
        }
    }));

    let file_appender = tracing_appender::rolling::daily(&log_dir, "mirror.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_target(false).without_time().with_ansi(false).compact())
        .with(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(non_blocking))
        .init();

    let _guard = guard;

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        if let Err(e) = server::run().await {
            tracing::error!("fatal: {:#}", e);
            std::process::exit(1);
        }
    });
}
