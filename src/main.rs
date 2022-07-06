use log::{debug, info};
use std::io::Write;

const DATE_FORMAT_STR: &'static str = "%Y-%m-%d %H:%M:%S";

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

    let time: chrono::DateTime<chrono::offset::Local> = std::time::SystemTime::now().into();

    let mut last_update =
        std::collections::HashMap::<String, chrono::DateTime<chrono::offset::Local>>::new();

    let mut handles = vec![];

    for username in config.follow {
        // last_update.insert(username.to_string(), time);
        let secret = config.secret.clone();
        let relays = config.relays.clone();
        debug!("Spawning update user task for {}", username);
        handles.push(tokio::spawn(async move {
            tostr::update_user(username, secret, relays, config.refresh_interval_secs).await;
        }));
    }

    for handle in handles {
        tokio::join!(handle);
    }
}
