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

    let listener = TcpListener::bind("0.0.0.0:".to_string() + accord::DEFAULT_PORT)
        .await
        .unwrap();

    let (ctx, crx) = mpsc::channel(32);

    AccordChannel::spawn(crx).await;

    log::info!("Server ready!");
    loop {
        let (socket, addr) = listener.accept().await.unwrap();
        ConnectionWrapper::spawn(socket, addr, ctx.clone()).await;
    }
}
