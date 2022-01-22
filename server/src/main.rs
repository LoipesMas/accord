use tokio::net::TcpListener;

use tokio::sync::mpsc;

use accord_server::channel::AccordChannel;
use accord_server::connection::ConnectionWrapper;

//TODO: use logging crate?
//TODO: encryption?

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("0.0.0.0:".to_string() + accord::DEFAULT_PORT)
        .await
        .unwrap();

    let (ctx, crx) = mpsc::channel(32);

    AccordChannel::spawn(crx);

    println!("Server ready!");
    loop {
        let (socket, addr) = listener.accept().await.unwrap();
        ConnectionWrapper::spawn(socket, addr, ctx.clone()).await;
    }
}
