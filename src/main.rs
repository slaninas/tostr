use futures_util::StreamExt;
use log::{debug, info};
use std::io::Write;

type WebSocket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

#[tokio::main]
async fn main() {
    let start = std::time::Instant::now();
    env_logger::Builder::from_default_env()
        .format(move |buf, rec| {
            let t = start.elapsed().as_secs_f32();
            writeln!(buf, "{:.03} [{}] - {}", t, rec.level(), rec.args())
        })
        .init();

    let config_path = std::path::PathBuf::from("config");
    let config = tostr::utils::parse_config(&config_path);
    debug!("{:?}", config);

    info!("Starting bot");
    // TODO: Use tokio Mutex?
    let db = tostr::simpledb::SimpleDatabase::from_file("blah".to_string());
    let db = std::sync::Arc::new(std::sync::Mutex::new(db));

    let relay = &config.relays[0];

    // TODO: Don't send Hi message in a loop
    // Also set profiles only once when new users are created
    loop {
        let ws_stream = connect(relay).await;
        let (sink, stream) = ws_stream.split();

        let secp = secp256k1::Secp256k1::new();
        let keypair = secp256k1::KeyPair::from_seckey_str(&secp, &config.secret).unwrap();

        tostr::run(
            keypair,
            tostr::Sink {
                sink: std::sync::Arc::new(tokio::sync::Mutex::new(sink)),
                peer_addr: relay.clone(),
            },
            stream,
            db.clone(),
            config.clone(),
        )
        .await;

        let wait_secs = 20;
        info!("Connection lost. Will try to reconnect in {} seconds", wait_secs);
        tokio::time::sleep(std::time::Duration::from_secs(wait_secs)).await;
    }
}

async fn connect(relay: &String) -> WebSocket {
    info!("Connecting to {}", relay);
    // TODO: Enable Tls
    let (ws_stream, _response) = tokio_tungstenite::connect_async(url::Url::parse(relay).unwrap())
        .await
        .expect("Can't connect");
    ws_stream
}
