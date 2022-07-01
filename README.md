# tostr
/ˈtəʊstə(r)/

**T**witter to n**ostr**.
Bot that forwards tweets to [nostr](https://github.com/nostr-protocol/nostr).


## How it works
It uses [twint](https://github.com/minamotorin/twint.git) to get the tweets,
[nostril](https://github.com/jb55/nostril) to create a nostr event and [websocat](https://github.com/vi/websocat.git) to send the event to relays.

## How to run
```
git clone https://github.com/slaninas/tostr/ && cd tostr
# Add secret to config file, choose relays and add accounts to follow
sudo docker build -t tostr . && sudo docker run --rm -ti tostr
```
Now the bot should be running. It relays only new tweets that were posted
after you launched it. It waits for `refresh_interval_secs` seconds between the checks.


## Known limitations/issues
- There are multiple processes spawned for each account check and relaying, twint also takes some time to process so it's slow,
I tested it with 40 accounts and it took almost a minute to check if there were any new tweets for them.
- twint is a Twitter scraper that currently works but who knows for how long
- Doesn't work for retweets by users you follow
- But it shows replies by peiple you follow (is that good or bad?)
