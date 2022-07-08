use log::{debug, info};
use std::io::Write;

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
    let config = tostr::parse_config(&config_path);
    debug!("{:?}", config);

    info!("Starting bot");
    let db = tostr::SimpleDatabase::from_file("blah".to_string());
    let db = std::sync::Arc::new(std::sync::Mutex::new(db));
    tostr::start_existing(db.clone(), &config, config.relays[0].clone());

    let relay = &config.relays[0];
    info!("Connecting to {}", relay);
    // TODO: Enable Tls
    let (ws_stream, _response) = tokio_tungstenite::connect_async(url::Url::parse(relay).unwrap())
        .await
        .expect("Can't connect");

    let mut bot = tostr::Bot::new(db, config, ws_stream);
    bot.run().await;
}
