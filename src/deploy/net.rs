use std::net::{IpAddr, Ipv4Addr, UdpSocket};

/// Best-effort LAN IPv4 addresses (excludes loopback and link-local).
pub fn lan_ipv4s() -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = sock.local_addr() {
                if let IpAddr::V4(v4) = addr.ip() {
                    if !v4.is_loopback() && !v4.is_link_local() {
                        out.push(v4);
                    }
                }
            }
        }
    }
    // Supplement from OS tools when available.
    for extra in parse_ifconfig_ipv4() {
        if !out.contains(&extra) {
            out.push(extra);
        }
    }
    out
}

fn parse_ifconfig_ipv4() -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    let cmds: &[(&str, &[&str])] = if cfg!(target_os = "linux") {
        &[("hostname", &["-I"]), ("ip", &["-4", "-o", "addr", "show"])]
    } else {
        &[("ifconfig", &[])]
    };
    for (bin, args) in cmds {
        let Ok(output) = std::process::Command::new(bin).args(*args).output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        for token in text.split(|c: char| !c.is_ascii_digit() && c != '.') {
            if let Ok(ip) = token.parse::<Ipv4Addr>() {
                if !ip.is_loopback() && !ip.is_unspecified() && !ip.is_link_local() && !out.contains(&ip) {
                    out.push(ip);
                }
            }
        }
        if !out.is_empty() {
            break;
        }
    }
    out
}

/// Public IPv4 via a short HTTPS lookup (optional; fails offline).
pub fn public_ipv4() -> Option<Ipv4Addr> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .ok()?;
    for url in [
        "https://api.ipify.org",
        "https://ifconfig.me/ip",
        "https://icanhazip.com",
    ] {
        let Ok(resp) = client.get(url).send() else {
            continue;
        };
        let Ok(text) = resp.text() else {
            continue;
        };
        let trimmed = text.trim();
        if let Ok(ip) = trimmed.parse::<Ipv4Addr>() {
            return Some(ip);
        }
    }
    None
}

pub fn firewall_hints(port: u16) -> Vec<String> {
    let mut hints = Vec::new();
    if cfg!(target_os = "macos") {
        hints.push(format!(
            "macOS: System Settings → Network → Firewall → Options, allow inbound TCP {port} (or turn the firewall off briefly to test)."
        ));
    } else if cfg!(target_os = "windows") {
        hints.push(format!(
            "Windows: allow inbound TCP {port} in Windows Defender Firewall (Advanced settings → Inbound Rules)."
        ));
    } else {
        hints.push(format!(
            "Linux: `sudo ufw allow {port}/tcp && sudo ufw reload` (or open the port in firewalld / your cloud security group)."
        ));
    }
    hints.push(
        "Home routers usually need a port-forward from the public IP to this machine for true internet access.".into(),
    );
    hints
}
