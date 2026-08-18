use serde::Serialize;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

const LAN_PORT: u16 = 9100;
/// Same 2s floor as the phone app. Sub-second timeouts look like “didn't search”.
const TCP_PROBE_TIMEOUT_MS: u64 = 2000;
const PROBE_CONCURRENCY: usize = 28;

const FALLBACK_LAN_PREFIXES: [&str; 8] = [
    "192.168.1",
    "192.168.0",
    "10.0.0",
    "192.168.68",
    "192.168.8",
    "192.168.4",
    "192.168.2",
    "10.0.1",
];

#[derive(Debug, Clone, Serialize)]
pub struct PrinterDevice {
    pub name: String,
    pub address: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanReport {
    pub devices: Vec<PrinterDevice>,
    pub lines: Vec<String>,
}

fn is_usable_v4(ip: Ipv4Addr) -> bool {
    !ip.is_loopback() && !ip.is_unspecified() && !ip.is_multicast() && !ip.is_link_local()
}

fn local_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if is_usable_v4(ip) => Some(ip),
        _ => None,
    }
}

fn hosts_for_prefix(prefix: &str, skip: Option<u8>) -> Vec<Ipv4Addr> {
    let mut hosts = Vec::with_capacity(254);
    for host in 1u8..=254 {
        if skip == Some(host) {
            continue;
        }
        let ip_str = format!("{prefix}.{host}");
        if let Ok(ip) = ip_str.parse::<Ipv4Addr>() {
            hosts.push(ip);
        }
    }
    hosts
}

fn hosts_for_local_ip(ip: Ipv4Addr) -> Vec<Ipv4Addr> {
    let oct = ip.octets();
    let prefix = format!("{}.{}.{}", oct[0], oct[1], oct[2]);
    hosts_for_prefix(&prefix, Some(oct[3]))
}

fn probe(host: Ipv4Addr, port: u16) -> Result<(), String> {
    let addr = SocketAddr::from((host, port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(TCP_PROBE_TIMEOUT_MS))
        .map(|_| ())
        .map_err(|err| err.to_string())
}

struct Sweep {
    devices: Vec<PrinterDevice>,
    probed: usize,
    errors: Vec<String>,
}

fn scan_hosts(hosts: Vec<Ipv4Addr>) -> Sweep {
    let n = hosts.len();
    if n == 0 {
        return Sweep {
            devices: Vec::new(),
            probed: 0,
            errors: Vec::new(),
        };
    }
    let found = Mutex::new(Vec::new());
    let errors = Mutex::new(Vec::new());
    let next = Mutex::new(0usize);
    let workers = PROBE_CONCURRENCY.min(n);

    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = {
                    let mut guard = next.lock().expect("probe index");
                    let i = *guard;
                    if i >= n {
                        return;
                    }
                    *guard += 1;
                    i
                };
                let host = hosts[i];
                match probe(host, LAN_PORT) {
                    Ok(()) => found.lock().expect("found list").push(PrinterDevice {
                        name: host.to_string(),
                        address: format!("lan:{host}:{LAN_PORT}"),
                    }),
                    Err(error) => {
                        let mut list = errors.lock().expect("error list");
                        if list.len() < 6 {
                            list.push(format!("{host}: {error}"));
                        }
                    }
                }
            });
        }
    });

    Sweep {
        devices: found.into_inner().expect("found list"),
        probed: n,
        errors: errors.into_inner().expect("error list"),
    }
}

/// Discover ESC/POS printers on this machine's LAN (TCP :9100).
///
/// When the local IPv4 is known, only that /24 is swept — extra prefixes
/// were the “thousands of probes, zero open” trap on the phone.
pub fn scan_lan_printers_sync() -> ScanReport {
    let started = Instant::now();
    let local = local_ipv4();
    let ip_label = local
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "none".to_string());

    let hosts = if let Some(ip) = local {
        hosts_for_local_ip(ip)
    } else {
        FALLBACK_LAN_PREFIXES
            .iter()
            .flat_map(|prefix| hosts_for_prefix(prefix, None))
            .collect()
    };

    let sweep = scan_hosts(hosts);
    let mut lines = vec![
        "tcp=yes thermal=n/a".to_string(),
        format!("timeout_ms={TCP_PROBE_TIMEOUT_MS} concurrency={PROBE_CONCURRENCY}"),
        format!(
            "probed={} open={} ms={}",
            sweep.probed,
            sweep.devices.len(),
            started.elapsed().as_millis()
        ),
    ];
    lines.extend(sweep.errors);
    lines.push(format!("done ip={ip_label}"));

    ScanReport {
        devices: sweep.devices,
        lines,
    }
}

pub fn parse_lan_address(address: &str) -> Result<(String, u16), String> {
    let raw = address.trim();
    if raw.len() < 5 || !raw[..4].eq_ignore_ascii_case("lan:") {
        return Err("invalid_lan_address".into());
    }
    let rest = &raw[4..];
    let (host, port_str) = rest
        .rsplit_once(':')
        .filter(|(host, _)| !host.is_empty())
        .ok_or_else(|| "invalid_lan_address".to_string())?;
    let port: u16 = port_str
        .parse()
        .map_err(|_| "invalid_lan_address".to_string())?;
    if !(1..=65535).contains(&port) {
        return Err("invalid_lan_address".into());
    }
    Ok((host.to_string(), port))
}

pub fn confirm_lan_printer_sync(address: String) -> Result<(), String> {
    let (host, port) = parse_lan_address(&address)?;
    let sock: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|_| "invalid_lan_address".to_string())?;
    TcpStream::connect_timeout(&sock, Duration::from_millis(TCP_PROBE_TIMEOUT_MS))
        .map(|_| ())
        .map_err(|_| "connect_failed".to_string())
}

pub fn print_lan_sync(address: String, data: Vec<u8>) -> Result<(), String> {
    let (host, port) = parse_lan_address(&address)?;
    let sock: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|_| "invalid_lan_address".to_string())?;
    let mut stream = TcpStream::connect_timeout(&sock, Duration::from_millis(5000))
        .map_err(|_| "connect_failed".to_string())?;
    stream
        .write_all(&data)
        .map_err(|_| "print_failed".to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lan_address() {
        assert_eq!(
            parse_lan_address("lan:10.0.0.41:9100").unwrap(),
            ("10.0.0.41".into(), 9100)
        );
        assert!(parse_lan_address("bt:AA:BB").is_err());
        assert!(parse_lan_address("lan:10.0.0.41").is_err());
    }

    #[test]
    fn local_ip_sweep_skips_self() {
        let hosts = hosts_for_local_ip(Ipv4Addr::new(10, 0, 0, 39));
        assert_eq!(hosts.len(), 253);
        assert_eq!(hosts[0], Ipv4Addr::new(10, 0, 0, 1));
        assert!(!hosts.contains(&Ipv4Addr::new(10, 0, 0, 39)));
        assert!(hosts.contains(&Ipv4Addr::new(10, 0, 0, 41)));
    }
}
