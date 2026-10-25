//! Web Push: word of new chapters straight to a browser, or to Mango added to a phone's
//! home screen, with no app of its own. A device that asks gets an address from its
//! browser's push service (Google's, Mozilla's, Apple's, Microsoft's). Mango keeps it and
//! sends each message there, encrypted for that device alone (RFC 8291) and signed with
//! the account's own key (VAPID, RFC 8292). The push service learns only where to deliver.
//!
//! The address comes from the browser, so it is held to what `net.rs` holds any address an
//! account names to, with local ones never allowed: HTTPS and the public internet only.

use crate::{
    db::{now, Db},
    error::{AppError, Result},
    net,
};
use aes_gcm::{aead::Aead, Aes128Gcm, KeyInit, Nonce};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use hkdf::Hkdf;
use p256::{
    ecdsa::{signature::Signer, Signature, SigningKey},
    elliptic_curve::sec1::ToEncodedPoint,
    PublicKey, SecretKey,
};
use reqwest::{Client, StatusCode, Url};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;
use std::time::Duration;

/// How long a push service holds a message for a device that is off, in seconds.
const TTL: &str = "86400";
/// One record carries the whole message.
const RECORD: u32 = 4096;
/// Push services take 4 KB at most, encryption included.
const MAX_PAYLOAD: usize = 3000;
/// The most devices one account may have told of new chapters.
const MAX_DEVICES: i64 = 20;
/// Who sends, as the push service is told, when the page gave no address of its own.
const CONTACT: &str = "https://github.com/apixelsnet/mango";

/// What a browser's `PushSubscription.toJSON()` gives.
#[derive(Deserialize)]
pub struct Subscription {
    pub endpoint: String,
    pub keys: Keys,
}

#[derive(Deserialize)]
pub struct Keys {
    pub p256dh: String,
    pub auth: String,
}

/// What the push service is asked to show.
#[derive(Serialize)]
pub struct Note<'a> {
    pub title: &'a str,
    pub body: &'a str,
    /// Where tapping it leads, inside Mango.
    pub url: &'a str,
}

struct Device {
    endpoint: String,
    p256dh: Vec<u8>,
    auth: Vec<u8>,
    origin: Option<String>,
}

#[derive(Clone)]
pub struct Push {
    db: Db,
    http: Client,
}

impl Push {
    pub fn new(db: Db) -> Result<Self> {
        let builder = Client::builder()
            .user_agent(concat!("Mango/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20));
        let http = net::guard(builder, false).build().map_err(|e| AppError::internal(e.to_string()))?;
        Ok(Self { db, http })
    }

    /// The account's public key, which a browser needs to subscribe, and how many devices it has.
    pub async fn status(&self) -> Result<(String, i64)> {
        self.db
            .call(|conn| {
                let key = B64.encode(public(&signing_key(conn)?));
                Ok((key, conn.query_row("SELECT COUNT(*) FROM push_devices", [], |r| r.get(0))?))
            })
            .await
    }

    /// Keeps a device's subscription, in place of the one it had at that address.
    pub async fn add(&self, subscription: Subscription, origin: Option<String>) -> Result<()> {
        let endpoint = checked(&subscription.endpoint)?;
        let bad = || AppError::bad_request("This browser's subscription is not one Mango can use");
        let p256dh = decode(&subscription.keys.p256dh).filter(|key| PublicKey::from_sec1_bytes(key).is_ok()).ok_or_else(bad)?;
        let auth = decode(&subscription.keys.auth).filter(|auth| auth.len() == 16).ok_or_else(bad)?;
        let origin = origin.filter(|origin| origin.starts_with("https://") && origin.len() < 300);
        self.db
            .call(move |conn| {
                let others: i64 = conn.query_row("SELECT COUNT(*) FROM push_devices WHERE endpoint <> ?1", [&endpoint], |r| r.get(0))?;
                if others >= MAX_DEVICES {
                    return Err(AppError::bad_request(format!("An account can notify {MAX_DEVICES} devices at most. Turn it off on one first.")));
                }
                conn.execute(
                    "INSERT INTO push_devices (endpoint, p256dh, auth, origin, added_at) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(endpoint) DO UPDATE SET p256dh = excluded.p256dh, auth = excluded.auth, origin = excluded.origin",
                    params![endpoint, p256dh, auth, origin, now()],
                )?;
                Ok(())
            })
            .await
    }

    /// Forgets a device. One that was not known is no fault.
    pub async fn remove(&self, endpoint: String) -> Result<()> {
        self.db.call(move |conn| Ok(conn.execute("DELETE FROM push_devices WHERE endpoint = ?1", [endpoint]).map(|_| ())?)).await
    }

    /// Sends a note to every device of the account. A device its push service says is gone
    /// is forgotten. Answers how many took it.
    pub async fn send(&self, note: &Note<'_>) -> Result<usize> {
        let (key, devices) = self.db.call(|conn| Ok((signing_key(conn)?, devices(conn)?))).await?;
        if devices.is_empty() {
            return Ok(0);
        }
        let mut payload = serde_json::to_vec(note).map_err(|e| AppError::internal(e.to_string()))?;
        if payload.len() > MAX_PAYLOAD {
            let body: String = note.body.chars().take(400).collect();
            payload = serde_json::to_vec(&Note { body: &body, ..*note }).map_err(|e| AppError::internal(e.to_string()))?;
        }
        let mut sent = 0;
        for device in devices {
            match self.deliver(&key, &device, &payload).await {
                Ok(true) => sent += 1,
                Ok(false) => {
                    let gone = device.endpoint.clone();
                    let _ = self.db.call(move |conn| Ok(conn.execute("DELETE FROM push_devices WHERE endpoint = ?1", [gone])?)).await;
                }
                Err(e) => tracing::warn!("push: {}", e.message),
            }
        }
        Ok(sent)
    }

    /// Answers false when the push service says the device is gone for good.
    async fn deliver(&self, key: &SecretKey, device: &Device, payload: &[u8]) -> Result<bool> {
        let url = Url::parse(&checked(&device.endpoint)?).map_err(|_| AppError::internal("a kept push address broke"))?;
        let body = encrypt(&device.p256dh, &device.auth, payload, random_key()?, random_salt()?)?;
        let token = vapid(key, &url.origin().ascii_serialization(), device.origin.as_deref().unwrap_or(CONTACT), now());
        let answer = self
            .http
            .post(url)
            .header("TTL", TTL)
            .header("Urgency", "normal")
            .header("Content-Encoding", "aes128gcm")
            .header("Content-Type", "application/octet-stream")
            .header("Authorization", format!("vapid t={token}, k={}", B64.encode(public(key))))
            .body(body)
            .send()
            .await
            .map_err(|e| AppError::new(502, "push_failed", format!("The push service did not answer: {}", net::told(&e))))?;
        match answer.status() {
            status if status.is_success() => Ok(true),
            StatusCode::NOT_FOUND | StatusCode::GONE => Ok(false),
            status => Err(AppError::new(502, "push_failed", format!("The push service answered {}", status.as_u16()))),
        }
    }
}

/// A push address, when it is one Mango may send to.
fn checked(endpoint: &str) -> Result<String> {
    let url = Url::parse(endpoint).ok().filter(|url| endpoint.len() <= 1000 && net::refusal(url, false).is_none());
    url.map(|_| endpoint.to_owned()).ok_or_else(|| AppError::bad_request("Push addresses are HTTPS addresses on the internet"))
}

fn decode(text: &str) -> Option<Vec<u8>> {
    B64.decode(text.trim().trim_end_matches('=')).ok()
}

fn devices(conn: &Connection) -> Result<Vec<Device>> {
    let mut stmt = conn.prepare("SELECT endpoint, p256dh, auth, origin FROM push_devices ORDER BY added_at")?;
    let rows = stmt.query_map([], |r| Ok(Device { endpoint: r.get(0)?, p256dh: r.get(1)?, auth: r.get(2)?, origin: r.get(3)? }))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The account's signing key, made the first time it is wanted.
fn signing_key(conn: &Connection) -> Result<SecretKey> {
    if let Some(secret) = conn.query_row("SELECT secret FROM push_key WHERE id = 1", [], |r| r.get::<_, Vec<u8>>(0)).optional()? {
        return SecretKey::from_slice(&secret).map_err(|_| AppError::internal("the push key is broken"));
    }
    let key = random_key()?;
    conn.execute("INSERT INTO push_key (id, secret) VALUES (1, ?1)", [key.to_bytes().to_vec()])?;
    Ok(key)
}

fn random_key() -> Result<SecretKey> {
    loop {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).map_err(|e| AppError::internal(e.to_string()))?;
        // Nearly every 32 bytes make a key; the few that do not are drawn again.
        if let Ok(key) = SecretKey::from_slice(&bytes) {
            return Ok(key);
        }
    }
}

fn random_salt() -> Result<[u8; 16]> {
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).map_err(|e| AppError::internal(e.to_string()))?;
    Ok(salt)
}

/// A key's public half, uncompressed, as push services and browsers write it.
fn public(key: &SecretKey) -> Vec<u8> {
    key.public_key().to_encoded_point(false).as_bytes().to_vec()
}

/// The signed token that says the message is the account's, for one push service, for 12 hours.
fn vapid(key: &SecretKey, audience: &str, contact: &str, at: i64) -> String {
    let head = B64.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
    let claims = B64.encode(json!({ "aud": audience, "exp": at + 12 * 3600, "sub": contact }).to_string());
    let signed = format!("{head}.{claims}");
    let signature: Signature = SigningKey::from(key).sign(signed.as_bytes());
    format!("{signed}.{}", B64.encode(signature.to_bytes()))
}

/// The message encrypted for one device, as one `aes128gcm` record (RFC 8188, RFC 8291):
/// the salt, the record size, the key it was sealed with, then the sealed text.
fn encrypt(device_key: &[u8], auth: &[u8], payload: &[u8], own: SecretKey, salt: [u8; 16]) -> Result<Vec<u8>> {
    let broken = |_| AppError::internal("could not encrypt a push message");
    let device = PublicKey::from_sec1_bytes(device_key).map_err(|_| AppError::bad_request("a device's push key is broken"))?;
    let shared = p256::ecdh::diffie_hellman(own.to_nonzero_scalar(), device.as_affine());
    let own_public = public(&own);
    let info = [b"WebPush: info\0".as_slice(), device_key, &own_public].concat();
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes()).expand(&info, &mut ikm).map_err(|_| AppError::internal("could not encrypt a push message"))?;
    let keys = Hkdf::<Sha256>::new(Some(&salt), &ikm);
    let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
    keys.expand(b"Content-Encoding: aes128gcm\0", &mut cek).map_err(|_| AppError::internal("could not encrypt a push message"))?;
    keys.expand(b"Content-Encoding: nonce\0", &mut nonce).map_err(|_| AppError::internal("could not encrypt a push message"))?;
    // The last record ends in 2, and has no padding.
    let plain = [payload, &[2]].concat();
    let sealed = Aes128Gcm::new_from_slice(&cek).map_err(|_| AppError::internal("could not encrypt a push message"))?.encrypt(Nonce::from_slice(&nonce), plain.as_slice()).map_err(broken)?;
    Ok([salt.as_slice(), &RECORD.to_be_bytes(), &[own_public.len() as u8], &own_public, &sealed].concat())
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::{signature::Verifier, VerifyingKey};

    fn key(text: &str) -> SecretKey {
        SecretKey::from_slice(&decode(text).unwrap()).unwrap()
    }

    /// The example in RFC 8291, appendix A.
    #[test]
    fn encrypts_as_the_rfc_does() {
        let own = key("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw");
        assert_eq!(B64.encode(public(&own)), "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8");
        let device = decode("BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4").unwrap();
        let auth = decode("BTBZMqHH6r4Tts7J_aSIgg").unwrap();
        let salt: [u8; 16] = decode("DGv6ra1nlYgDCS1FRnbzlw").unwrap().try_into().unwrap();
        let body = encrypt(&device, &auth, b"When I grow up, I want to be a watermelon", own, salt).unwrap();
        assert_eq!(
            B64.encode(body),
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
        );
    }

    #[test]
    fn the_token_is_signed_by_the_key() {
        let own = key("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw");
        let token = vapid(&own, "https://push.example.net", "https://mango.example.org", 1000);
        let (signed, signature) = token.rsplit_once('.').unwrap();
        let claims: serde_json::Value = serde_json::from_slice(&decode(signed.split('.').nth(1).unwrap()).unwrap()).unwrap();
        assert_eq!(claims, json!({ "aud": "https://push.example.net", "exp": 1000 + 12 * 3600, "sub": "https://mango.example.org" }));
        let signature = Signature::from_slice(&decode(signature).unwrap()).unwrap();
        assert!(VerifyingKey::from(own.public_key()).verify(signed.as_bytes(), &signature).is_ok());
    }

    #[test]
    fn only_public_https_addresses() {
        assert!(checked("https://fcm.googleapis.com/fcm/send/abc").is_ok());
        for endpoint in ["http://fcm.googleapis.com/x", "https://127.0.0.1/x", "https://10.0.0.2/x", "https://localhost/x", "file:///etc/passwd"] {
            assert!(checked(endpoint).is_err(), "{endpoint}");
        }
    }

    #[tokio::test]
    async fn devices_are_kept_and_forgotten() {
        let push = Push::new(Db::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
        let device = B64.encode(public(&random_key().unwrap()));
        let subscription = |endpoint: &str, p256dh: &str| Subscription {
            endpoint: endpoint.into(),
            keys: Keys { p256dh: p256dh.into(), auth: B64.encode([7u8; 16]) },
        };
        push.add(subscription("https://push.example.net/a", &device), Some("https://mango.example.org".into())).await.unwrap();
        push.add(subscription("https://push.example.net/a", &device), None).await.unwrap();
        assert_eq!(push.status().await.unwrap().1, 1, "the same address twice is one device");
        assert!(push.add(subscription("https://push.example.net/b", "AAAA"), None).await.is_err(), "a broken key is refused");
        let (key, _) = push.status().await.unwrap();
        assert_eq!(key, push.status().await.unwrap().0, "the key stays");
        push.remove("https://push.example.net/a".into()).await.unwrap();
        assert_eq!(push.status().await.unwrap().1, 0);
    }
}
