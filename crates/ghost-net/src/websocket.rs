//! Websockets that leave the machine the way HTTP requests do.
//!
//! A platform's gateway is a websocket, and a socket opened with
//! `tokio_tungstenite::connect_async` dials the host itself: no proxy, no
//! `direct` list, nothing of the rule. So a socket is opened here as what
//! it starts as, an HTTP request asking to be upgraded, made by a client
//! from [`Network::builder`](crate::Network::builder). Whatever that client
//! does for a request to the same address, the socket does: through an
//! HTTP `CONNECT` tunnel with TLS inside it for `wss://`, straight there for
//! a host that goes direct, with the proxy's password if it has one. There
//! is no second way of choosing a route to keep in step with the first.

use anyhow::{Context, bail, ensure};
use reqwest::{
    ClientBuilder, StatusCode, Upgraded, Url,
    header::{
        AUTHORIZATION, CONNECTION, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_VERSION,
        UPGRADE,
    },
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        handshake::{client::generate_key, derive_accept_key},
        protocol::Role,
    },
};

/// An open websocket, whichever way it went.
pub type WebSocket = WebSocketStream<Upgraded>;

/// Open a websocket to `url` (`ws://` or `wss://`) the way this process's
/// rule says. `authorization` is the value of an `Authorization` header, for
/// a server that asks for one in the handshake.
pub async fn websocket(url: &str, authorization: Option<&str>) -> anyhow::Result<WebSocket> {
    open(crate::builder(), url, authorization).await
}

async fn open(
    builder: ClientBuilder,
    url: &str,
    authorization: Option<&str>,
) -> anyhow::Result<WebSocket> {
    let mut address =
        Url::parse(url).with_context(|| format!("{url:?} is not a websocket address"))?;
    // The handshake is an HTTP request; the client knows only those schemes.
    let scheme = match address.scheme() {
        "ws" => "http",
        "wss" => "https",
        other => bail!("{url} is not a websocket address ({other}://)"),
    };
    let _ = address.set_scheme(scheme);
    // An upgrade is HTTP/1.1's; a server speaking HTTP/2 would not offer it.
    let client = builder
        .http1_only()
        .build()
        .context("build an HTTP client")?;
    let key = generate_key();
    let mut request = client
        .get(address)
        .header(CONNECTION, "Upgrade")
        .header(UPGRADE, "websocket")
        .header(SEC_WEBSOCKET_VERSION, "13")
        .header(SEC_WEBSOCKET_KEY, &key);
    if let Some(authorization) = authorization {
        request = request.header(AUTHORIZATION, authorization);
    }
    let response = request
        .send()
        .await
        .with_context(|| format!("connect to {url}"))?;
    ensure!(
        response.status() == StatusCode::SWITCHING_PROTOCOLS,
        "{url} answered {} instead of opening a websocket",
        response.status()
    );
    let accepted = response
        .headers()
        .get(SEC_WEBSOCKET_ACCEPT)
        .is_some_and(|accept| accept.as_bytes() == derive_accept_key(key.as_bytes()).as_bytes());
    ensure!(accepted, "{url} opened a websocket to some other request");
    let upgraded = response
        .upgrade()
        .await
        .with_context(|| format!("open the websocket to {url}"))?;
    Ok(WebSocketStream::from_raw_socket(upgraded, Role::Client, None).await)
}

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        sync::{Arc, Mutex},
    };

    use futures_util::{SinkExt, StreamExt};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    use tokio_tungstenite::tungstenite::{Message, handshake::server::Request};

    use super::*;
    use crate::{Network, Setting};

    /// A stand-in for an HTTP proxy that writes down the first line of each
    /// request it is sent and refuses it.
    async fn proxy() -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let asked = Arc::new(Mutex::new(Vec::new()));
        tokio::spawn({
            let asked = asked.clone();
            async move {
                while let Ok((mut socket, _)) = listener.accept().await {
                    let mut head = Vec::new();
                    let mut byte = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") && socket.read(&mut byte).await.is_ok() {
                        head.push(byte[0]);
                    }
                    let head = String::from_utf8_lossy(&head);
                    asked
                        .lock()
                        .unwrap()
                        .push(head.lines().next().unwrap_or_default().to_owned());
                    let refusal = "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n";
                    let _ = socket.write_all(refusal.as_bytes()).await;
                }
            }
        });
        (addr, asked)
    }

    /// A websocket server that says back what it is sent, once, after the
    /// `Authorization` it was opened with.
    async fn echo() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let mut authorization = String::new();
                #[expect(
                    clippy::result_large_err,
                    reason = "the callback returns what tungstenite asks of it"
                )]
                let mut ws =
                    tokio_tungstenite::accept_hdr_async(tcp, |request: &Request, reply| {
                        authorization = request
                            .headers()
                            .get("authorization")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or_default()
                            .to_owned();
                        Ok(reply)
                    })
                    .await
                    .unwrap();
                if let Some(Ok(Message::Text(text))) = ws.next().await {
                    let said = format!("{authorization}: {}", text.as_str());
                    ws.send(Message::text(said)).await.unwrap();
                }
            }
        });
        addr
    }

    fn through(proxy: SocketAddr) -> Network {
        let setting = Setting {
            proxy: Some(format!("http://{proxy}")),
            direct: Some(vec!["*.near.test".to_owned()]),
        };
        Network::new(&setting, |_| None).unwrap()
    }

    #[tokio::test]
    async fn a_socket_to_a_far_host_goes_through_the_proxy() {
        let (proxy, asked) = proxy().await;
        let network = through(proxy);
        let refused = open(network.builder(), "wss://gateway.far.test/socket", None)
            .await
            .unwrap_err();
        assert!(
            format!("{refused:#}").contains("gateway.far.test"),
            "{refused:#}"
        );
        // A plain socket is asked of the proxy as any plain request is.
        open(network.builder(), "ws://plain.far.test/", None)
            .await
            .unwrap_err();
        assert_eq!(
            *asked.lock().unwrap(),
            [
                "CONNECT gateway.far.test:443 HTTP/1.1",
                "GET http://plain.far.test/ HTTP/1.1"
            ]
        );
    }

    #[tokio::test]
    async fn a_socket_to_a_host_that_goes_direct_never_touches_the_proxy() {
        let (proxy, asked) = proxy().await;
        let (network, site) = (through(proxy), echo().await);
        for (host, builder) in [
            // Listed direct, and looked up as the site.
            (
                "a.near.test",
                network.builder().resolve("a.near.test", site),
            ),
            // This machine.
            ("127.0.0.1", network.builder()),
        ] {
            let url = format!("ws://{host}:{}/", site.port());
            let mut ws = open(builder, &url, Some("Bearer t")).await.unwrap();
            ws.send(Message::text("在吗")).await.unwrap();
            let Some(Ok(Message::Text(said))) = ws.next().await else {
                panic!("no echo from {host}");
            };
            assert_eq!(said.as_str(), "Bearer t: 在吗", "{host}");
        }
        assert!(asked.lock().unwrap().is_empty(), "{:?}", asked.lock());
    }

    #[tokio::test]
    async fn only_websocket_addresses_are_opened() {
        let refused = open(reqwest::Client::builder(), "https://far.test/", None)
            .await
            .unwrap_err();
        assert!(refused.to_string().contains("https://"), "{refused}");
    }
}
