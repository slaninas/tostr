use secp256k1::{rand, KeyPair, Secp256k1};
use futures_util::sink::SinkExt;

// TODO: Add remaining fields
pub struct Event {
    id: String,
    pubkey: String,
    created_at: u64,
    kind: u64,
    content: String,
    sig: String,
}

impl Event {
    pub fn new(secret: String, content: String, created_at: u64) -> Self {
        let secp = Secp256k1::new();

        let key_pair = secp256k1::KeyPair::from_seckey_str(&secp, &secret).unwrap();
        let (pubkey, parity) = key_pair.x_only_public_key();
        // println!("secret {}", key_pair.display_secret());
        // println!("pubkey {}", pubkey);

        let msg = format!(r#"[0,"{}",{},1,[],"{}"]"#, pubkey, created_at, content);
        let id =
            secp256k1::Message::from_hashed_data::<secp256k1::hashes::sha256::Hash>(msg.as_bytes());

        let signature = secp.sign_schnorr(&id, &key_pair);

        Event {
            id: id.to_string(),
            pubkey: pubkey.to_string(),
            created_at,
            kind: 1,
            content,
            sig: signature.to_string(),
        }
    }

    pub fn format(&self) -> String{
        format!(
            r#"["EVENT",{{"id": "{}", "pubkey": "{}", "created_at": {}, "kind": {}, "tags": [], "content": "{}", "sig": "{}"}}]"#,
            self.id, self.pubkey, self.created_at, self.kind, self.content, self.sig
        )
    }
    pub fn print(&self) {
        println!("{}", self.format());
    }

    pub async fn send(&self, address: &String) {
        // TODO: Keep the connection alive
        let (mut ws_stream, response) =
            tokio_tungstenite::connect_async(url::Url::parse(address).unwrap()).await
                .expect("Can't connect");

        ws_stream.send(tungstenite::Message::Text(self.format().into())).await.unwrap();
        ws_stream.close(None).await.unwrap();

    }
}

