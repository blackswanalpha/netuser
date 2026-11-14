//! Counts devices on the local network by probing every address in the
//! subnet (which makes the kernel send ARP requests) and then reading the
//! kernel neighbour table. Linux only. No root required.

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::net::{Ipv4Addr, UdpSocket};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

/// Largest prefix we sweep. A /16 would be 65k probes, so cap at /22 (1022 hosts).
const MIN_PREFIX: u32 = 22;
/// Probe cadence: several nudges wake power-saving phones, then a wait for the
/// kernel to ARP-resolve newly-seen hosts before we read the table. We filter on
/// a known MAC (not REACHABLE), so present devices count even while still in
/// DELAY/PROBE, and absent ones drop to FAILED and lose their MAC over later scans.
const PROBE_GAP: Duration = Duration::from_millis(900);
const SETTLE: Duration = Duration::from_millis(3500);
const PROBE_ROUNDS: usize = 3;
/// A device seen in a recent scan stays listed (as "away") this long, so a phone
/// that misses one cycle doesn't flicker out of the list.
const GRACE: Duration = Duration::from_secs(180);

#[derive(Serialize, Debug, Clone)]
pub struct Device {
    pub ip: String,
    pub mac: String,
    /// User-chosen name for this device, keyed by MAC. Survives IP changes.
    pub name: Option<String>,
    pub vendor: Option<String>,
    pub private_mac: bool,
    pub is_router: bool,
    pub is_self: bool,
    /// Answered this scan. False = seen recently but quiet now (shown as "away").
    pub online: bool,
    /// Seconds since this device last answered a scan.
    pub last_seen_secs: u64,
}

#[derive(Serialize, Debug)]
pub struct ScanResult {
    pub interface: String,
    pub subnet: String,
    /// The router's IP, for the "open router page" helper.
    pub router_ip: Option<String>,
    /// Devices online now, excluding the router.
    pub count: usize,
    pub devices: Vec<Device>,
}

/// Remembers devices between scans so the list is stable across missed cycles.
#[derive(Clone)]
struct Seen {
    dev: Device,
    last: Instant,
    present: bool,
}
#[derive(Default)]
struct Cache {
    subnet: String,
    map: HashMap<String, Seen>,
}
fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

/// Name and gateway of the interface carrying the default route (lowest metric).
pub(crate) fn default_iface() -> Result<(String, Option<Ipv4Addr>), String> {
    let routes = ip_json(&["-4", "route", "show", "default"])?;
    let route = routes
        .iter()
        .min_by_key(|r| r["metric"].as_u64().unwrap_or(0))
        .ok_or("No default route. Connect to WiFi and try again.")?;
    let iface = route["dev"]
        .as_str()
        .ok_or("Default route has no interface")?
        .to_string();
    let gateway = route["gateway"].as_str().and_then(|s| s.parse().ok());
    Ok((iface, gateway))
}

/// `load_names` is called after probing, right before the merge, so names
/// reflect renames made while the scan was running.
pub fn run(load_names: impl FnOnce() -> HashMap<String, String>) -> Result<ScanResult, String> {
    let (iface, gateway) = default_iface()?;

    let addrs = ip_json(&["-4", "addr", "show", "dev", &iface])?;
    let inet = addrs
        .first()
        .and_then(|a| a["addr_info"].as_array())
        .and_then(|list| list.iter().find(|x| x["family"] == "inet"))
        .ok_or_else(|| format!("{iface} has no IPv4 address"))?;
    let own_ip: Ipv4Addr = inet["local"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("{iface} has an invalid IPv4 address"))?;
    let prefix = (inet["prefixlen"].as_u64().unwrap_or(24) as u32).clamp(MIN_PREFIX, 32);

    let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
    let net = u32::from(own_ip) & mask;
    let targets = hosts(own_ip, net, mask);

    for round in 0..PROBE_ROUNDS {
        probe(own_ip, &targets);
        thread::sleep(if round + 1 < PROBE_ROUNDS { PROBE_GAP } else { SETTLE });
    }

    // Keep every neighbour with a known MAC (REACHABLE/STALE/DELAY/PROBE). After
    // our probes, absent hosts go FAILED and lose their lladdr, so a present MAC
    // is the reliable presence signal — stricter REACHABLE-only misses devices
    // still mid-confirmation at read time (e.g. the router in DELAY).
    let mut devices: Vec<Device> = ip_json(&["-4", "neigh", "show", "dev", &iface])?
        .iter()
        .filter_map(|n| {
            let ip: Ipv4Addr = n["dst"].as_str()?.parse().ok()?;
            let mac = n["lladdr"].as_str()?.to_lowercase();
            (u32::from(ip) & mask == net && ip != own_ip)
                .then(|| device(ip, mac, Some(ip) == gateway, false))
        })
        .collect();

    let own_mac = fs::read_to_string(format!("/sys/class/net/{iface}/address"))
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();
    devices.push(device(own_ip, own_mac, false, true));

    // Merge this cycle into the memory cache so recently-seen devices persist.
    let subnet = format!("{}/{}", Ipv4Addr::from(net), prefix);
    let nicknames = load_names();
    let now = Instant::now();
    // A panic elsewhere must not poison every later scan.
    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    if c.subnet != subnet {
        c.map.clear(); // joined a different network: drop the old one's devices
        c.subnet = subnet.clone();
    }
    for s in c.map.values_mut() {
        s.present = false;
    }
    for mut dev in devices {
        dev.online = true;
        dev.last_seen_secs = 0;
        c.map.insert(dev.mac.clone(), Seen { dev, last: now, present: true });
    }
    c.map.retain(|_, s| now.duration_since(s.last) <= GRACE);

    let mut devices: Vec<Device> = c
        .map
        .values()
        .map(|s| {
            let mut d = s.dev.clone();
            d.online = s.present;
            d.last_seen_secs = now.duration_since(s.last).as_secs();
            d.name = nicknames.get(&d.mac).cloned();
            d
        })
        .collect();
    drop(c);
    devices.sort_by_key(|d| d.ip.parse::<Ipv4Addr>().map(u32::from).unwrap_or(0));

    Ok(ScanResult {
        interface: iface,
        router_ip: gateway.map(|g| g.to_string()),
        count: devices.iter().filter(|d| d.online && !d.is_router).count(),
        devices,
        subnet,
    })
}

fn hosts(own_ip: Ipv4Addr, net: u32, mask: u32) -> Vec<Ipv4Addr> {
    let broadcast = net | !mask;
    if broadcast - net < 2 {
        return Vec::new();
    }
    (net + 1..broadcast)
        .map(Ipv4Addr::from)
        .filter(|ip| *ip != own_ip)
        .collect()
}

/// A UDP datagram to each host forces the kernel to ARP for it (port 9 = discard).
/// Each send pays a per-destination neighbour-table cost, so fan the sweep across
/// threads — serial sends of a /24 take seconds, parallel ones a fraction of that.
fn probe(own_ip: Ipv4Addr, targets: &[Ipv4Addr]) {
    const THREADS: usize = 16;
    if targets.is_empty() {
        return;
    }
    let chunk = targets.len().div_ceil(THREADS).max(1);
    thread::scope(|s| {
        for slice in targets.chunks(chunk) {
            s.spawn(move || {
                if let Ok(sock) = UdpSocket::bind((own_ip, 0)) {
                    for t in slice {
                        let _ = sock.send_to(&[0], (*t, 9));
                    }
                }
            });
        }
    });
}

fn device(ip: Ipv4Addr, mac: String, is_router: bool, is_self: bool) -> Device {
    // Locally administered bit set means a randomized MAC (common on phones).
    let private_mac = mac
        .get(0..2)
        .and_then(|b| u8::from_str_radix(b, 16).ok())
        .is_some_and(|b| b & 0x02 != 0);
    Device {
        ip: ip.to_string(),
        name: None,
        vendor: if private_mac { None } else { vendor(&mac) },
        mac,
        private_mac,
        is_router,
        is_self,
        online: true,
        last_seen_secs: 0,
    }
}

fn vendor(mac: &str) -> Option<String> {
    static OUI: OnceLock<HashMap<String, String>> = OnceLock::new();
    let table = OUI.get_or_init(|| {
        fs::read_to_string("/usr/share/nmap/nmap-mac-prefixes")
            .map(|text| {
                text.lines()
                    .filter(|l| !l.starts_with('#'))
                    .filter_map(|l| l.split_once(' '))
                    .filter(|(p, _)| p.len() == 6)
                    .map(|(p, v)| (p.to_uppercase(), v.trim().to_string()))
                    .collect()
            })
            .unwrap_or_default()
    });
    let key: String = mac.split(':').take(3).collect::<String>().to_uppercase();
    table.get(&key).cloned()
}

pub(crate) fn ip_json(args: &[&str]) -> Result<Vec<Value>, String> {
    let out = Command::new("ip")
        .arg("-j")
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run `ip`: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    if out.stdout.iter().all(u8::is_ascii_whitespace) {
        return Ok(Vec::new());
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("Unexpected `ip` output: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_range_excludes_network_broadcast_and_self() {
        let own: Ipv4Addr = "192.168.1.10".parse().unwrap();
        let mask = u32::MAX << 8;
        let net = u32::from(own) & mask;
        let h = hosts(own, net, mask);
        assert_eq!(h.len(), 253);
        assert_eq!(h.first().unwrap().to_string(), "192.168.1.1");
        assert_eq!(h.last().unwrap().to_string(), "192.168.1.254");
        assert!(!h.contains(&own));
    }

    #[test]
    fn detects_private_mac() {
        assert!(device("10.0.0.2".parse().unwrap(), "3a:a6:02:49:fe:a8".into(), false, false).private_mac);
        assert!(!device("10.0.0.2".parse().unwrap(), "c0:7e:40:be:97:96".into(), false, false).private_mac);
    }

    /// Real scan of the current network: `cargo test -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_scan() {
        let r = run(HashMap::new).expect("scan failed");
        println!("{}", serde_json::to_string_pretty(&r).unwrap());
        assert!(r.count >= 1);
    }
}
