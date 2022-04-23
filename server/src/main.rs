use tokio::net::TcpListener;

use tokio::sync::mpsc;

use accord_server::channel::AccordChannel;
use accord_server::connection::ConnectionWrapper;

use flexi_logger::Logger;
//TODO: pad message for security/privacy (so length isn't obvious)?

fn init_logger() {
    Logger::try_with_env_or_str("info")
        .unwrap()
        .start()
        .unwrap();
}

#[tokio::main]
async fn main() {
    init_logger();

    let config = accord_server::config::load_config();

    let port = config.port.unwrap_or(accord::DEFAULT_PORT);
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();

    log::info!("Listening on port {}.", port);
    let (ctx, crx) = mpsc::channel(32);

    AccordChannel::spawn(crx).await;

    log::info!("Server ready!");
    loop {
        let (socket, addr) = listener.accept().await.unwrap();
        ConnectionWrapper::spawn(socket, addr, ctx.clone()).await;
    }
}
