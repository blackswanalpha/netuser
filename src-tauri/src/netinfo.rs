//! WiFi link quality and live throughput for the active interface. Linux only.
//! Sources: `/proc/net/wireless` (dBm, quality), `nmcli` (SSID, rate, band),
//! and `/sys/class/net/<iface>/statistics` byte counters.

use serde::Serialize;
use std::fs;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use crate::scan;

/// Window used to measure current throughput from the byte counters.
const SAMPLE: Duration = Duration::from_millis(1000);

#[derive(Serialize, Debug, Default)]
pub struct NetStats {
    pub interface: String,
    pub ssid: Option<String>,
    /// Received signal strength, dBm (negative; closer to 0 is stronger).
    pub signal_dbm: Option<i32>,
    /// Signal as a 0-100 percentage (from nmcli, else derived from dBm).
    pub signal_pct: Option<u32>,
    /// Qualitative label: Excellent / Good / Fair / Weak / Very weak.
    pub quality: String,
    /// Negotiated link rate in Mbit/s.
    pub link_mbps: Option<f64>,
    pub freq_mhz: Option<u32>,
    pub band: Option<String>,
    pub channel: Option<String>,
    /// Live throughput over the sample window, bytes/second.
    pub rx_bps: u64,
    pub tx_bps: u64,
    /// Total bytes since the interface came up.
    pub rx_total: u64,
    pub tx_total: u64,
}

pub fn run() -> Result<NetStats, String> {
    let (iface, _) = scan::default_iface()?;
    let mut s = NetStats {
        interface: iface.clone(),
        ..Default::default()
    };

    // Throughput: two counter reads, SAMPLE apart.
    let (rx0, tx0) = counters(&iface)?;
    let t0 = Instant::now();
    thread::sleep(SAMPLE);
    let (rx1, tx1) = counters(&iface)?;
    let secs = t0.elapsed().as_secs_f64().max(0.001);
    s.rx_total = rx1;
    s.tx_total = tx1;
    s.rx_bps = ((rx1.saturating_sub(rx0)) as f64 / secs) as u64;
    s.tx_bps = ((tx1.saturating_sub(tx0)) as f64 / secs) as u64;

    if let Some(dbm) = signal_dbm(&iface) {
        s.signal_dbm = Some(dbm);
        s.signal_pct = Some(dbm_to_pct(dbm));
        s.quality = quality_label(dbm).into();
    }

    // nmcli enriches with SSID, exact rate, band — overrides derived signal_pct.
    if let Some(w) = nmcli_active(&iface) {
        if w.ssid.is_some() { s.ssid = w.ssid; }
        if let Some(p) = w.signal_pct { s.signal_pct = Some(p); }
        s.link_mbps = w.link_mbps;
        s.freq_mhz = w.freq_mhz;
        s.channel = w.channel;
        s.band = w.freq_mhz.map(band_of);
        if s.signal_dbm.is_none() {
            if let Some(p) = w.signal_pct {
                s.quality = quality_label(pct_to_dbm(p)).into();
            }
        }
    }

    Ok(s)
}

fn counters(iface: &str) -> Result<(u64, u64), String> {
    let read = |k: &str| -> Result<u64, String> {
        fs::read_to_string(format!("/sys/class/net/{iface}/statistics/{k}"))
            .map_err(|e| format!("Can't read {k}: {e}"))?
            .trim()
            .parse()
            .map_err(|_| format!("Bad {k} counter"))
    };
    Ok((read("rx_bytes")?, read("tx_bytes")?))
}

/// Signal level in dBm from `/proc/net/wireless` for the given interface.
fn signal_dbm(iface: &str) -> Option<i32> {
    let text = fs::read_to_string("/proc/net/wireless").ok()?;
    for line in text.lines() {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix(iface).and_then(|r| r.strip_prefix(':')) {
            // columns: status quality link-qual level noise ...
            let cols: Vec<&str> = rest.split_whitespace().collect();
            // level is the 3rd numeric column (index 2), e.g. "-61."
            let lvl = cols.get(2)?.trim_end_matches('.');
            let dbm: f64 = lvl.parse().ok()?;
            return Some(dbm as i32);
        }
    }
    None
}

struct Wifi {
    ssid: Option<String>,
    signal_pct: Option<u32>,
    link_mbps: Option<f64>,
    freq_mhz: Option<u32>,
    channel: Option<String>,
}

/// The active WiFi connection row for `iface` from nmcli, or None if nmcli is
/// absent or `iface` isn't a WiFi device (e.g. the default route is ethernet).
fn nmcli_active(iface: &str) -> Option<Wifi> {
    // `list --rescan no` reads the cached AP list instead of forcing a ~6s scan.
    // Scoped to `iface` so a second adapter can't report its own AP.
    let out = Command::new("nmcli")
        .args([
            "-t", "-f", "IN-USE,SSID,SIGNAL,RATE,FREQ,CHAN",
            "dev", "wifi", "list", "ifname", iface, "--rescan", "no",
        ])
        .env("LC_ALL", "C")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let f = split_terse(line);
        if f.first().map(String::as_str) != Some("*") {
            continue;
        }
        let nonempty = |s: &String| (!s.is_empty()).then(|| s.clone());
        return Some(Wifi {
            ssid: f.get(1).and_then(nonempty),
            signal_pct: f.get(2).and_then(|s| s.parse().ok()),
            // RATE looks like "130 Mbit/s"
            link_mbps: f.get(3).and_then(|s| s.split_whitespace().next()?.parse().ok()),
            freq_mhz: f.get(4).and_then(|s| s.split_whitespace().next()?.parse().ok()),
            channel: f.get(5).and_then(nonempty),
        });
    }
    None
}

/// Splits an nmcli terse line on unescaped ':' and unescapes '\:' and '\\'.
fn split_terse(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ':' => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

fn band_of(mhz: u32) -> String {
    match mhz {
        0..=2499 => "2.4 GHz",
        2500..=5924 => "5 GHz",
        _ => "6 GHz",
    }
    .into()
}

fn quality_label(dbm: i32) -> &'static str {
    match dbm {
        d if d >= -50 => "Excellent",
        d if d >= -60 => "Very good",
        d if d >= -67 => "Good",
        d if d >= -75 => "Fair",
        d if d >= -85 => "Weak",
        _ => "Very weak",
    }
}

/// Maps dBm to a rough 0-100 strength, clamped. −50 dBm ≈ 100, −90 dBm ≈ 0.
fn dbm_to_pct(dbm: i32) -> u32 {
    (2 * (dbm + 100)).clamp(0, 100) as u32
}

fn pct_to_dbm(pct: u32) -> i32 {
    (pct as i32) / 2 - 100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terse_handles_escaped_colons() {
        let f = split_terse(r"*:My\:Net:59:130 Mbit/s:2412 MHz:1");
        assert_eq!(f[0], "*");
        assert_eq!(f[1], "My:Net");
        assert_eq!(f[3], "130 Mbit/s");
    }

    #[test]
    fn quality_and_pct() {
        assert_eq!(quality_label(-61), "Good");
        assert_eq!(dbm_to_pct(-61), 78);
        assert_eq!(band_of(2412), "2.4 GHz");
        assert_eq!(band_of(5200), "5 GHz");
    }

    #[test]
    #[ignore]
    fn live() {
        let s = run().expect("net stats");
        println!("{}", serde_json::to_string_pretty(&s).unwrap());
    }
}
