# Vantage

A native Windows network monitor and firewall written in Rust with egui, modeled on GlassWire.

- **Traffic Monitor**: live and historical bandwidth graphs (5 min to 1 month), usage bars, top hosts, and per-app totals.
- **Firewall Rules**: allow or block each app's incoming and outgoing traffic with one click. Blocks are persistent Windows Filtering Platform filters, so they work alongside Windows Firewall or third-party firewalls such as Kaspersky.
- **Activity Log**: first network activity per app, internet drops and reconnects, app version changes, new LAN devices, and data-cap alerts.
- **Network Scanner**: an ARP sweep of the physical LAN, with vendor lookup from Wireshark's `manuf` database.
- **Hardware Resources**: per-app CPU, GPU, memory, and disk, plus 5-minute system charts.

Run it **as administrator** for per-app and per-host byte counts (TCP EStats) and silent firewall changes. Without admin rights it shows total traffic, apps by connection count, and asks for UAC on each firewall change.

Data is stored in `%APPDATA%\Vantage`.

## Build

You need Rust with the `x86_64-pc-windows-gnu` toolchain and MinGW-w64 (WinLibs) on `PATH`.

```bash
cargo build --release
```

The binary is written to `target/release/vantage.exe`.
