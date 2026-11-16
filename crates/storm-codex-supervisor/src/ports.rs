use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// How long the `[::1]` probe waits for a listener to accept. Loopback accepts at once; Windows
/// can take about a second to report a refused loopback connection, hence the short cap.
const IPV6_PROBE_TIMEOUT: Duration = Duration::from_millis(250);

/// How many OS-assigned ports are tried before giving up on a port free on both loopbacks.
const FALLBACK_TRIES: usize = 16;

/// Returns `preferred` if it is free, otherwise an OS-assigned free port.
///
/// Free means: it can be bound on 127.0.0.1 **and** nothing answers on `[::1]`. The server binds
/// 127.0.0.1 only, but `http://localhost:<port>` (the OBS URLs) resolves to `::1` first on
/// Windows: another program listening on `[::]:<port>` (qBittorrent's web UI on 8088, for
/// example) would answer those URLs instead of us. Binding `[::1]` is not a reliable test (Windows
/// lets the same user bind a specific address over another socket's wildcard), so `[::1]` is
/// probed with a connection attempt instead.
pub fn pick_port(preferred: u16) -> std::io::Result<u16> {
    if TcpListener::bind((Ipv4Addr::LOCALHOST, preferred)).is_ok()
        && !listening_on_ipv6_loopback(preferred)
    {
        return Ok(preferred);
    }
    let mut last = None;
    for _ in 0..FALLBACK_TRIES {
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?
            .local_addr()?
            .port();
        if !listening_on_ipv6_loopback(port) {
            return Ok(port);
        }
        last = Some(port);
    }
    // Every candidate was answered on [::1]: keep the IPv4-free one rather than fail the start.
    match last {
        Some(port) => Ok(port),
        None => Err(std::io::Error::other("no free loopback port")),
    }
}

/// `true` when something accepts a TCP connection on `[::1]:port` (no IPv6 loopback = `false`).
fn listening_on_ipv6_loopback(port: u16) -> bool {
    let addr = SocketAddr::from((Ipv6Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, IPV6_PROBE_TIMEOUT).is_ok()
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn free_port() -> u16 {
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("bind")
            .local_addr()
            .expect("addr")
            .port()
    }

    #[test]
    fn pick_port_returns_preferred_when_free() {
        let p = free_port();
        assert_eq!(pick_port(p).expect("pick"), p);
    }

    #[test]
    fn pick_port_falls_back_when_taken() {
        let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
        let p = held.local_addr().expect("addr").port();
        let got = pick_port(p).expect("pick");
        assert_ne!(got, p);
        assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, got)).is_ok());
    }

    /// Another program listening only on `[::1]` (or `[::]`) answers `http://localhost:<port>`
    /// (Windows resolves `localhost` to `::1` first), so that port is not ours to take even
    /// though 127.0.0.1 can still be bound.
    #[test]
    fn pick_port_falls_back_when_taken_on_ipv6_loopback_only() {
        let Ok(held) = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)) else {
            eprintln!("no IPv6 loopback on this machine: test skipped");
            return;
        };
        let p = held.local_addr().expect("addr").port();
        if TcpListener::bind((Ipv4Addr::LOCALHOST, p)).is_err() {
            eprintln!("port {p} also taken on 127.0.0.1: test not meaningful, skipped");
            return;
        }
        let got = pick_port(p).expect("pick");
        assert_ne!(got, p);
    }
}
