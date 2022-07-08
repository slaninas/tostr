use futures_util::sink::SinkExt;
use futures_util::StreamExt;
use log::{debug, info};
use std::io::Write;

pub mod nostr;
pub mod websocket;

const DATE_FORMAT_STR: &'static str = "%Y-%m-%d %H:%M:%S";

type Database = std::sync::Arc<std::sync::Mutex<crate::SimpleDatabase>>;
type WebSocketStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type SplitSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    tungstenite::Message,
>;
type Stream = futures_util::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;

type WrappedSink = std::sync::Arc<std::sync::Mutex<SplitSink>>;

#[derive(Clone)]
pub struct Sink {
    pub sink: WrappedSink,
    pub peer_addr: String,
}

pub async fn run(
    keypair: secp256k1::KeyPair,
    sink: Sink,
    stream: Stream,
    db: Database,
    config: Config,
) {
    let welcome = crate::nostr::Event::new(
        &keypair,
        "Hi, I'm tostr, reply with command 'add @twitter_account'".to_string(),
        unix_timestamp(),
        vec![],
    );

    send(welcome.format(), sink.clone()).await;

    // Listen for my pubkey mentions
    send(
        format!(
            r##"["REQ", "{}", {{"#p": ["{}"], "since": {}}} ]"##,
            "dsfasdfdafadf",
            keypair.x_only_public_key().0,
            unix_timestamp(),
        ),
        sink.clone(),
    )
    .await;

    start_existing(db.clone(), &config, sink.clone());

    let f = stream.for_each(|message| async {
        let data = message.unwrap();

        let data_str = data.to_string();
        debug!("Got message >{}<", data_str);

        match serde_json::from_str::<crate::nostr::Message>(&data.to_string()) {
            Ok(message) => {
                match handle_command(
                    message.content,
                    db.clone(),
                    sink.clone(),
                    config.refresh_interval_secs,
                )
                .await
                {
                    Ok(response) => send(response.sign(&keypair).format(), sink.clone()).await,
                    Err(e) => debug!("{}", e),
                }
            }
            Err(e) => {
                debug!("Unable to parse message: {}", e);
            }
        }
    });

    f.await;
}

struct AddHandler {
    main_bot_pubkey: String,
}

async fn handle_command(
    event: nostr::Event,
    db: Database,
    sink: Sink,
    refresh_interval_secs: u64,
) -> Result<nostr::EventNonSigned, String> {
    let response = match event.content.get(..5) {
        Some("add @") => Ok(handle_add(db, event, sink, refresh_interval_secs).await),
        _ => Err(format!("Unknown command >{}<", event.content)),
    };
    response
}

async fn handle_add(
    db: Database,
    event: nostr::Event,
    sink: Sink,
    refresh_interval_secs: u64,
) -> nostr::EventNonSigned {
    let username = event.content[5..event.content.len()].to_string();

    if db.clone().lock().unwrap().contains_key(&username) {
        let keypair = get_user_keypair(&username, db);
        let (pubkey, _parity) = keypair.x_only_public_key();
        debug!(
            "User @{} already added before. Sending existing pubkey {}",
            username, pubkey
        );
        return get_handle_response(event, &pubkey.to_string(), sink.clone());
    }
    let keypair = get_random_keypair();

    db.clone()
        .lock()
        .unwrap()
        .insert(username.clone(), keypair.display_secret().to_string())
        .unwrap();
    let (xonly_pubkey, _) = keypair.x_only_public_key();
    let username = username.to_string();
    info!(
        "Starting worker for username {}, pubkey {}",
        username, xonly_pubkey
    );

    {
        let sink = sink.clone();
        tokio::spawn(async move {
            crate::update_user(username, &keypair, sink, refresh_interval_secs).await;
        });
    }

    get_handle_response(event, &xonly_pubkey.to_string(), sink)
}

fn get_handle_response(
    event: crate::nostr::Event,
    new_bot_pubkey: &str,
    sink: Sink,
) -> crate::nostr::EventNonSigned {
    let mut all_tags = crate::nostr::get_tags_for_reply(event);
    all_tags.push(vec!["p".to_string(), new_bot_pubkey.to_string()]);
    let last_tag_position = all_tags.len() - 1;

    crate::nostr::EventNonSigned {
        created_at: unix_timestamp(),
        kind: 1,
        tags: all_tags,
        content: format!("Hi, pubkey is #[{}]", last_tag_position),
    }
}

async fn send(msg: String, sink: Sink) {
    debug!("Sending >{}< to {}", msg, sink.peer_addr);
    sink.sink
        .lock()
        .unwrap()
        .send(tungstenite::Message::Text(msg))
        .await
        .unwrap();
}

pub fn start_existing(
    db: std::sync::Arc<std::sync::Mutex<crate::SimpleDatabase>>,
    config: &crate::Config,
    sink: Sink,
) {
    for (username, keypair) in db.lock().unwrap().get_follows() {
        info!("Starting worker for username {}", username);

        {
            let refresh = config.refresh_interval_secs.clone();
            let sink = sink.clone();
            tokio::spawn(async move {
                crate::update_user(username, &keypair, sink, refresh).await;
            });
        }
    }
}

async fn fake_worker(username: String, refresh_interval_secs: u64) {
    loop {
        debug!(
            "Fake worker for user {}  is going to sleep for {} s",
            username, refresh_interval_secs
        );
        tokio::time::sleep(std::time::Duration::from_secs(refresh_interval_secs)).await;
        debug!("Faking the work for user {}", username);
    }
}

pub struct Config {
    pub secret: String,
    pub refresh_interval_secs: u64,
    pub relays: Vec<String>,
}

pub fn parse_config(path: &std::path::Path) -> Config {
    let get_value = |line: String| line.split('=').collect::<Vec<_>>()[1].to_string();

    let content = std::fs::read_to_string(path).expect("Config reading failed.");

    let mut secret = String::new();
    let mut refresh_interval_secs = 0;
    let mut relays = vec![];

    for line in content.lines() {
        let line = line.to_string();

        if line.starts_with("secret") {
            secret = get_value(line);
        } else if line.starts_with("refresh_interval_secs") {
            refresh_interval_secs = get_value(line)
                .parse::<u64>()
                .expect("Failed to parse the refresh interval.");
        } else if line.starts_with("add_relay") {
            relays.push(get_value(line))
        }
    }

    assert!(secret.len() > 0);
    assert!(refresh_interval_secs > 0);
    assert!(relays.len() > 0);

    Config {
        secret,
        refresh_interval_secs,
        relays,
    }
}

pub async fn update_user(
    username: String,
    keypair: &secp256k1::KeyPair,
    sink: Sink,
    refresh_interval_secs: u64,
) {
    let mut since: chrono::DateTime<chrono::offset::Local> = std::time::SystemTime::now().into();
    fake_worker(username, refresh_interval_secs).await;
    return;
    loop {
        debug!(
            "Worker for @{} is going to sleep for {} s",
            username, refresh_interval_secs
        );
        tokio::time::sleep(std::time::Duration::from_secs(refresh_interval_secs)).await;

        let new_tweets = get_new_tweets(&username, since).await;
        since = std::time::SystemTime::now().into();

        // twint returns newest tweets first, reverse the Vec here so that tweets are send to relays
        // in order they were published. Still the created_at field can easily be the same so in the
        // end it depends on how the relays handle it
        for tweet in new_tweets.iter().rev() {
            send(get_tweet_event(tweet).sign(&keypair).format(), sink);
        }
        // break;
    }
}

#[derive(Debug)]
struct Tweet {
    // date: String,
    username: String,
    tweet: String,
    link: String,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmt.debug_struct("Config")
            .field("secret", &"***")
            .field("refresh_interval_secs", &self.refresh_interval_secs)
            .field("relays", &self.relays)
            .finish()
    }
}

fn get_tweet_event(tweet: &Tweet) -> nostr::EventNonSigned {
    let formatted = format!(
        "[@{}@twitter.com]({}): {}",
        tweet.username, tweet.link, tweet.tweet
    );

    nostr::EventNonSigned {
        created_at: unix_timestamp(),
        kind: 1,
        tags: vec![],
        content: formatted,
    }
}

async fn get_new_tweets(
    username: &String,
    since: chrono::DateTime<chrono::offset::Local>,
) -> Vec<Tweet> {
    debug!("Checking new tweets from {}", username);
    let workfile = format!("{}_workfile.csv", username);

    // let since = "2022-07-03 20:39:17";
    let cmd = format!(
        "twint -u '{}' --since \"{}\" --csv -o {}",
        username,
        since.format(DATE_FORMAT_STR),
        // since,
        workfile
    );
    debug!("Running >{}<", cmd);
    // TODO: Handle status
    let _output = async_process::Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .status()
        .await
        .unwrap();

    let mut new_tweets = vec![];
    match std::fs::read_to_string(workfile.clone()) {
        Ok(content) => {
            std::fs::remove_file(workfile).unwrap();

            let csv = content.lines().collect::<Vec<_>>();

            for i in 1..csv.len() {
                let line = csv[i].split("\t").collect::<Vec<_>>();
                new_tweets.push(Tweet {
                    // date: format!("{} {} {}", line[3], line[4], line[5]),
                    username: line[7].to_string(),
                    tweet: line[10].to_string(),
                    link: line[20].to_string(),
                });
            }

            info!("Found {} new tweets from {}", new_tweets.len(), username);
        }
        Err(_) => {
            info!("No new tweets from {} found", username);
        }
    }

    new_tweets
}

pub struct SimpleDatabase {
    follows: std::collections::HashMap<String, String>,
    file: String,
}

impl SimpleDatabase {
    pub fn from_file(path: String) -> SimpleDatabase {
        let mut db = SimpleDatabase {
            follows: std::collections::HashMap::new(),
            file: path.clone(),
        };

        let content = std::fs::read_to_string(path).expect("Failed opening database file");

        for line in content.lines() {
            let split = line.split(":").collect::<Vec<_>>();
            if split.len() != 2 {
                debug!("unable to parse line: >{:?}<, skipping", split);
                continue;
            }
            let username = split[0];
            let seckey = split[1];

            match db.follows.insert(username.to_string(), seckey.to_string()) {
                Some(_) => panic!(
                    "Inconsistent database, username {} is more than once in the database",
                    username
                ),
                None => {
                    debug!(
                        "Read from file: inserting username {} into database",
                        username
                    );
                }
            }
        }

        db
    }

    pub fn insert(&mut self, username: String, seckey: String) -> Result<(), String> {
        if self.follows.contains_key(&username) {
            return Err("Key already in the database".to_string());
        }

        self.follows.insert(username.clone(), seckey.clone());
        debug!("Added {} to the database", username);

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .append(true)
            .open(self.file.clone())
            .unwrap();

        write!(file, "{}:{}\n", username, seckey).unwrap();
        debug!("Wrote updated database to the file");
        Ok(())
    }

    pub fn get(&self, key: &str) -> String {
        self.follows.get(key).unwrap().to_string()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.follows.contains_key(key)
    }

    pub fn get_follows(&self) -> std::collections::HashMap<String, secp256k1::KeyPair> {
        let mut result = std::collections::HashMap::<String, secp256k1::KeyPair>::new();
        let secp = secp256k1::Secp256k1::new();
        for (username, secret) in &self.follows {
            result
                .insert(
                    username.clone(),
                    secp256k1::KeyPair::from_seckey_str(&secp, &secret).unwrap(),
                )
                .unwrap();
        }
        result
    }
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn get_random_keypair() -> secp256k1::KeyPair {
    let secp = secp256k1::Secp256k1::new();
    let secret = secp256k1::SecretKey::new(&mut rand::thread_rng());
    secret.keypair(&secp)
}

fn get_user_keypair(username: &String, db: Database) -> secp256k1::KeyPair {
    let secp = secp256k1::Secp256k1::new();
    let existing_secret = db.lock().unwrap().get(username);
    secp256k1::KeyPair::from_seckey_str(&secp, &existing_secret).unwrap()
}
