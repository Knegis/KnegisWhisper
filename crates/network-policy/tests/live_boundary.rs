//! Host-dependent traffic probe for the SSRF boundary.
//!
//! This is ignored in the ordinary suite because it opens listeners on the
//! machine's active LAN address. Run it explicitly on the Windows validation
//! machine with:
//!
//! `cargo test -p kw-network-policy --test live_boundary -- --ignored --nocapture`

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::io::{Read as _, Write as _};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, UdpSocket};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::Duration;

use kw_network_policy::{validate, NetworkPolicy, PolicyError, RequestPurpose, ALL_PURPOSES};

const SELF_HOSTED: RequestPurpose = RequestPurpose::ExplicitlyConfiguredSelfHostedEndpoint;

fn active_lan_ipv4() -> Ipv4Addr {
    // UDP connect chooses a route without sending a packet. Reading the local
    // endpoint gives us the interface Windows would use for ordinary egress,
    // so the HTTP probe is genuinely non-loopback without hard-coding this
    // machine's current DHCP lease.
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("bind route probe");
    socket
        .connect((Ipv4Addr::new(192, 0, 2, 1), 80))
        .expect("select an active route");
    let IpAddr::V4(address) = socket.local_addr().expect("read selected route").ip() else {
        panic!("the selected route was not IPv4");
    };
    assert!(
        !address.is_loopback(),
        "the probe requires a non-loopback LAN address"
    );
    assert!(
        kw_network_policy::is_private_host(&address.to_string()),
        "the active address {address} is not private; refusing to expose a test listener"
    );
    address
}

fn listener(
    address: Ipv4Addr,
    response: Option<&'static [u8]>,
) -> (SocketAddr, Receiver<Vec<u8>>, JoinHandle<()>) {
    let listener = TcpListener::bind((address, 0)).expect("bind traffic probe");
    let address = listener.local_addr().expect("read traffic probe address");
    let (sent, received) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept traffic probe");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("set probe timeout");
        let mut bytes = vec![0_u8; 4096];
        let count = stream.read(&mut bytes).expect("read traffic probe");
        bytes.truncate(count);
        sent.send(bytes).expect("report traffic probe");
        if let Some(response) = response {
            stream.write_all(response).expect("answer traffic probe");
        }
    });
    (address, received, worker)
}

fn assert_cloud_refusal_names_host(url: &str, expected_host: &str) {
    for purpose in ALL_PURPOSES {
        if purpose == SELF_HOSTED {
            continue;
        }
        let error = validate(url, purpose).expect_err("cloud purpose must refuse a private host");
        assert!(
            error.to_string().contains(expected_host),
            "{} refusal for {url} did not name {expected_host}: {error}",
            purpose.as_str()
        );
    }
}

async fn assert_refused_before_connect(
    policy: &NetworkPolicy,
    listener: &TcpListener,
    url: &str,
    expected_host: &str,
) {
    for purpose in ALL_PURPOSES {
        if purpose == SELF_HOSTED {
            continue;
        }
        let error = policy
            .get_text(purpose, url)
            .await
            .expect_err("cloud request must be refused before send");
        assert!(
            error.to_string().contains(expected_host),
            "{} refusal for {url} did not name {expected_host}: {error}",
            purpose.as_str()
        );
    }

    listener
        .set_nonblocking(true)
        .expect("make refusal listener observable");
    let error = listener
        .accept()
        .expect_err("a refused request unexpectedly reached the listener");
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
}

#[tokio::test]
#[ignore = "opens listeners on this machine's active LAN interface"]
async fn real_traffic_crosses_only_the_self_hosted_boundary() {
    let lan = active_lan_ipv4();
    let policy = NetworkPolicy::new().expect("build network policy");

    // The established capability: explicit self-hosted HTTP reaches a server
    // on another interface, not merely a loopback test double.
    let (http_address, request, worker) = listener(
        lan,
        Some(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"),
    );
    let body = policy
        .get_text(SELF_HOSTED, &format!("http://{http_address}/probe"))
        .await
        .expect("self-hosted LAN HTTP should reach the listener");
    assert_eq!(body, "ok");
    let bytes = request
        .recv_timeout(Duration::from_secs(2))
        .expect("observe HTTP request");
    assert!(bytes.starts_with(b"GET /probe HTTP/1.1"));
    worker.join().expect("join HTTP listener");

    // The capability added by the SSRF fix: private HTTPS is admitted for the
    // self-hosted purpose. The tiny server intentionally is not TLS; seeing a
    // ClientHello and then a TLS error proves policy allowed the request to
    // leave while avoiding certificates or secrets in this probe.
    let (tls_address, hello, worker) = listener(lan, None);
    let error = policy
        .get_text(SELF_HOSTED, &format!("https://{tls_address}/probe"))
        .await
        .expect_err("a plaintext listener cannot finish a TLS request");
    assert!(matches!(error, PolicyError::Client(_)));
    let bytes = hello
        .recv_timeout(Duration::from_secs(2))
        .expect("observe TLS ClientHello");
    assert!(bytes.len() > 5, "TLS probe sent no record");
    assert_eq!(
        bytes.first(),
        Some(&0x16),
        "first TLS record was not a handshake"
    );
    worker.join().expect("join TLS listener");

    // The narrowing: every non-self-hosted purpose refuses these targets at
    // request construction, the diagnostics name the target host, and real
    // listening sockets observe no connection.
    let refused_http = TcpListener::bind((lan, 0)).expect("bind refused HTTP listener");
    let refused_http_address = refused_http
        .local_addr()
        .expect("read refused HTTP address");
    assert_refused_before_connect(
        &policy,
        &refused_http,
        &format!("http://{refused_http_address}/"),
        &lan.to_string(),
    )
    .await;

    let refused_https = TcpListener::bind((lan, 0)).expect("bind refused HTTPS listener");
    let refused_https_address = refused_https
        .local_addr()
        .expect("read refused HTTPS address");
    assert_refused_before_connect(
        &policy,
        &refused_https,
        &format!("https://{refused_https_address}/"),
        &lan.to_string(),
    )
    .await;

    let refused_trailing_dot =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind trailing-dot refusal listener");
    let trailing_dot_port = refused_trailing_dot
        .local_addr()
        .expect("read trailing-dot listener address")
        .port();
    assert_refused_before_connect(
        &policy,
        &refused_trailing_dot,
        &format!("https://localhost.:{trailing_dot_port}/"),
        "localhost.",
    )
    .await;

    assert_cloud_refusal_names_host("https://169.254.169.254/", "169.254.169.254");

    // These ranges usually are not assigned to the validation machine, so a
    // live reachability assertion would test routing rather than the policy.
    // Assert both sides of the deliberate widening directly: cloud purposes
    // refuse them and the explicit self-hosted purpose admits plaintext.
    for host in ["100.64.0.1", "0.0.0.1"] {
        let url = format!("http://{host}/");
        assert_cloud_refusal_names_host(&url, host);
        assert!(
            validate(&url, SELF_HOSTED).is_ok(),
            "self-hosted should admit {url}"
        );
    }
}
