# NetEye on Linux and macOS: port plan

Status: plan only, nothing built yet (2026-10-27).

## Where NetEye stands

The UI (egui/eframe, every page in `src/ui/`), the data model and history (`src/store.rs`),
aggregation (`src/ui/traffic_data.rs`), the globe, flags, CSV/PNG export and the country
database (`src/geo.rs`) are plain Rust and already portable.

Everything that talks to the operating system is Windows-only:

| Module | Windows API today | Linux | macOS |
|---|---|---|---|
| `net.rs` connections + owning process | IP Helper `GetExtendedTcpTable` / `GetExtendedUdpTable` | `/proc/net/{tcp,tcp6,udp,udp6}` + inode → pid via `/proc/<pid>/fd` (or `netlink` `sock_diag`) | `libproc` `proc_pidfdinfo` (`PROC_PIDFDSOCKETINFO`) per process |
| `net.rs` per-connection bytes | TCP EStats (admin) | eBPF on `tcp_sendmsg`/`tcp_cleanup_rbuf` (root or `CAP_BPF`), or `sock_diag` `tcp_info` byte counters | `nettop`-style data via `NetworkStatistics` (private) or `libproc` `proc_pid_rusage` totals per process |
| `net.rs` adapters + counters | `GetIfTable2` | `/sys/class/net/*/statistics`, `getifaddrs` | `getifaddrs` + `AF_LINK` `if_data` |
| `wfp.rs` per-app block | Windows Filtering Platform, persistent filters | nftables with `meta cgroup` / `socket cgroupv2` matches; apps launched into per-app cgroups via systemd scopes. Root needed | Network Extension content filter (`NEFilterDataProvider`): needs Apple Developer account, the Network Extension entitlement, signing and notarization |
| `system.rs` LAN scan | `SendARP`, IP Helper | `/proc/net/arp` + ARP ping over a raw socket (`CAP_NET_RAW`) | `sysctl` `NET_RT_FLAGS` ARP table + ICMP/ARP sweep |
| `system.rs` DNS, proxy, hosts, RDP, startup | registry, `GetNetworkParams` | `/etc/resolv.conf` or `resolvectl`, env/GNOME proxy, `/etc/hosts`, SSH/VNC listeners, XDG autostart `.desktop` | `scutil --dns`, `SCDynamicStore` proxies, `/etc/hosts`, Screen Sharing/SSH, LaunchAgent plist |
| `sign.rs` signatures | WinTrust + catalogs | package ownership (`dpkg -S` / `rpm -qf`) instead of signatures | `SecStaticCodeCheckValidity` (Security.framework) |
| camera/mic monitor | CapabilityAccessManager registry | `/proc/*/fd` holding `/dev/video*`, PipeWire/PulseAudio clients | TCC is private; `AVCaptureDevice.isInUseByAnotherApplication` for the camera, CoreAudio `kAudioDevicePropertyDeviceIsRunningSomewhere` for the mic |
| `hw.rs` CPU/GPU/memory/disk | PDH, PSAPI, toolhelp | `/proc/stat`, `/proc/<pid>/{stat,io,status}`, GPU via vendor tools (optional) | `host_statistics64`, `proc_pid_rusage`, IOKit |
| app icons + version | Shell `SHGetFileInfo`, version resources | `.desktop` files + icon theme lookup | `NSWorkspace iconForFile`, `Info.plist` |
| tray, notifications | tray-icon, notify-rust | tray-icon (AppIndicator/libappindicator), notify-rust (D-Bus) | tray-icon (NSStatusItem), notify-rust |
| Event Log mirror | `ReportEventW` | journald (`sd_journal_send`) | `os_log` |
| install | Inno Setup | `.deb`, `.rpm`, AppImage; a polkit rule or systemd service for the privileged part | `.dmg` with a signed, notarized `.app`; the firewall part as a system extension |

## Shape of the port

1. **Split the platform layer.** Move today's Windows code behind a `platform` module with
   one trait per concern (`Connections`, `Firewall`, `LanScan`, `Monitors`, `Hardware`,
   `AppInfo`) and `#[cfg(target_os = ...)]` implementations. Windows keeps working unchanged;
   this step alone is mostly moving code. `cargo build` for Linux and macOS starts passing with
   stub implementations that return "not available".
2. **Privileged helper.** On Linux and macOS, packet-level work needs root, and running a GUI as
   root is wrong there. Split a small helper (systemd service / launchd daemon) that does
   connections, bytes and blocking, and talks to the UI over a local socket. Windows can keep
   the current single process.
3. **Linux, monitoring first:** connections and processes from `/proc`, adapter counters, LAN
   scan, hosts/DNS monitors, hardware, icons from `.desktop` files. Per-app bytes start
   approximate (`tcp_info`), then eBPF. Ship as AppImage + `.deb`.
4. **Linux firewall:** nftables + cgroup v2 per app. Hardest part: putting already-running apps
   into a cgroup (move their pids) and new ones (systemd scope wrapper or a cgroup watcher).
5. **macOS, monitoring:** libproc connections, `getifaddrs`, scan, monitors, signatures through
   Security.framework. Ship a signed, notarized `.app` in a `.dmg`.
6. **macOS firewall:** Network Extension content filter. Needs a paid Apple Developer account
   and Apple's approval of the entitlement; without it macOS NetEye stays monitor-only.

## What each step needs from outside

- A Linux machine or VM (Ubuntu and Fedora) and a Mac to run and test on; this Windows PC can
  only check that the code compiles for those targets.
- macOS: Apple Developer Program ($99/year) for signing and notarization; Network Extension
  entitlement request for the firewall.
- CI: add `ubuntu-latest` and `macos-latest` jobs to `.github/workflows/release.yml` once step 1
  lands.

## Rough size

Step 1 is a few days. Steps 3 and 5 are about a week each for monitoring parity. The two
firewalls (4, 6) are the long poles, a week or more each, and macOS's depends on Apple.
