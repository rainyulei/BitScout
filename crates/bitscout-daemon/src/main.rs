mod server;
mod rg_flags;
mod rg_compat;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let watch = args.iter().position(|a| a == "--watch")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| ".".into());
    let socket = args.iter().position(|a| a == "--socket")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| {
            format!("{}/.bitscout/bitscout.sock", std::env::var("HOME").unwrap())
        });

    eprintln!("BitScout daemon starting...");
    eprintln!("  watch: {}", watch);
    eprintln!("  socket: {}", socket);

    let server = server::DaemonServer::new(socket.into(), watch.into());
    if let Err(e) = server.run().await {
        eprintln!("daemon error: {}", e);
        std::process::exit(1);
    }
}
