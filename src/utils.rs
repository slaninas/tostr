use crate::nostr;
use log::{debug, info, warn};

const DATE_FORMAT_STR: &'static str = "%Y-%m-%d %H:%M:%S";

#[derive(Clone)]
pub struct Config {
    pub secret: String,
    pub hello_message: String,
    pub refresh_interval_secs: u64,
    pub relay: String,
    pub max_follows: usize,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmt.debug_struct("Config")
            .field("secret", &"***")
            .field("hello_message", &self.hello_message)
            .field("refresh_interval_secs", &self.refresh_interval_secs)
            .field("relay", &self.relay)
            .field("max_follows", &self.max_follows)
            .finish()
    }
}

pub fn parse_config(path: &std::path::Path) -> Config {
    let get_value = |line: String| {

        let mut value = line.split('=').collect::<Vec<_>>()[1].to_string();
        if value.starts_with('"') && value.ends_with('"'){
            value = value[1..value.len() - 1].to_string();
        }
        value
    };



    let content = std::fs::read_to_string(path).expect("Config reading failed.");

    let mut secret = String::new();
    let mut hello_message = String::new();
    let mut refresh_interval_secs = 0;
    let mut relay = String::new();
    let mut max_follows = 0;

    for line in content.lines() {
        let line = line.to_string();

        if line.starts_with("secret") {
            secret = get_value(line);
        } else if line.starts_with("hello_message") {
            hello_message = get_value(line);
        } else if line.starts_with("refresh_interval_secs") {
            refresh_interval_secs = get_value(line)
                .parse::<u64>()
                .expect("Failed to parse the refresh interval.");
        } else if line.starts_with("relay") {
            relay = get_value(line);
        } else if line.starts_with("max_follows") {
            max_follows = get_value(line).parse::<usize>().expect("Can't parse value");
        } else if line.starts_with("#") {
            // Ignoring comments
        } else {
            warn!("Unknown config line >{}", line);
        }
    }

    assert!(secret.len() > 0);
    assert!(hello_message.len() > 0);
    assert!(refresh_interval_secs > 0);
    assert!(relay.len() > 0);
    assert!(max_follows > 0);

    Config {
        secret,
        hello_message,
        refresh_interval_secs,
        relay,
        max_follows,
    }
}

pub struct Tweet {
    username: String,
    tweet: String,
    link: String,
}

pub fn get_tweet_event(tweet: &Tweet) -> nostr::EventNonSigned {
    let formatted = format!("{} ([source]({}))", tweet.tweet, tweet.link);

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
        "twint -u '{}' --since \"{}\" --csv -o {} 1>/dev/null",
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

                let tweet = line[10].to_string();
                // Filter out replies
                if tweet.starts_with('@') {
                    debug!("Ignoring reply >{}< from {}", tweet, username);
                    continue;
                }
                new_tweets.push(Tweet {
                    // date: format!("{} {} {}", line[3], line[4], line[5]),
                    username: line[7].to_string(),
                    tweet,
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

pub async fn user_exists(username: &String) -> bool {
    let mut since: chrono::DateTime<chrono::offset::Local> = std::time::SystemTime::now().into();

    let cmd = format!(
        "twint -u '{}' --since \"{}\" 1>/dev/null",
        username,
        since.format(DATE_FORMAT_STR),
    );
    debug!("Running >{}<", cmd);
    let status = async_process::Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .status()
        .await
        .unwrap();

    status.success()
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
