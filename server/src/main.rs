use tokio::net::TcpListener;

use tokio::sync::mpsc;

use accord_server::channel::AccordChannel;
use accord_server::connection::ConnectionWrapper;

use clap::Parser;

use flexi_logger::{writers::LogWriter, FileSpec, Logger};
//TODO: pad message for security/privacy (so length isn't obvious)?

mod logging;
mod tui;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Disable TUI (just output to stdout, no commandline)
    #[clap(short, long)]
    no_tui: bool,

    /// Log to file as well
    #[clap(short, long)]
    log_to_file: bool,
}

fn init_logger_tui(writer: Box<dyn LogWriter>, log_to_file: bool) {
    let logger = Logger::try_with_env_or_str("info").unwrap();

    let logger = if log_to_file {
        logger.log_to_file_and_writer(FileSpec::default(), writer)
    } else {
        logger.log_to_writer(writer)
    };
    if let Err(e) = logger.start() {
        eprintln!("Error while setting up logger: {}", e);
    }
}

fn init_logger_stdout(log_to_file: bool) {
    let logger = Logger::try_with_env_or_str("info").unwrap();

    let logger = if log_to_file {
        logger
            .log_to_file(FileSpec::default())
            .duplicate_to_stdout(flexi_logger::Duplicate::All)
    } else {
        logger
    };
    if let Err(e) = logger.start() {
        eprintln!("Error while setting up logger: {}", e);
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let (ctx, crx) = mpsc::channel(32);
    let tui = !args.no_tui;
    let mut tui_handle = None;
    if tui {
        let (logs_tx, logs_rx) = mpsc::channel(128);
        let writer = logging::LogVec::new(logs_tx);
        init_logger_tui(Box::new(writer), args.log_to_file);
        tui_handle = Some(tui::Tui::new(logs_rx, ctx.clone()).launch());
    } else {
        init_logger_stdout(args.log_to_file);
    }

    let config = accord_server::config::load_config();

    let port = config.port.unwrap_or(accord::DEFAULT_PORT);
    let listener = match TcpListener::bind(format!("0.0.0.0:{}", port)).await {
        Ok(listener) => listener,
        Err(e) => {
            log::error!("Failed to bind to port {}. Error: {}", port, e);
            if let Some(tui_handle) = tui_handle {
                log::info!("Enter `exit` command to exit.");
                if let Err(e) = tui_handle.await {
                    eprintln!("Error while joining tui_handle: {}", e);
                }
                return;
            }
            return;
        }
    };

    log::info!("Listening on port {}.", port);

    let result = AccordChannel::spawn(crx, config).await;
    match result {
        Err(e) => {
            log::error!("Failed to start server. Error: {}", e);
            if let Some(tui_handle) = tui_handle {
                log::info!("Enter `exit` command to exit.");
                if let Err(e) = tui_handle.await {
                    eprintln!("Error while joining tui_handle: {}", e);
                }
            }
        }
        Ok(_) => {
            log::info!("Server ready!");
            if let Some(mut tui_handle2) = tui_handle {
                loop {
                    tokio::select! {
                        res = listener.accept() => {
                            let (socket, addr) = res.unwrap();
                            ConnectionWrapper::spawn(socket, addr, ctx.clone()).await;
                        },
                        _ = &mut tui_handle2 => {
                            break;
                        }
                    }
                }
            } else {
                #[cfg(unix)]
                tokio::spawn(async move {
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap().recv().await;
                    std::process::exit(0);
                });

                loop {
                    let (socket, addr) = listener.accept().await.unwrap();
                    ConnectionWrapper::spawn(socket, addr, ctx.clone()).await;
                }
            };
        }
    }
}
