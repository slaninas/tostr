use futures_util::StreamExt;
use log::{debug, info};

use rand::Rng;

use crate::simpledb;
use crate::utils;
use crate::nostr;
use crate::network;
use crate::twitter;


pub async fn run(
    keypair: secp256k1::KeyPair,
    sink: network::Sink,
    stream: network::StreamType,
    db: simpledb::Database,
    config: utils::Config,
) {
    request_subscription(&keypair, sink.clone()).await;

    start_existing(db.clone(), &config, sink.clone());

    let loooooop = |message: Result<tungstenite::Message, tungstenite::Error>| async {
                let data = match message {
                    Ok(data) => data,
                    Err(error) => {
                        info!("Stream read failed: {}", error);
                        return;
                    }
                };

                let data_str = data.to_string();
                debug!("Got message >{}<", data_str);

                match serde_json::from_str::<nostr::Message>(&data.to_string()) {
                    Ok(message) => {
                        match handle_command(message.content, db.clone(), sink.clone(), &config)
                            .await
                        {
                            Ok(response) => {
                                network::send(response.sign(&keypair).format(), sink.clone()).await
                            }
                            Err(e) => debug!("{}", e),
                        }
                    }
                    Err(e) => {
                        debug!("Unable to parse message: {}", e);
                    }
                }
            };

    match stream {
        network::StreamType::Clearnet(stream) => {
            let f = stream.for_each(loooooop);
            f.await;
        }
        network::StreamType::Tor(stream) => {
            let f = stream.for_each(loooooop);
            f.await;
        }
    }
}

async fn handle_command(
    event: nostr::Event,
    db: simpledb::Database,
    sink: network::Sink,
    config: &utils::Config,
) -> Result<nostr::EventNonSigned, String> {
    let command = &event.content;

    let response = if command.starts_with("add ") {
        Ok(handle_add(db, event, sink, config).await)
    } else if command.starts_with("random") {
        Ok(handle_random(db, event).await)
    } else if command.starts_with("list") {
        Ok(handle_list(db, event).await)
    } else {
        Err(format!("Unknown command >{}<", command))
    };
    response
}

async fn handle_list(db: simpledb::Database, event: nostr::Event) -> nostr::EventNonSigned {
    let follows = db.lock().unwrap().get_follows();
    let mut usernames = follows.keys().collect::<Vec<_>>();
    usernames.sort();

    let mut tags = nostr::get_tags_for_reply(event);
    let orig_tags_count = tags.len();

    let mut text = format!("Hi, I'm following {} accounts:\\n", usernames.len());
    for (index, username) in usernames.iter().enumerate() {
        let secret = follows.get(username.clone()).unwrap();
        tags.push(vec![
            "p".to_string(),
            secret.x_only_public_key().0.to_string(),
        ]);
        text.push_str(&format!("#[{}]\\n", index + orig_tags_count));
    }

    nostr::EventNonSigned {
        created_at: utils::unix_timestamp(),
        kind: 1,
        tags,
        content: text,
    }

}

async fn handle_random(db: simpledb::Database, event: nostr::Event) -> nostr::EventNonSigned {
    let follows = db.lock().unwrap().get_follows();
    let index = rand::thread_rng().gen_range(0..follows.len());

    let random_username = follows.keys().collect::<Vec<_>>()[index];

    let secret = follows.get(random_username).unwrap();

    let mut tags = nostr::get_tags_for_reply(event);
    tags.push(vec![
        "p".to_string(),
        secret.x_only_public_key().0.to_string(),
    ]);
    let mention_index = tags.len() - 1;

    debug!("Command random: returning {}", random_username);
    nostr::EventNonSigned {
        created_at: utils::unix_timestamp(),
        kind: 1,
        tags,
        content: format!("Hi, random account to follow: #[{}]", mention_index),
    }
}

async fn handle_add(
    db: simpledb::Database,
    event: nostr::Event,
    sink: network::Sink,
    config: &utils::Config,
) -> nostr::EventNonSigned {
    let username = event.content[4..event.content.len()]
        .to_ascii_lowercase()
        .replace('@', "");

    if db.clone().lock().unwrap().contains_key(&username) {
        let keypair = simpledb::get_user_keypair(&username, db);
        let (pubkey, _parity) = keypair.x_only_public_key();
        debug!(
            "User {} already added before. Sending existing pubkey {}",
            username, pubkey
        );
        return get_handle_response(event, &pubkey.to_string());
    }

    if db.lock().unwrap().follows_count() + 1 > config.max_follows {
        return nostr::EventNonSigned {
            created_at: utils::unix_timestamp(),
            kind: 1,
            tags: nostr::get_tags_for_reply(event),
            content: format!("Hi, sorry, couldn't add new account. I'm already running at my max capacity ({} users).", config.max_follows),
        };
    }

    if !twitter::user_exists(&username).await {
        return nostr::EventNonSigned {
            created_at: utils::unix_timestamp(),
            kind: 1,
            tags: nostr::get_tags_for_reply(event),
            content: format!("Hi, I wasn't able to find {} on Twitter :(.", username),
        };
    }

    let keypair = utils::get_random_keypair();

    db.lock()
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
        let refresh_interval_secs = config.refresh_interval_secs;
        tokio::spawn(async move {
            update_user(username, &keypair, sink, refresh_interval_secs).await;
        });
    }

    get_handle_response(event, &xonly_pubkey.to_string())
}

fn get_handle_response(event: nostr::Event, new_bot_pubkey: &str) -> nostr::EventNonSigned {
    let mut all_tags = nostr::get_tags_for_reply(event);
    all_tags.push(vec!["p".to_string(), new_bot_pubkey.to_string()]);
    let last_tag_position = all_tags.len() - 1;

    nostr::EventNonSigned {
        created_at: utils::unix_timestamp(),
        kind: 1,
        tags: all_tags,
        content: format!(
            "Hi, tweets will be forwarded to nostr by #[{}].",
            last_tag_position
        ),
    }
}


pub async fn introduction(config: &utils::Config, keypair: &secp256k1::KeyPair, sink: network::Sink) {
    // info!("Main bot is sending set_metadata >{}<
    // Set profile
    info!(
        "main bot is settings name: \"{}\", about: \"{}\", picture_url: \"{}\"",
        config.name, config.about, config.picture_url
    );
    let event = nostr::Event::new(
        keypair,
        utils::unix_timestamp(),
        0,
        vec![],
        format!(
            r#"{{\"name\":\"{}\",\"about\":\"{}\",\"picture\":\"{}\"}}"#,
            config.name, config.about, config.picture_url
        ),
    );

    network::send(event.format(), sink.clone()).await;

    // Say hi
    let welcome = nostr::Event::new(
        keypair,
        utils::unix_timestamp(),
        1,
        vec![],
        config.hello_message.clone(),
    );

    info!("main bot is sending message \"{}\"", config.hello_message);
    network::send(welcome.format(), sink.clone()).await;
}

async fn request_subscription(keypair: &secp256k1::KeyPair, sink: network::Sink) {
    let random_string = rand::thread_rng()
        .sample_iter(rand::distributions::Alphanumeric)
        .take(64)
        .collect::<Vec<_>>();
    let random_string = String::from_utf8(random_string).unwrap();
    // Listen for my pubkey mentions
    network::send(
        format!(
            r##"["REQ", "{}", {{"#p": ["{}"], "since": {}}} ]"##,
            random_string,
            keypair.x_only_public_key().0,
            utils::unix_timestamp(),
        ),
        sink,
    )
    .await;
}

pub fn start_existing(db: simpledb::Database, config: &utils::Config, sink: network::Sink) {
    for (username, keypair) in db.lock().unwrap().get_follows() {
        info!("Starting worker for username {}", username);

        {
            let refresh = config.refresh_interval_secs;
            let sink = sink.clone();
            tokio::spawn(async move {
                update_user(username, &keypair, sink, refresh).await;
            });
        }
    }
}

#[allow(dead_code)]
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

pub async fn update_user(
    username: String,
    keypair: &secp256k1::KeyPair,
    sink: network::Sink,
    refresh_interval_secs: u64,
) {
    // fake_worker(username, refresh_interval_secs).await;
    // return;

    let pic_url = twitter::get_pic_url(&username).await;
    let event = nostr::Event::new(
        keypair,
        utils::unix_timestamp(),
        0,
        vec![],
        format!(
            r#"{{\"name\":\"tostr_{}\",\"about\":\"Tweets forwarded from https://twitter.com/{} by [tostr](https://github.com/slaninas/tostr) bot.\",\"picture\":\"{}\"}}"#,
            username, username, pic_url
        ),
    );

    network::send(event.format(), sink.clone()).await;

    let mut since: chrono::DateTime<chrono::offset::Local> = std::time::SystemTime::now().into();

    loop {
        debug!(
            "Worker for @{} is going to sleep for {} s",
            username, refresh_interval_secs
        );
        tokio::time::sleep(std::time::Duration::from_secs(refresh_interval_secs)).await;

        let until = std::time::SystemTime::now().into();
        let new_tweets = twitter::get_new_tweets(&username, since, until).await;
        // --since seems to be inclusive and --until exclusive so this should be fine
        since = until;

        // twint returns newest tweets first, reverse the Vec here so that tweets are send to relays
        // in order they were published. Still the created_at field can easily be the same so in the
        // end it depends on how the relays handle it
        for tweet in new_tweets.iter().rev() {
            network::send(
                twitter::get_tweet_event(tweet).sign(keypair).format(),
                sink.clone(),
            )
            .await;
        }
        // break;
    }
}
