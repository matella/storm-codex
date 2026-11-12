use std::net::{Ipv4Addr, TcpListener};

/// Returns `preferred` if it can be bound on 127.0.0.1, otherwise an OS-assigned free port.
pub fn pick_port(preferred: u16) -> std::io::Result<u16> {
    if TcpListener::bind((Ipv4Addr::LOCALHOST, preferred)).is_ok() {
        return Ok(preferred);
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    Ok(listener.local_addr()?.port())
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
}
