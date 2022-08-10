use log::{debug, info};

use crate::utils;

const DATE_FORMAT_STR: &str = "%Y-%m-%d %H:%M:%S";

pub struct Tweet {
    timestamp: u64,
    tweet: String,
    link: String,
}

pub fn get_tweet_event(tweet: &Tweet) -> nostr_bot::EventNonSigned {
    let formatted = format!("{} ([source]({}))", tweet.tweet, tweet.link);

    nostr_bot::EventNonSigned {
        created_at: utils::unix_timestamp(),
        kind: 1,
        tags: vec![vec![
            "tweet_timestamp".to_string(),
            format!("{}", tweet.timestamp),
        ]],
        content: formatted,
    }
}

pub async fn user_exists(username: &String) -> bool {
    let since: chrono::DateTime<chrono::offset::Local> = std::time::SystemTime::now().into();

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

pub async fn get_pic_url(username: &String) -> String {
    let pic_cmd = format!(
        r#"twint --user-full -u '{}' 2>&1 | sed 's/.*Avatar: \(https.*\)/\1/' | tr -d \\n"#,
        username
    );
    debug!("Runnings bash -c '{}", pic_cmd);

    let stdout = async_process::Command::new("bash")
        .arg("-c")
        .arg(pic_cmd)
        .output()
        .await
        .expect("twint command failed")
        .stdout;

    let pic_url = String::from_utf8(stdout).unwrap();

    if pic_url.starts_with("http") {
        debug!("Found pic url {} for {}", pic_url, username);
        pic_url
    } else {
        info!("Unable to find picture for {}", username);
        "".to_string()
    }
}

pub async fn get_new_tweets(
    username: &String,
    since: chrono::DateTime<chrono::offset::Local>,
    until: chrono::DateTime<chrono::offset::Local>,
) -> Result<Vec<Tweet>, String> {
    debug!("Checking new tweets from {}", username);
    let workfile = format!("{}_workfile.csv", username);
    let twint_date_format = "%Y-%m-%d %T %z";

    let cmd = format!(
        "twint -u '{}' --since \"{}\" --until \"{}\" --csv -o {} 1>/dev/null",
        username,
        since.format(DATE_FORMAT_STR),
        until.format(DATE_FORMAT_STR),
        workfile
    );
    debug!("Running >{}<", cmd);
    // TODO: Handle status
    let output = async_process::Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .status()
        .await
        .unwrap();

    if !output.success() {
        return Err(format!("Unable to check for new tweets from {}", username));
    }

    let mut new_tweets = vec![];
    match std::fs::read_to_string(workfile.clone()) {
        Ok(content) => {
            std::fs::remove_file(workfile).unwrap();

            let csv = content.lines().collect::<Vec<_>>();

            for item in csv.iter().skip(1) {
                let line = item.split('\t').collect::<Vec<_>>();

                let tweet = line[10].to_string();
                // Filter out replies
                if tweet.starts_with('@') {
                    debug!("Ignoring reply >{}< from {}", tweet, username);
                    continue;
                }

                let timestamp = chrono::DateTime::parse_from_str(
                    &format!("{} {} {}", line[3], line[4], line[5]),
                    twint_date_format,
                )
                .unwrap()
                .timestamp() as u64;
                new_tweets.push(Tweet {
                    timestamp,
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

    // Follow links to the final destinations
    follow_links(&mut new_tweets).await;

    Ok(new_tweets)
}

async fn follow_links(tweets: &mut Vec<Tweet>) {
    let finder = linkify::LinkFinder::new();

    for tweet in tweets {
        let text = &tweet.tweet;
        let links: Vec<_> = finder.links(text).collect();

        let mut curr_pos = 0;
        let mut final_tweet = String::new();

        for link in &links {
            let start = link.start();
            let end = link.end();

            let request = reqwest::get(link.as_str()).await;

            let final_url = match request {
                Ok(response) => response.url().as_str().to_string(),
                Err(e) => {
                    debug!(
                        "Failed to follow link >{}< ({}), using orignal url",
                        link.as_str().to_string(),
                        e
                    );
                    link.as_str().to_string()
                }
            };

            final_tweet.push_str(&text[curr_pos..start]);
            final_tweet.push_str(&final_url);
            curr_pos = end;
        }

        final_tweet.push_str(&text[curr_pos..]);

        debug!(
            "follow_links: orig. tweet >{}<, final tweet >{}<",
            text, final_tweet
        );
        tweet.tweet = final_tweet;
    }
}
