use log::{info, debug};
use crate::nostr;

const DATE_FORMAT_STR: &'static str = "%Y-%m-%d %H:%M:%S";

pub struct Config {
    pub secret: String,
    pub refresh_interval_secs: u64,
    pub relays: Vec<String>,
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


pub struct Tweet {
    username: String,
    tweet: String,
    link: String,
}


pub fn get_tweet_event(tweet: &Tweet) -> nostr::EventNonSigned {
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


pub async fn get_new_tweets(
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

pub fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub fn get_random_keypair() -> secp256k1::KeyPair {
    let secp = secp256k1::Secp256k1::new();
    let secret = secp256k1::SecretKey::new(&mut rand::thread_rng());
    secret.keypair(&secp)
}
