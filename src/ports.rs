//! Port to traffic-type classification (Traffic Insights, Task 1).
//!
//! Maps a connection's ports to a short traffic-type id such as `https`.
//! The ids are written into stored history, so an existing id must never be
//! renamed or reused for a different service.

use std::cmp::Reverse;

/// Transport a table row applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pr {
    Tcp,
    Udp,
    Both,
}

/// Display group of a traffic type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cat {
    Web,
    Names,
    Files,
    Mail,
    Remote,
    Media,
    Games,
    System,
    Other,
}

/// One row of the port table: an inclusive port range on a transport.
#[derive(Clone, Copy, Debug)]
pub struct PortType {
    pub lo: u16,
    pub hi: u16,
    pub proto: Pr,
    pub id: &'static str,
    pub label: &'static str,
    pub cat: Cat,
}

/// A TCP connection whose ports match no table row.
pub const OTHER_TCP: &str = "other_tcp";
/// Bytes no connection accounts for (UDP, ICMP, system).
pub const UNATTRIBUTED: &str = "unattributed";
/// UI-only residual for history recorded without flows. Never stored.
pub const UNCLASSIFIED: &str = "__unclassified";

/// First port of the Windows ephemeral (client-side) range.
const EPHEMERAL_START: u16 = 49152;

/// Traffic-type id for a connection.
///
/// The service port is `local_port` when `inbound`, otherwise `remote_port`.
/// If it has no row, the other port is tried when it is below 49152.
/// Anything else is [`OTHER_TCP`].
pub fn classify(local_port: u16, remote_port: u16, inbound: bool, tcp: bool) -> &'static str {
    classify_in(TABLE, local_port, remote_port, inbound, tcp)
}

/// [`classify`] against any table, so precedence can be tested in isolation.
fn classify_in(
    table: &[PortType],
    local_port: u16,
    remote_port: u16,
    inbound: bool,
    tcp: bool,
) -> &'static str {
    let (service, other) = if inbound {
        (local_port, remote_port)
    } else {
        (remote_port, local_port)
    };
    find(table, service, tcp)
        .or_else(|| (other < EPHEMERAL_START).then(|| find(table, other, tcp)).flatten())
        .map_or(OTHER_TCP, |r| r.id)
}

/// Display label for a stored or sentinel id. An unknown id is returned as-is.
pub fn label(id: &str) -> String {
    match id {
        OTHER_TCP => "Other TCP".to_string(),
        UNATTRIBUTED => "Other traffic (UDP, ICMP, system)".to_string(),
        UNCLASSIFIED => "Not classified".to_string(),
        _ => by_id(id).map_or_else(|| id.to_string(), |r| r.label.to_string()),
    }
}

/// Display group for an id. Unknown and sentinel ids are [`Cat::Other`].
pub fn category(id: &str) -> Cat {
    by_id(id).map_or(Cat::Other, |r| r.cat)
}

fn by_id(id: &str) -> Option<&'static PortType> {
    TABLE.iter().find(|r| r.id == id)
}

fn covers(r: &PortType, port: u16, tcp: bool) -> bool {
    let proto_ok = match r.proto {
        Pr::Both => true,
        Pr::Tcp => tcp,
        Pr::Udp => !tcp,
    };
    proto_ok && r.lo <= port && port <= r.hi
}

/// Specificity of a row; higher wins. A proto-specific row beats `Both`
/// first, then a narrower range beats a wider one (a single port is narrowest).
fn rank(r: &PortType) -> (bool, Reverse<u16>) {
    (r.proto != Pr::Both, Reverse(r.hi.saturating_sub(r.lo)))
}

fn find(table: &[PortType], port: u16, tcp: bool) -> Option<&PortType> {
    table
        .iter()
        .filter(|r| covers(r, port, tcp))
        .max_by_key(|r| rank(r))
}

const fn r(
    lo: u16,
    hi: u16,
    proto: Pr,
    id: &'static str,
    label: &'static str,
    cat: Cat,
) -> PortType {
    PortType { lo, hi, proto, id, label, cat }
}

/// The port table. Self-written, with IANA assignments as the reference.
/// No `Tcp` or `Both` row may reach 49152 or above (checked by `table_valid`).
/// Deliberate overlap: TCP 27017 (MongoDB) sits inside the Steam games range
/// and wins as the more specific row.
#[rustfmt::skip]
const TABLE: &[PortType] = {
    use Cat::*;
    use Pr::*;
    &[
        // Web
        r(80, 80, Tcp, "http", "HTTP", Web),
        r(443, 443, Tcp, "https", "HTTPS", Web),
        r(443, 443, Udp, "quic", "QUIC", Web),
        r(8080, 8080, Tcp, "http_alt", "HTTP 8080", Web),
        r(8443, 8443, Tcp, "https_alt", "HTTPS 8443", Web),
        r(8000, 8000, Tcp, "http_8000", "HTTP 8000", Web),
        r(8888, 8888, Tcp, "http_8888", "HTTP 8888", Web),
        r(3128, 3128, Tcp, "web_proxy", "Web proxy", Web),
        r(1080, 1080, Tcp, "socks", "SOCKS", Web),
        r(9001, 9001, Tcp, "tor", "Tor", Web),

        // Names and discovery
        r(53, 53, Both, "dns", "DNS", Names),
        r(853, 853, Both, "dns_tls", "DNS over TLS", Names),
        r(5353, 5353, Udp, "mdns", "mDNS", Names),
        r(5355, 5355, Both, "llmnr", "LLMNR", Names),
        r(137, 137, Udp, "netbios_ns", "NetBIOS names", Names),
        r(138, 138, Udp, "netbios_dgm", "NetBIOS datagram", Names),
        r(1900, 1900, Udp, "ssdp", "SSDP", Names),
        r(2869, 2869, Tcp, "upnp_events", "UPnP events", Names),
        r(3702, 3702, Udp, "ws_discovery", "WS-Discovery", Names),
        r(5357, 5357, Tcp, "wsd", "WSD", Names),
        r(67, 68, Udp, "dhcp", "DHCP", Names),
        r(546, 547, Udp, "dhcpv6", "DHCPv6", Names),

        // Windows and directory
        r(88, 88, Both, "kerberos", "Kerberos", System),
        r(464, 464, Both, "kpasswd", "Kerberos password", System),
        r(389, 389, Both, "ldap", "LDAP", System),
        r(636, 636, Tcp, "ldaps", "LDAPS", System),
        r(3268, 3268, Tcp, "global_catalog", "Global catalog", System),
        r(135, 135, Tcp, "msrpc", "RPC", System),
        r(1688, 1688, Tcp, "kms", "KMS", System),
        r(8530, 8531, Tcp, "wsus", "WSUS", System),
        r(7680, 7680, Tcp, "delivery_opt", "Delivery Optimization", System),
        r(3544, 3544, Udp, "teredo", "Teredo", System),

        // Files and printing
        r(445, 445, Tcp, "smb", "SMB", Files),
        r(139, 139, Tcp, "netbios_ssn", "NetBIOS session", Files),
        r(20, 21, Tcp, "ftp", "FTP", Files),
        r(989, 990, Tcp, "ftps", "FTPS", Files),
        r(69, 69, Udp, "tftp", "TFTP", Files),
        r(2049, 2049, Both, "nfs", "NFS", Files),
        r(873, 873, Tcp, "rsync", "rsync", Files),
        r(631, 631, Tcp, "ipp", "IPP", Files),
        r(9100, 9100, Tcp, "raw_print", "Raw printing", Files),
        r(6881, 6889, Both, "bittorrent", "BitTorrent", Files),
        r(22000, 22000, Both, "syncthing", "Syncthing", Files),
        r(21027, 21027, Udp, "syncthing_lan", "Syncthing discovery", Files),
        r(17500, 17500, Both, "dropbox_lan", "Dropbox LAN sync", Files),
        r(9418, 9418, Tcp, "git", "Git", Files),

        // Mail and messaging
        r(25, 25, Tcp, "smtp", "SMTP", Mail),
        r(465, 465, Tcp, "smtps", "SMTPS", Mail),
        r(587, 587, Tcp, "mail_submit", "Mail submit", Mail),
        r(110, 110, Tcp, "pop3", "POP3", Mail),
        r(995, 995, Tcp, "pop3s", "POP3S", Mail),
        r(143, 143, Tcp, "imap", "IMAP", Mail),
        r(993, 993, Tcp, "imaps", "IMAPS", Mail),
        r(5222, 5222, Tcp, "xmpp", "XMPP", Mail),
        r(6665, 6669, Tcp, "irc", "IRC", Mail),
        r(6697, 6697, Tcp, "ircs", "IRC TLS", Mail),

        // Remote access and VPN
        r(22, 22, Tcp, "ssh", "SSH", Remote),
        r(23, 23, Tcp, "telnet", "Telnet", Remote),
        r(3389, 3389, Both, "rdp", "RDP", Remote),
        r(5900, 5909, Tcp, "vnc", "VNC", Remote),
        r(5938, 5938, Both, "teamviewer", "TeamViewer", Remote),
        r(7070, 7070, Tcp, "anydesk", "AnyDesk", Remote),
        r(21115, 21119, Both, "rustdesk", "RustDesk", Remote),
        r(3283, 3283, Both, "apple_remote", "ARD", Remote),
        r(1494, 1494, Tcp, "citrix_ica", "Citrix ICA", Remote),
        r(5985, 5985, Tcp, "winrm", "WinRM", Remote),
        r(5986, 5986, Tcp, "winrm_tls", "WinRM TLS", Remote),
        r(1194, 1194, Both, "openvpn", "OpenVPN", Remote),
        r(1723, 1723, Tcp, "pptp", "PPTP", Remote),
        r(1701, 1701, Udp, "l2tp", "L2TP", Remote),
        r(500, 500, Udp, "ike", "IKE", Remote),
        r(4500, 4500, Udp, "ipsec_nat", "IPsec NAT-T", Remote),
        r(51820, 51820, Udp, "wireguard", "WireGuard", Remote),
        r(41641, 41641, Udp, "tailscale", "Tailscale", Remote),

        // Media, calls and streaming
        r(554, 554, Both, "rtsp", "RTSP", Media),
        r(1935, 1935, Tcp, "rtmp", "RTMP", Media),
        r(5004, 5005, Udp, "rtp", "RTP", Media),
        r(3478, 3478, Both, "stun", "STUN/TURN", Media),
        r(5349, 5349, Tcp, "turn_tls", "TURN TLS", Media),
        r(5060, 5060, Both, "sip", "SIP", Media),
        r(5061, 5061, Tcp, "sip_tls", "SIP TLS", Media),
        r(8801, 8802, Tcp, "zoom", "Zoom", Media),
        r(8801, 8810, Udp, "zoom_media", "Zoom media", Media),
        r(50000, 50059, Udp, "teams_media", "Teams media", Media),
        r(19302, 19309, Udp, "google_meet", "Google Meet", Media),
        r(32400, 32400, Tcp, "plex", "Plex", Media),
        r(8096, 8096, Tcp, "jellyfin", "Jellyfin", Media),
        r(8009, 8009, Tcp, "cast", "Cast", Media),
        r(7000, 7000, Tcp, "airplay", "AirPlay", Media),
        r(4070, 4070, Tcp, "spotify", "Spotify Connect", Media),

        // Games and launchers
        r(27000, 27014, Udp, "steam_client", "Steam client", Games),
        r(27015, 27030, Both, "steam_games", "Steam games", Games),
        r(27031, 27037, Both, "steam_lan", "Steam LAN", Games),
        r(3074, 3074, Both, "xbox_live", "Xbox network", Games),
        r(9295, 9304, Both, "ps_remote", "PS Remote Play", Games),
        r(3659, 3659, Both, "ea_online", "EA online", Games),
        r(1119, 1119, Tcp, "battlenet", "Battle.net", Games),
        r(3724, 3724, Both, "blizzard_games", "Blizzard games", Games),
        r(25565, 25565, Tcp, "minecraft", "Minecraft", Games),
        r(19132, 19133, Udp, "minecraft_bedrock", "Minecraft Bedrock", Games),
        r(7777, 7778, Both, "game_7777", "Game server 7777", Games),
        r(47984, 48010, Both, "gamestream", "GameStream", Games),
        r(9987, 9987, Udp, "teamspeak", "TeamSpeak", Games),

        // System and updates
        r(123, 123, Udp, "ntp", "NTP", System),
        r(161, 162, Udp, "snmp", "SNMP", System),
        r(514, 514, Udp, "syslog", "Syslog", System),
        r(5223, 5223, Tcp, "apple_push", "Apple push", System),
        r(5228, 5230, Tcp, "google_push", "Google push", System),
        r(1883, 1883, Tcp, "mqtt", "MQTT", System),
        r(8883, 8883, Tcp, "mqtt_tls", "MQTT TLS", System),
        r(1812, 1813, Udp, "radius", "RADIUS", System),
        r(2375, 2376, Tcp, "docker", "Docker API", System),

        // Databases and peer networks
        r(1433, 1433, Tcp, "mssql", "SQL Server", Other),
        r(3306, 3306, Tcp, "mysql", "MySQL", Other),
        r(5432, 5432, Tcp, "postgres", "PostgreSQL", Other),
        r(6379, 6379, Tcp, "redis", "Redis", Other),
        r(27017, 27017, Tcp, "mongodb", "MongoDB", Other),
        r(8333, 8333, Tcp, "bitcoin", "Bitcoin", Other),
    ]
};

#[cfg(test)]
mod tests {
    use super::*;

    const fn t(lo: u16, hi: u16, proto: Pr, id: &'static str) -> PortType {
        PortType { lo, hi, proto, id, label: id, cat: Cat::Other }
    }

    #[test]
    fn https_outbound() {
        assert_eq!(classify(50123, 443, false, true), "https");
    }

    #[test]
    fn rdp_inbound() {
        assert_eq!(classify(3389, 51000, true, true), "rdp");
    }

    #[test]
    fn fallback_to_other_port() {
        let row = TABLE
            .iter()
            .find(|r| r.lo == 8080 && r.hi == 8080 && r.proto != Pr::Udp)
            .expect("8080 row");
        assert_eq!(classify(8080, 50999, false, true), row.id);
        assert_eq!(row.id, "http_alt");
    }

    #[test]
    fn service_port_beats_other_port() {
        // Outbound: the remote port is the service port, even when the local
        // port also has a row.
        assert_eq!(classify(8080, 443, false, true), "https");
        // Inbound on an ephemeral local port falls back to the remote port.
        assert_eq!(classify(50123, 443, true, true), "https");
        // The fallback port must be below 49152.
        assert_eq!(classify(50999, 51820, true, true), OTHER_TCP);
    }

    #[test]
    fn unknown_is_other_tcp() {
        assert_eq!(classify(50001, 50002, false, true), OTHER_TCP);
    }

    #[test]
    fn precedence_specific_beats_range_and_both() {
        let table = [
            t(8000, 8100, Pr::Both, "range_both"),
            t(8000, 8100, Pr::Tcp, "range_tcp"),
            t(8080, 8080, Pr::Both, "single_both"),
            t(8080, 8080, Pr::Tcp, "single_tcp"),
            t(8080, 8080, Pr::Udp, "single_udp"),
        ];
        let mut reversed = table;
        reversed.reverse();
        for tbl in [&table[..], &reversed[..]] {
            // Single port and proto-specific wins over the range and Both rows.
            assert_eq!(classify_in(tbl, 50000, 8080, false, true), "single_tcp");
            assert_eq!(classify_in(tbl, 8080, 50000, true, false), "single_udp");
            // Inside the range only: the proto-specific range beats the Both range.
            assert_eq!(classify_in(tbl, 50000, 8050, false, true), "range_tcp");
            // A row for the other transport never matches.
            assert_eq!(classify_in(tbl, 50000, 8050, false, false), "range_both");
            // Outside every row.
            assert_eq!(classify_in(tbl, 50000, 9000, false, true), OTHER_TCP);
        }
        // Without a proto-specific single row, the Both single port beats both ranges.
        let no_specific = [
            t(8000, 8100, Pr::Tcp, "range_tcp"),
            t(8080, 8080, Pr::Both, "single_both"),
            t(8000, 8100, Pr::Both, "range_both"),
        ];
        assert_eq!(classify_in(&no_specific, 50000, 8080, false, false), "single_both");
    }

    #[test]
    fn labels_for_sentinels() {
        assert_eq!(label(OTHER_TCP), "Other TCP");
        assert_eq!(label(UNATTRIBUTED), "Other traffic (UDP, ICMP, system)");
        assert_eq!(label(UNCLASSIFIED), "Not classified");
        assert_eq!(label("https"), "HTTPS");
        assert_eq!(label("no_such_type"), "no_such_type");
    }

    #[test]
    fn category_unknown_is_other() {
        assert_eq!(category("no_such_type"), Cat::Other);
        assert_eq!(category(OTHER_TCP), Cat::Other);
        assert_eq!(category(UNATTRIBUTED), Cat::Other);
        assert_eq!(category(UNCLASSIFIED), Cat::Other);
        assert_eq!(category("https"), Cat::Web);
        assert_eq!(category("dns"), Cat::Names);
    }

    #[test]
    fn table_valid() {
        let mut seen = std::collections::HashSet::new();
        for r in TABLE {
            assert!(seen.insert(r.id), "duplicate id {}", r.id);
            assert!(r.lo <= r.hi, "{}: lo > hi", r.id);
            if r.proto != Pr::Udp {
                assert!(r.hi < 49152, "{}: TCP row reaches the ephemeral range", r.id);
            }
            assert!(
                ![OTHER_TCP, UNATTRIBUTED, UNCLASSIFIED].contains(&r.id),
                "{}: id collides with a sentinel",
                r.id
            );
            assert!(
                !r.id.is_empty()
                    && r.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
                "{}: id must be lowercase snake_case",
                r.id
            );
            assert!(!r.label.is_empty(), "{}: empty label", r.id);
        }
    }

    #[test]
    fn table_unambiguous() {
        // For every port and transport, exactly one matching row has the best rank,
        // so the result never depends on table order.
        for tcp in [true, false] {
            for port in 0..=u16::MAX {
                let ranks: Vec<_> = TABLE
                    .iter()
                    .filter(|r| covers(r, port, tcp))
                    .map(rank)
                    .collect();
                if let Some(best) = ranks.iter().max() {
                    let n = ranks.iter().filter(|k| *k == best).count();
                    assert_eq!(n, 1, "port {port} tcp={tcp}: {n} rows tie");
                }
            }
        }
    }
}
