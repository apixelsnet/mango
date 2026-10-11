//! Which addresses Mango fetches from when an account names them.
//!
//! A store's address comes from whoever is signed in. Left unchecked it could aim the
//! server at this machine or its network, and the error would say what answered there.
//! So unless the owner says stores may be local, they are fetched over HTTPS and from
//! public addresses only: the address given, every name it resolves to, and every
//! redirect on the way.

use reqwest::{
    dns::{Addrs, Name, Resolve, Resolving},
    redirect::Policy,
    ClientBuilder, Url,
};
use std::{
    error::Error,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};

const NOT_HTTPS: &str = "Stores are fetched over HTTPS. MANGO_STORES_LOCAL=on allows plain HTTP.";
const LOCAL: &str = "That address is on this machine or its network. MANGO_STORES_LOCAL=on allows it.";

/// Whether an address is this machine's, its network's, or no host's at all.
pub fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => private_v4(ip),
        IpAddr::V6(ip) => {
            if let Some(inner) = ip.to_ipv4_mapped() {
                return private_v4(inner);
            }
            let parts = ip.segments();
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                // Unique local and link local.
                || parts[0] & 0xfe00 == 0xfc00
                || parts[0] & 0xffc0 == 0xfe80
                // 64:ff9b::/96 carries an IPv4 address in its last 32 bits.
                || (parts[..6] == [0x64, 0xff9b, 0, 0, 0, 0] && private_v4(Ipv4Addr::from((u32::from(parts[6]) << 16) | u32::from(parts[7]))))
        }
    }
}

fn private_v4(ip: Ipv4Addr) -> bool {
    let [first, second, ..] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || first == 0
        // Shared between a provider and its customers, and the range kept for testing networks.
        || (first == 100 && second & 0xc0 == 64)
        || (first == 198 && second & 0xfe == 18)
}

/// Whether an address names this machine or its network outright: a number in a private
/// range, or `localhost`. A name that only resolves to one is caught at lookup.
pub fn names_this_network(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let host = host.trim_end_matches('.');
    match host.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>() {
        Ok(ip) => private(ip),
        Err(_) => host.is_empty() || host == "localhost" || host.ends_with(".localhost"),
    }
}

/// Why `url` is not fetched, when it is not.
pub fn refusal(url: &Url, local: bool) -> Option<&'static str> {
    match () {
        _ if local => None,
        _ if url.scheme() != "https" => Some(NOT_HTTPS),
        _ if names_this_network(url) => Some(LOCAL),
        _ => None,
    }
}

/// Hands a client only the public addresses a name resolves to.
struct PublicOnly;

impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let found: Vec<SocketAddr> = tokio::net::lookup_host((name.as_str(), 0)).await?.filter(|at| !private(at.ip())).collect();
            if found.is_empty() {
                return Err(LOCAL.into());
            }
            Ok::<Addrs, Box<dyn Error + Send + Sync>>(Box::new(found.into_iter()))
        })
    }
}

/// Makes a client keep to public addresses, through name lookups and redirects too.
pub fn guard(builder: ClientBuilder, local: bool) -> ClientBuilder {
    if local {
        return builder;
    }
    builder.dns_resolver(Arc::new(PublicOnly)).redirect(Policy::custom(|attempt| {
        if attempt.previous().len() >= 10 {
            return attempt.error("too many redirects");
        }
        match refusal(attempt.url(), false) {
            Some(why) => attempt.error(why),
            None => attempt.follow(),
        }
    }))
}

/// An error with what caused it, which `reqwest` leaves out of its own text.
pub fn told(error: &dyn Error) -> String {
    let mut text = error.to_string();
    let mut cause = error.source();
    while let Some(inner) = cause {
        let more = inner.to_string();
        if !text.contains(&more) {
            text = format!("{text}: {more}");
        }
        cause = inner.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(url: &str) -> bool {
        refusal(&Url::parse(url).unwrap(), false).is_some()
    }

    #[tokio::test]
    async fn a_name_that_leads_to_this_machine_is_not_followed() {
        // `localhost` stands in for any name whose lookup answers with a private address.
        let client = guard(reqwest::Client::builder(), false).build().unwrap();
        let error = client.get("https://localhost:9/repo.json").send().await.unwrap_err();
        assert!(told(&error).contains("on this machine"), "{}", told(&error));
    }

    #[test]
    fn only_public_https_addresses_are_fetched() {
        assert!(!refused("https://example.org/repo.json"));
        assert!(!refused("https://203.0.113.9/index.pb"));
        assert!(refused("http://example.org/repo.json"), "plain HTTP lets the network offer its own key");
        for local in ["127.0.0.1", "10.0.0.1", "172.16.0.1", "192.168.0.1", "169.254.169.254", "100.64.0.1", "0.0.0.0", "localhost", "store.localhost"] {
            assert!(refused(&format!("https://{local}/repo.json")), "{local}");
        }
        for local in ["[::1]", "[fd00::1]", "[fe80::1]", "[::ffff:10.0.0.1]", "[64:ff9b::a00:1]"] {
            assert!(refused(&format!("https://{local}/repo.json")), "{local}");
        }
        // Other ways to write the same number are read as that number.
        assert!(refused("https://2130706433/"));
        assert!(refused("https://0x7f.1/"));
        assert!(refusal(&Url::parse("http://10.0.0.1/repo.json").unwrap(), true).is_none(), "the owner may allow local stores");
    }
}
