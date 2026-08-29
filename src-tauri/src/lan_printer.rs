use serde::Serialize;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicUsize, Ordering};
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

/// Windows returns localized text; match WSA codes first.
fn classify_probe_error(error: &str) -> &'static str {
    if error.contains("10013") {
        return "blocked";
    }
    if error.contains("10060") || error.contains("10035") {
        return "timeout";
    }
    if error.contains("10061") {
        return "refused";
    }
    if error.contains("10051") || error.contains("10065") || error.contains("1231") {
        return "unreachable";
    }
    let e = error.to_ascii_lowercase();
    if e.contains("timed out") || e.contains("timeout") {
        return "timeout";
    }
    if e.contains("refused") {
        return "refused";
    }
    if e.contains("unreachable") {
        return "unreachable";
    }
    "other"
}

struct Sweep {
    devices: Vec<PrinterDevice>,
    probed: usize,
    errors: Vec<String>,
    timeout: usize,
    refused: usize,
    blocked: usize,
    unreachable: usize,
    other: usize,
}

fn empty_sweep() -> Sweep {
    Sweep {
        devices: Vec::new(),
        probed: 0,
        errors: Vec::new(),
        timeout: 0,
        refused: 0,
        blocked: 0,
        unreachable: 0,
        other: 0,
    }
}

fn scan_hosts(hosts: Vec<Ipv4Addr>) -> Sweep {
    let n = hosts.len();
    if n == 0 {
        return empty_sweep();
    }
    let found = Mutex::new(Vec::new());
    let errors = Mutex::new(Vec::new());
    let next = Mutex::new(0usize);
    let timeout = AtomicUsize::new(0);
    let refused = AtomicUsize::new(0);
    let blocked = AtomicUsize::new(0);
    let unreachable = AtomicUsize::new(0);
    let other = AtomicUsize::new(0);
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
                        let bucket = match classify_probe_error(&error) {
                            "timeout" => &timeout,
                            "refused" => &refused,
                            "blocked" => &blocked,
                            "unreachable" => &unreachable,
                            _ => &other,
                        };
                        bucket.fetch_add(1, Ordering::Relaxed);
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
        timeout: timeout.load(Ordering::Relaxed),
        refused: refused.load(Ordering::Relaxed),
        blocked: blocked.load(Ordering::Relaxed),
        unreachable: unreachable.load(Ordering::Relaxed),
        other: other.load(Ordering::Relaxed),
    }
}

fn report_from_sweep(
    sweep: Sweep,
    started: Instant,
    ip_label: String,
    extra: Vec<String>,
) -> ScanReport {
    let mut lines = vec![
        "tcp=yes thermal=n/a".to_string(),
        format!("timeout_ms={TCP_PROBE_TIMEOUT_MS} concurrency={PROBE_CONCURRENCY}"),
    ];
    lines.extend(extra);
    lines.push(format!(
        "probed={} open={} ms={}",
        sweep.probed,
        sweep.devices.len(),
        started.elapsed().as_millis()
    ));
    lines.push(format!(
        "errs=timeout:{} refused:{} blocked:{} unreachable:{} other:{}",
        sweep.timeout, sweep.refused, sweep.blocked, sweep.unreachable, sweep.other
    ));
    lines.extend(sweep.errors);
    lines.push(format!("done ip={ip_label}"));
    ScanReport {
        devices: sweep.devices,
        lines,
    }
}

/// Mac / Linux: one /24 from the default route. Do not change this path —
/// the Mac build already finds printers this way.
#[cfg(not(windows))]
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

    report_from_sweep(scan_hosts(hosts), started, ip_label, Vec::new())
}

/// Windows only: Hyper-V/VPN/VirtualBox often look like “the LAN”. Skip those
/// NICs and sweep the real Wi‑Fi/Ethernet (10.x and 192.168 alike).
#[cfg(windows)]
pub fn scan_lan_printers_sync() -> ScanReport {
    let started = Instant::now();
    let (ifaces, iface_err) = windows_list_ifaces();
    let route_ip = local_ipv4();
    let physical: Vec<WinIface> = ifaces
        .iter()
        .filter(|row| !row.virtual_nic)
        .cloned()
        .collect();
    let virtual_nics: Vec<WinIface> = ifaces
        .iter()
        .filter(|row| row.virtual_nic)
        .cloned()
        .collect();
    let targets = windows_scan_targets(&physical, route_ip);
    let used_fallback = targets.is_empty();

    let (hosts, ip_label) = if used_fallback {
        (
            WINDOWS_FALLBACK_PREFIXES
                .iter()
                .flat_map(|prefix| hosts_for_prefix(prefix, None))
                .collect::<Vec<_>>(),
            WINDOWS_FALLBACK_PREFIXES
                .iter()
                .map(|prefix| format!("{prefix}.0/24"))
                .collect::<Vec<_>>()
                .join(","),
        )
    } else {
        let hosts = targets
            .iter()
            .flat_map(|(prefix, skip)| hosts_for_prefix(prefix, *skip))
            .collect::<Vec<_>>();
        let label = targets
            .iter()
            .map(|(prefix, _)| format!("{prefix}.0/24"))
            .collect::<Vec<_>>()
            .join(",");
        (hosts, label)
    };

    let sweep = scan_hosts(hosts);
    let mut extra = vec![
        "os=windows".to_string(),
        format!(
            "route={}",
            route_ip
                .map(|ip| ip.to_string())
                .unwrap_or_else(|| "none".to_string())
        ),
        format!("physical={}", format_ifaces(&physical)),
        format!("virtual={}", format_ifaces(&virtual_nics)),
        windows_empty_reason(
            &sweep,
            &physical,
            used_fallback,
            &ip_label,
            started.elapsed().as_millis(),
        ),
    ];
    if let Some(err) = iface_err {
        extra.insert(2, format!("ifaces_err={err}"));
    }
    report_from_sweep(sweep, started, ip_label, extra)
}

#[cfg(any(windows, test))]
const WINDOWS_MAX_PREFIXES: usize = 4;

/// When Windows only exposes Hyper-V/VPN, still try the usual till nets.
#[cfg(any(windows, test))]
const WINDOWS_FALLBACK_PREFIXES: [&str; 3] = ["10.0.0", "192.168.1", "192.168.0"];

#[cfg(any(windows, test))]
#[derive(Clone)]
struct WinIface {
    name: String,
    ip: Ipv4Addr,
    virtual_nic: bool,
}

#[cfg(any(windows, test))]
fn is_virtual_adapter_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("vethernet")
        || n.contains("hyper-v")
        || n.contains("virtualbox")
        || n.contains("virtual adapter")
        || n.contains("vmware")
        || n.contains("vbox")
        || n.contains("wsl")
        || n.contains("docker")
        || n.contains("tailscale")
        || n.contains("nordlynx")
        || n.contains("wireguard")
        || n.contains("zerotier")
        || n.contains("anyconnect")
        || n.contains("vpn")
        || n.contains("warp")
        || n.contains("hamachi")
        || n.contains("npcap")
        || n.contains("tap-windows")
        || n.contains("wi-fi direct")
        || n.contains("wifi direct")
        || n.contains("bluetooth")
        || n.contains("loopback")
        || n.contains("hosted network")
        || n.contains("pseudo")
}

#[cfg(any(windows, test))]
fn is_rfc1918(ip: Ipv4Addr) -> bool {
    let oct = ip.octets();
    oct[0] == 10
        || (oct[0] == 192 && oct[1] == 168)
        || (oct[0] == 172 && (16..=31).contains(&oct[1]))
}

#[cfg(any(windows, test))]
fn prefix_of(ip: Ipv4Addr) -> String {
    let oct = ip.octets();
    format!("{}.{}.{}", oct[0], oct[1], oct[2])
}

#[cfg(any(windows, test))]
fn format_ifaces(rows: &[WinIface]) -> String {
    if rows.is_empty() {
        return "none".to_string();
    }
    rows.iter()
        .map(|row| format!("{}={}", row.name, row.ip))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(windows)]
fn windows_list_ifaces() -> (Vec<WinIface>, Option<String>) {
    match if_addrs::get_if_addrs() {
        Ok(ifaces) => {
            let rows = ifaces
                .into_iter()
                .filter(|iface| !iface.is_loopback())
                .filter_map(|iface| match iface.addr {
                    if_addrs::IfAddr::V4(v4) if is_usable_v4(v4.ip) && is_rfc1918(v4.ip) => {
                        Some(WinIface {
                            virtual_nic: is_virtual_adapter_name(&iface.name),
                            name: iface.name,
                            ip: v4.ip,
                        })
                    }
                    _ => None,
                })
                .collect();
            (rows, None)
        }
        Err(err) => (Vec::new(), Some(err.to_string())),
    }
}

/// Physical RFC1918 /24s. Prefer the adapter that matches the default route
/// when that route is not a virtual NIC.
#[cfg(any(windows, test))]
fn windows_scan_targets(
    physical: &[WinIface],
    route_ip: Option<Ipv4Addr>,
) -> Vec<(String, Option<u8>)> {
    use std::collections::BTreeMap;

    let route_is_physical = route_ip.is_some_and(|ip| {
        is_usable_v4(ip)
            && is_rfc1918(ip)
            && physical
                .iter()
                .any(|row| prefix_of(row.ip) == prefix_of(ip))
    });
    let route_prefix = route_ip.map(prefix_of);

    let mut by_prefix: BTreeMap<String, (u8, Option<u8>)> = BTreeMap::new();
    for row in physical {
        if row.virtual_nic || !is_usable_v4(row.ip) || !is_rfc1918(row.ip) {
            continue;
        }
        let prefix = prefix_of(row.ip);
        let rank = if route_is_physical && route_prefix.as_deref() == Some(prefix.as_str()) {
            0
        } else {
            1
        };
        by_prefix
            .entry(prefix)
            .or_insert((rank, Some(row.ip.octets()[3])));
    }
    let mut rows: Vec<(u8, String, Option<u8>)> = by_prefix
        .into_iter()
        .map(|(prefix, (rank, skip))| (rank, prefix, skip))
        .collect();
    rows.sort_by_key(|(rank, prefix, _)| (*rank, prefix.clone()));
    rows.into_iter()
        .take(WINDOWS_MAX_PREFIXES)
        .map(|(_, prefix, skip)| (prefix, skip))
        .collect()
}

#[cfg(any(windows, test))]
fn windows_empty_reason(
    sweep: &Sweep,
    physical: &[WinIface],
    used_fallback: bool,
    scan_label: &str,
    elapsed_ms: u128,
) -> String {
    if !sweep.devices.is_empty() {
        return "why=found".to_string();
    }
    if physical.is_empty() {
        return "why=no_wifi_ethernet_ip_tried_fallback".to_string();
    }
    if used_fallback {
        return "why=no_scan_targets_tried_fallback".to_string();
    }
    if sweep.probed == 0 {
        return "why=nothing_probed".to_string();
    }
    // Hyper-V excluded ports can 10013 a handful of binds. Only call it a
    // firewall block when most probes were denied.
    if sweep.blocked > 0 && sweep.blocked * 2 >= sweep.probed {
        return "why=windows_blocked_tcp_9100_firewall".to_string();
    }
    if sweep.probed >= 50
        && elapsed_ms < 1500
        && sweep.refused == 0
        && sweep.blocked == 0
        && sweep.timeout + sweep.other == sweep.probed
    {
        return "why=windows_connect_timeout_not_honored".to_string();
    }
    format!("why=no_open_9100_on_{scan_label}")
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

fn is_private_lan_host(host: &str) -> bool {
    let Ok(ip) = host.parse::<Ipv4Addr>() else {
        return false;
    };
    let oct = ip.octets();
    oct[0] == 10
        || (oct[0] == 192 && oct[1] == 168)
        || (oct[0] == 172 && (16..=31).contains(&oct[1]))
}

pub fn confirm_lan_printer_sync(address: String) -> Result<(), String> {
    let (host, port) = parse_lan_address(&address)?;
    if !is_private_lan_host(&host) {
        return Err("invalid_lan_address".into());
    }
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
    stream.set_nodelay(true).ok();
    stream
        .write_all(&data)
        .map_err(|_| "print_failed".to_string())?;
    stream.flush().map_err(|_| "print_failed".to_string())?;
    let _ = stream.shutdown(std::net::Shutdown::Write);
    // The kick pulse is ~200 ms. Closing too fast can RST before the solenoid fires.
    thread::sleep(Duration::from_millis(250));
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
        assert!(is_private_lan_host("10.0.0.41"));
        assert!(!is_private_lan_host("127.0.0.1"));
        assert!(!is_private_lan_host("localhost"));
        assert!(!is_private_lan_host("8.8.8.8"));
    }

    #[test]
    fn local_ip_sweep_skips_self() {
        let hosts = hosts_for_local_ip(Ipv4Addr::new(10, 0, 0, 39));
        assert_eq!(hosts.len(), 253);
        assert_eq!(hosts[0], Ipv4Addr::new(10, 0, 0, 1));
        assert!(!hosts.contains(&Ipv4Addr::new(10, 0, 0, 39)));
        assert!(hosts.contains(&Ipv4Addr::new(10, 0, 0, 41)));
    }

    #[test]
    fn windows_skips_hyperv_and_uses_wifi() {
        let wifi = WinIface {
            name: "Wi-Fi".into(),
            ip: Ipv4Addr::new(192, 168, 1, 20),
            virtual_nic: false,
        };
        let hyperv = WinIface {
            name: "vEthernet (Default Switch)".into(),
            ip: Ipv4Addr::new(172, 29, 80, 1),
            virtual_nic: true,
        };
        let hyperv_ip = hyperv.ip;
        let targets = windows_scan_targets(&[hyperv, wifi], Some(hyperv_ip));
        assert_eq!(targets[0].0, "192.168.1");
        assert_eq!(targets[0].1, Some(20));
        assert!(targets.len() <= WINDOWS_MAX_PREFIXES);
    }

    #[test]
    fn windows_prefers_venue_10net_over_virtual_192() {
        let wifi = WinIface {
            name: "Wi-Fi".into(),
            ip: Ipv4Addr::new(10, 0, 0, 39),
            virtual_nic: false,
        };
        let vbox = WinIface {
            name: "VirtualBox Host-Only Network".into(),
            ip: Ipv4Addr::new(192, 168, 56, 1),
            virtual_nic: true,
        };
        let vbox_ip = vbox.ip;
        let targets = windows_scan_targets(&[vbox, wifi], Some(vbox_ip));
        assert_eq!(targets[0].0, "10.0.0");
        assert_eq!(targets.len(), 1);
    }

    #[test]
    fn windows_skips_hyperv_default_route_when_wifi_is_10() {
        let wifi = WinIface {
            name: "Wi-Fi".into(),
            ip: Ipv4Addr::new(10, 0, 0, 39),
            virtual_nic: false,
        };
        let targets = windows_scan_targets(&[wifi], Some(Ipv4Addr::new(172, 29, 80, 1)));
        assert_eq!(targets[0].0, "10.0.0");
        assert_eq!(targets[0].1, Some(39));
    }

    #[test]
    fn virtual_adapter_names() {
        assert!(is_virtual_adapter_name("vEthernet (Default Switch)"));
        assert!(is_virtual_adapter_name("VirtualBox Host-Only Network"));
        assert!(is_virtual_adapter_name("Microsoft Wi-Fi Direct Virtual Adapter"));
        assert!(is_virtual_adapter_name("Cloudflare WARP"));
        assert!(!is_virtual_adapter_name("Wi-Fi"));
        assert!(!is_virtual_adapter_name("Ethernet"));
        assert!(!is_virtual_adapter_name("Conexión de área local"));
    }

    #[test]
    fn classifies_windows_socket_errors() {
        assert_eq!(classify_probe_error("os error 10013"), "blocked");
        assert_eq!(
            classify_probe_error("timed out (os error 10060)"),
            "timeout"
        );
        assert_eq!(
            classify_probe_error("connection refused (os error 10061)"),
            "refused"
        );
    }

    #[test]
    fn windows_fallback_includes_venue_10net() {
        assert!(WINDOWS_FALLBACK_PREFIXES.contains(&"10.0.0"));
    }

    #[test]
    fn windows_reason_explains_empty_physical() {
        let sweep = empty_sweep();
        assert_eq!(
            windows_empty_reason(&sweep, &[], true, "10.0.0.0/24", 20_000),
            "why=no_wifi_ethernet_ip_tried_fallback"
        );
        assert_eq!(format_ifaces(&[]), "none");
    }

    #[test]
    fn windows_reason_explains_firewall_block() {
        let mut sweep = empty_sweep();
        sweep.probed = 253;
        sweep.blocked = 200;
        let wifi = WinIface {
            name: "Wi-Fi".into(),
            ip: Ipv4Addr::new(10, 0, 0, 39),
            virtual_nic: false,
        };
        assert_eq!(format_ifaces(&[wifi.clone()]), "Wi-Fi=10.0.0.39");
        assert_eq!(
            windows_empty_reason(&sweep, &[wifi.clone()], false, "10.0.0.0/24", 20_000),
            "why=windows_blocked_tcp_9100_firewall"
        );
        sweep.blocked = 1;
        sweep.timeout = 252;
        assert_eq!(
            windows_empty_reason(&sweep, &[wifi], false, "10.0.0.0/24", 20_000),
            "why=no_open_9100_on_10.0.0.0/24"
        );
    }

    #[test]
    fn windows_reason_explains_fast_timeouts() {
        let mut sweep = empty_sweep();
        sweep.probed = 253;
        sweep.timeout = 253;
        let wifi = WinIface {
            name: "Wi-Fi".into(),
            ip: Ipv4Addr::new(10, 0, 0, 39),
            virtual_nic: false,
        };
        assert_eq!(
            windows_empty_reason(&sweep, &[wifi], false, "10.0.0.0/24", 80),
            "why=windows_connect_timeout_not_honored"
        );
    }
}
