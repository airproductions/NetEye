# GlassWire vs Vantage: feature gap report

Sources: ~1,150 UI strings from GlassWire's translation file (inferred features, not verified in the running product) and a read of Vantage's source (`C:\Users\idanr\GlassWire\src`, ~6,200 lines). Vantage claims were confirmed in code, not taken from the README. Paths below are relative to `src/`.

Legend for effort: S = under a day, M = a few days, L = a week or more / architectural.
Flags: [DRIVER] needs a kernel driver, [CLOUD] needs a cloud service/server, [ACCT] needs an account, [KEY] needs an API key or paid service.

## Status refresh (2026-10-27, evening; app now named NetEye)

Re-checked against the code at `classic-graphite`. The tables below are the morning snapshot; this list supersedes them.

**Built since the morning report:** scale menu and pause (G6, G7), group/network/direction filters (G8), countries with flags (G11 view), traffic types (G12), bits/bytes (G17), Ask to Connect and Block All modes (F3, F4), firewall profiles (F6), Allow all in the app popup (F7), 75% plan warning with day/week/month periods (A4), device left/joined (A5), filter by alert type, sort, Mark all read (A8, A9, A11), snooze (A13), usage tables by host/type/country (U1), data plan (U2), All Time/Day/Week/Month/Custom (U3), CSV export (U4), exclude LAN from plan (U5), scan interval (N4), rename devices (N6), ARP spoofing, DNS, proxy and RDP monitors (S2, S5, S12, S13), VirusTotal hash lookup (S6), app details popup with path menu (D1, D2), host countries (D9), run on startup (T4), host/country popups, column choosers, scanner network picker.

**Still missing** (excluding the Skip rows: premium, remote servers, community stats, credential check):

| Area | Feature | Effort |
|---|---|---|
| Graph | Live connections page (G9; today: Connected now list + firewall row expansion) | M |
| Graph | Live world map (G10) | L |
| Graph | Block by country (G11, blocking half) | L |
| Graph | Save graph as image (G16) | S |
| Graph | Graph colour themes (G18) | S |
| Firewall | Uninstalled section and remove old apps (F8, F9) | S |
| Firewall | Guard NetEye itself and Defender from blocks (F12) | S |
| Firewall | Warn when Windows Firewall is off (F13) | S |
| Firewall | Settings/firewall password (F14, T11) | M |
| Firewall | Custom IP/host/port rules (F17) | M |
| Alerts | Delete a single alert (A10) | S |
| Alerts | While You Were Away summary (A12) | M |
| Alerts | Mirror to Windows Event Log (A15, T10) | S |
| Alerts | Alert retention setting (A16; fixed 2,000) | S |
| Things | Forget device (N7) | S |
| Things | Trust devices, alert only on unknown ones (N8, N9) | S |
| Things | Device type override (N6, second half) | S |
| Things | Network info: DNS servers, saved Wi-Fi (N10) | M |
| Security | Suspicious host list (S1) | L |
| Security | Evil twin / open Wi-Fi alerts (S3, S4) | M |
| Security | Camera and microphone use (S7) | M |
| Security | Executable changed, publisher changed, signature lost (S8, S9, S10) | M |
| Security | hosts file monitor (S14) | S |
| Security | New network adapter/driver alert (S15) | M |
| App details | Hide app, Incognito, Trust app (D3, D4, D5) | S–M |
| App details | Search online (D6) | S |
| App details | Signed/unsigned status (D7) | M |
| Tray | Mini Viewer with always-on-top (M2, M3) | M |
| Tray | Tray menu actions: firewall mode, snooze, mini viewer (M6) | S |
| Settings | Language selection (T1) | M |
| Settings | Text size (T3) | S |
| Settings | Update check / auto-update (T5, T6, A17) | M |
| Settings | 24-hour clock option (T8) | S |
| Settings | History retention setting (T9) | S |
| Usage | Clear history for a date range (U6) | S |

What Vantage was in the morning (confirmed in code): 6 pages (Traffic, Firewall, Activity, Scanner, Hardware, Settings; `ui/mod.rs:243-249`), per-app in/out block via persistent WFP filters (`wfp.rs:148`, `system.rs:36`), 7 event kinds (`store.rs` `EventKind`: NewApp, InternetUp, InternetDown, VersionChanged, NewDevice, Firewall, DataCap), 5 notification toggles (`ui/settings.rs:70-74`), tray with Open/Quit (`ui/mod.rs:480-523`), 24 h of per-minute and 32 days of per-hour buckets (`store.rs:10-11`).

---

## A. GlassWire feature inventory

IDs (G1, F1, ...) are reused in the table in section B.

### Graph / Traffic
- G1 Live in/out bandwidth graph: real-time chart of upload and download. "Traffic In" / "Traffic Out"
- G2 Time ranges: switch the graph window from minutes to a month. "5 Minutes" / "3 Hours" / "24 Hours"
- G3 Usage view: totals as bars per time slice. "Strings::UsagePage" / "All Usage"
- G4 Per-app graph filter: pick one app and see only its traffic. "All Apps" / "Active Apps"
- G5 Hosts view: which remote hosts an app talks to, with counts. "Hosts (%1)" / "All hosts"
- G6 Auto-scale: graph axis scales itself or is set manually. "Enable Auto-Scale" / "Uncheck Auto-Scale to set the desired graph scale manually."
- G7 Pause graph: freeze the live graph to inspect it. "Pause the graph" / "Unpause the graph"
- G8 Traffic type and direction filters: internet vs local, in vs out, protocol. "Traffic Type" / "Traffic direction" / "Exclude local network traffic."
- G9 Live Connections pane: list of current connections, disabled while filtering. "Live Connections" / "Live Connections are disabled whilst filtering."
- G10 Live global traffic map: world map of app and host connections. "Live Global Traffic Map" / "Discover real-time global app and host connections on an interactive map."
- G11 Country-level view and block: group hosts by country, block whole countries. "All countries" / "block suspicious country communications"
- G12 Protocol breakdown: traffic by protocol. "Used Protocols"
- G13 Search and filters: filter apps/hosts/publishers/countries. "All publishers" / "No matches found. Modify your search or reset it."
- G14 Unidentified traffic bucket: traffic that cannot be tied to an app. "Unidentified Traffic"
- G15 Host DNS names: reverse-resolve remote IPs. "Look up DNS names for hosts (nslookup)"
- G16 Snapshot: save the current graph as an image, with social upload. "GlassWire Snapshot" / "Choose where to save the snapshot"
- G17 Bandwidth units: choose bits vs bytes per second. "Bandwidth Speed Units:" / "Bits per second"
- G18 Graph themes: coloured graph skins. "Graph Theme:" / "Night Neon Theme"
- G19 Apps Popularity / average consumption vs other users: cloud comparison. "Apps Popularity" / "%1 out of %2 GlassWire users have used %3 in the past month."
- G20 Anomaly Detection and GW Score: cloud-assisted safety score. "Anomaly Detection" / "GlassWire Safety Score"

### Firewall
- F1 Per-app block, split by direction. "Block Incoming Connections" / "Block Outgoing Connections"
- F2 Master firewall on/off. "Firewall On" / "Firewall Off"
- F3 Ask to Connect mode: new unknown apps prompt before they get online. "Block new unknown connections with \"Ask to connect\" mode."
- F4 Block All mode: cuts all apps at once. "Block All Connections" / "GlassWire has blocked connections for all applications."
- F5 Lockdown / Allow All: opposite switch that permits everything. "Allow All Connections" / "Lock Down Mode"
- F6 Firewall profiles: save and switch named rule sets. "Create and save firewall profiles." / "Create New Firewall Profile"
- F7 Reset: unblock everything / restore defaults. "Unblock all apps" / "Restore defaults. All currently blocked apps will be unblocked."
- F8 Prune the list: remove inactive or uninstalled apps. "Remove inactive applications." / "Remove uninstalled applications."
- F9 Active / Inactive / Uninstalled app sections. "Active Apps" / "Inactive Apps" / "Uninstalled Apps"
- F10 Show connected ports per app. "Show connected ports on the Firewall tab"
- F11 Protected-process safeguards: warns when blocking critical components. "System can't be blocked." / "Blocking the Windows kernel can cause instability."
- F12 Self and defender safeguards. "GlassWire can't be blocked." / "Blocking Windows Defender makes your PC susceptible to malware."
- F13 Windows Firewall status and enable prompt. "Your Windows Operating System Firewall is disabled." / "Turn on the Windows Firewall"
- F14 Admin-password gate for firewall changes. "Enable administrator's account password request"
- F15 Hosts an app is currently connecting to, shown in the firewall row. "Upgrade to see the hosts that the app is currently connecting to."
- F16 Close existing connections when blocking (implied by block semantics). "Click To Block"
- F17 Custom host / port-range rules (IP, port start/end entries; may belong to remote-access settings, uncertain). "Port range begins:" / "Hostname or IP address:"
- F18 Free-tier cap on blocked apps. "GlassWire Free allows for the blocking of maximum 2 Apps."

### Alerts / Activity log
- A1 First network activity: an app goes online for the first time. "%1 initiated its first network connection to %2"
- A2 Internet up/down. "Internet connection was lost." / "Internet connection now available."
- A3 App info changed: version change. "The application version changed from %1 to %2."
- A4 Traffic limit and overage: warning at 75%, alert at limit, over a chosen period. "You used 75% of your %1 traffic limit." / "Traffic limit reached"
- A5 Network device join/leave. "%1 joined the %2 network." / "Report all devices joining or leaving."
- A6 Firewall state changes. "Firewall disabled by %1" / "\"%1\" is now on."
- A7 Severity levels and Important section. "Important" / "Alerts level:"
- A8 Filters by type, severity, time period. "Filter by alert type" / "Filter by severity"
- A9 Read/unread handling and bulk mark. "Mark all as read" / "Mark as unread"
- A10 Delete alert / clear history. "Delete Alert" / "Clear history"
- A11 Search and sort alerts. "Close search bar" / "Sort by Date"
- A12 While You Were Away report: summary of activity during idle time. "While you were away %1 apps accessed the network. %2 alerts were received."
- A13 Snooze notifications for an interval. "Snooze mode on" / "Notifications disabled for unlimited interval."
- A14 Desktop (tray) notification toggle. "Show desktop notification." / "Enable tray notifications"
- A15 Mirror alerts to the Windows Event Log. "Send GlassWire Alerts to Windows Event Log"
- A16 Alert retention / history limit. "Keep last day" / "Keep last week"
- A17 Service-health alerts (GlassWire service stopped, update available). "The GlassWire service stopped running."

### Usage
- U1 Usage tables by app, host, country, traffic type for a period. "Total network usage:" / "Traffic amount:"
- U2 Data plan with billing start day and allowance. "Data Plan" / "Start day:"
- U3 Long history and fixed periods. "Last 7 Days" / "Last 30 Days" / "All Time"
- U4 Export as CSV. "Export as CSV" / "Data Export Functionality"
- U5 Exclude local network from usage. "Exclude local network traffic."
- U6 Clear history for a chosen period. "Are you sure you want to permanently clear your history for the selected time period?"
- U7 Hardware resource usage (CPU, GPU, memory, disk) per process. "Process Resource Usage" / "All GPU engines utilization"

### Things / Network scanner
- N1 Device list with IP, MAC, vendor, first/last seen. "First Seen" / "Last Seen" / "IP & MAC address"
- N2 Device type classification with many icons. "Device Types" / "Smart Thermostat/Doorbell/Printer..." (about 80 types)
- N3 Online/offline status. "Device State" / "Last seen %1"
- N4 Auto-scan with interval. "Auto-scan the network every"
- N5 Manual scan with progress and timestamp. "Scanning..." / "Last scan: %1"
- N6 Edit device (name, type, notes, vendor). "Device options" / "Make and Model:" / "Notes:"
- N7 Forget device. "Forget device"
- N8 Trust/untrust a device. "Mark as untrusted"
- N9 Report only new unknown devices vs all joins/leaves. "Report only new unknown devices."
- N10 Network info and saved networks (gateway, DNS, WiFi, forget network). "About Network" / "Forget Network" / "Gateway:" / "DNS Servers:"
- N11 Scan thread count. "Threads quantity:"
- N12 Hostname lookup of devices. "Look up DNS names for hosts (nslookup)"
- N13 Vendor database lookup. "Vendor:"

### Security
- S1 Suspicious host connection: app talks to a flagged host. "Suspicious host connection" / "Threat connection attempt"
- S2 ARP spoofing: IP-to-MAC mapping changed. "ARP spoofing detection" / "IP: %1, Old MAC: %2, New MAC: %3"
- S3 Evil twin / suspicious WiFi. "Possible evil twin WiFi detected" / "A suspicious WiFi connection was detected."
- S4 Open or newly unprotected WiFi. "Your WiFi is no longer password protected."
- S5 RDP detection. "RDP connected" / "Remote Desktop connection terminated."
- S6 VirusTotal scan of app executables. "VirusTotal scan result is %1/%2." / "Activate VirusTotal scanning with your personal API Key."
- S7 Camera and microphone usage. "The application has activated the \"%1\" camera." / "Notify me if an application is using my camera or microphone."
- S8 Executable changed. "The application executable changed."
- S9 Publisher changed. "%1 publisher changed from %2 to %3."
- S10 Signature lost. "The application is no longer signed."
- S11 Certificate/HTTPS MITM error. "HTTPS \"Man in the middle\" error."
- S12 DNS server settings monitor. "DNS server settings changed" / "New DNS server addresses"
- S13 Proxy settings monitor. "Proxy settings changed"
- S14 System file monitor (hosts / lmhosts). "Notify me if \"hosts\" or \"lmhosts\" files are modified."
- S15 Network device/driver list monitor. "Notify me if a networking device or driver has been added, removed, or changed."
- S16 Credential checking. "Credential checking" (purpose unclear from strings; likely a check for exposed credentials)
- S17 Security Settings monitor umbrella. "Security Settings monitor"

### App details popup and per-app actions
- D1 App details popup: publisher, path, version, first seen, hosts, resource use. "Application info monitor" / "First network activity" / "Publisher:" / "Version:"
- D2 Open file location. "Open File Location"
- D3 Hide app / Show app / Unhide all. "Hide App" / "Show App" / "Unhide all apps"
- D4 Incognito: stop logging an app's traffic. "Add to Incognito..." / "Incognito mode is on."
- D5 Trust the app / mark untrusted. "Trust the App" / "Mark as untrusted"
- D6 Search online for an app. "Search Online"
- D7 Signed/unsigned status and VT status shown. "Unsigned" / "Unscanned" / "Clean (%1)" / "Infected (%1)"
- D8 Per-app block from the popup. "Block App" / "Unblock App"
- D9 Per-app hosts with country. "Hosts (%1)" / "Countries"
- D10 Per-app process info (PID, services). "Process ID" / "%n Service(s)"

### Mini viewer / tray
- M1 Tray icon with menu. "Show/Hide GlassWire" / "Strings::TrayMenu"
- M2 Mini Viewer: small live widget. "Show Mini Viewer" / "Mini Viewer"
- M3 Keep Mini Viewer on top, adjustable transparency. "Keep Mini Viewer on top" / "Mini Viewer Transparency"
- M4 Tray popups. "Strings::TrayPopups"
- M5 Close-to-tray behaviour. "Hide GlassWire"

### Remote access / servers
- R1 Remote server list: monitor other PCs. "Add New Server" / "Server List"
- R2 Remote access password and encryption. "Allow Remote Access by password" / "Remote access is encrypted."
- R3 IP allow-list and listening port. "Allow Access only from IP:" / "Server listening port:"
- R4 Remote clients online and connect alerts. "GlassWire remote client connected from %1" / "Remote clients online:"
- R5 Remote control of firewall. "Remote Control" / "This mode will block remote monitoring and disconnect you, are you sure you want to do this?"
- R6 Local server / multi-server switcher. "Connecting to the Local Server." / "Connecting to \"%1\" Server."

### Settings
- T1 Language selection (about 17 languages). "Language:" / "Chinese Simplified"
- T2 App themes. "Themes" / "Asphalt Theme" / "Daylight Theme"
- T3 Text size. "Text size:"
- T4 Run on startup. "Run on startup"
- T5 Auto-update with schedule. "Enable automatic version upgrades" / "Install at Midnight" / "Skip Version"
- T6 Check for updates. "Check for updates"
- T7 Speed units. "Bandwidth Speed Units:"
- T8 System time format. "Use system time format"
- T9 History retention. "Clear History:" / "Keep last week"
- T10 Windows Event Log mirror. "Send GlassWire Alerts to Windows Event Log"
- T11 Settings password. "Enable administrator's account password request"
- T12 Per-monitor alert switches. "Notify me about RDP connections." / "Notify me about network device activity."
- T13 GPU acceleration toggle. "Use GPU acceleration (experimental)"
- T14 Restart-to-apply and settings-changed alerts. "Restart to apply settings changes."

### Premium / account
- P1 Free vs Premium gating. "Upgrade to Premium to unlock full firewall control."
- P2 Trial. "Your premium trial expires in %n days."
- P3 Account sign in/up. "Create an account" / "Sign In"
- P4 Licence activation. "Activate with License code"
- P5 Management console and endpoint licences. "Management Console"
- P6 Subscription renewal and billing. "Extend Subscription"
- P7 Upsell and marketing prompts. "Unleash the full power of GlassWire!"
- P8 Social sharing. "Share on Facebook"
- P9 Ads for a partner product (IT monitoring). "Go to Domotz for a full feature network monitoring solution"

---

## B. Comparison table

| Area | Feature | In Vantage? | Effort | Notes |
|---|---|---|---|---|
| Graph | G1 Live in/out graph | Yes: `ui/traffic.rs:331,386` | - | |
| Graph | G2 Time ranges | Yes: `ui/traffic.rs:12` (5 min to month) | - | Same five ranges as GlassWire |
| Graph | G3 Usage bars view | Yes: `ui/traffic.rs:331,465` | - | |
| Graph | G4 Per-app graph filter | Yes: `ui/traffic.rs:295` | - | |
| Graph | G5 Hosts view | Yes: `ui/traffic.rs:529` | - | Needs admin (TCP EStats, `net.rs:263`) |
| Graph | G6 Auto-scale toggle / manual scale | No | S | |
| Graph | G7 Pause graph | No | S | |
| Graph | G8 Traffic type / direction / protocol filters | Partial: internet vs LAN is counted (`store.rs` `Bucket.lan_*`, `ui/traffic.rs:134,665`) but not filterable | S | Add toggles over existing buckets |
| Graph | G9 Live Connections pane | Partial: per-app connection list in firewall row expansion (`ui/firewall.rs:285-313`) | M | Needs its own page with all-app live list |
| Graph | G10 Live global traffic map | No | L | Needs an offline GeoIP DB (DB-IP/GeoLite; GeoLite needs a free MaxMind account key) plus map rendering. [KEY, optional] |
| Graph | G11 Country view and country block | No | L | GeoIP DB as above; blocking by country means WFP remote-address filters from IP ranges, which are large |
| Graph | G12 Protocol breakdown | No | S | Port to protocol table from `Conn.remote_port` |
| Graph | G13 Search / filter apps, hosts, publishers | Partial: app search `ui/traffic.rs:220`, firewall search by app/publisher/host `ui/firewall.rs:181` | S | No country/publisher dropdown |
| Graph | G14 Unidentified traffic bucket | Yes: `monitor.rs:115` (`other_name`) | - | |
| Graph | G15 Host DNS names | Yes: `system.rs:242`, `monitor.rs:158` | - | Always on, no setting |
| Graph | G16 Snapshot image | No | S | egui screenshot command; skip the social upload |
| Graph | G17 Bits vs bytes units | No | S | `theme::rate` formatting plus a setting |
| Graph | G18 Graph themes | Partial: light/dark/system only (`ui/settings.rs:50-58`) | S | Colour presets via `ui/palette.rs` |
| Graph | G19 Apps Popularity / community average | No | L | Skip: [CLOUD] [ACCT] and sends usage data off-device |
| Graph | G20 Anomaly Detection / GW Score | No | L | Skip: [CLOUD]; a local baseline anomaly alert is a possible substitute |
| Firewall | F1 Per-app block, in/out separately | Yes: `ui/firewall.rs:233-240`, `monitor.rs:217`, `wfp.rs:148` | - | Persistent WFP filters, works with Kaspersky etc. |
| Firewall | F2 Master firewall on/off | Yes: `monitor.rs:255`, `ui/firewall.rs:151-160` | - | |
| Firewall | F3 Ask to Connect mode | No | L | Needs to hold a new connection until the user answers: WFP callout [DRIVER], or approximate with default-block-new-apps plus an Allow toast (no kernel driver, but the first connection fails) |
| Firewall | F4 Block All mode | No | S | One WFP "block all except Vantage/system" filter, plus a toggle and tray item |
| Firewall | F5 Lockdown / Allow All | Partial: master switch removes all blocks (`monitor.rs:255`) | S | Real lockdown = F4 variant that keeps a user-chosen allow-list |
| Firewall | F6 Firewall profiles | No | M | Persist named sets of `block_in/out` (`store.rs` `AppRecord`) and apply as a batch via `system.rs:36` |
| Firewall | F7 Unblock all / restore defaults | No | S | Batch unblock through `apply_firewall` |
| Firewall | F8 Remove inactive / uninstalled apps from list | No | S | Delete from `store.data.apps` after confirming no active filter |
| Firewall | F9 Active / Inactive sections | Partial: Active and Inactive (`ui/firewall.rs:122-133`); no Uninstalled split | S | Check `path` exists |
| Firewall | F10 Connected ports per app | Yes: `ui/firewall.rs:300,313` | - | |
| Firewall | F11 Protected-process safeguards | Yes: `system.rs:113`, tooltip `ui/firewall.rs:233` | - | |
| Firewall | F12 Self / Defender safeguards | Partial: system processes covered, Vantage itself and Defender not special-cased (`system.rs:113-117`) | S | Add exe names to the list with a stronger warning |
| Firewall | F13 Windows Firewall status and enable prompt | Partial: detects registered product (`system.rs:121`, `ui/firewall.rs:166`); no off warning | S | |
| Firewall | F14 Admin password gate | No | M | Local passphrase hash in settings. Low value for a single-PC app |
| Firewall | F15 Hosts per app in firewall row | Yes: `ui/firewall.rs:250-262` | - | |
| Firewall | F16 Close open connections on block | Yes: `net.rs:245`, `ui/settings.rs:124` | - | Vantage extra |
| Firewall | F17 Custom IP/host/port rules | No | M | WFP supports remote address/port conditions; needs rule editor UI |
| Firewall | F18 Free-tier block limit | No | - | Skip: licensing |
| Alerts | A1 First network activity | Yes: `monitor.rs:640-642`, `ui/settings.rs:71` | - | |
| Alerts | A2 Internet up/down | Yes: `monitor.rs:296-312` | - | |
| Alerts | A3 App version changed | Yes: `monitor.rs:588-590` | - | Off by default (`store.rs` Default) |
| Alerts | A4 Traffic limit and 75% warning, custom period | Partial: daily cap only (`monitor.rs:655-661`, `ui/settings.rs:84-90`) | S | Add warning at 75% and a weekly/monthly period |
| Alerts | A5 Device join/leave | Partial: join only (`monitor.rs:385-387`) | S | Track `s.online` transitions (`ui/scanner.rs:293`) |
| Alerts | A6 Firewall change events | Yes: `monitor.rs:244,282` | - | |
| Alerts | A7 Severity / Important section | Partial: Important vs Everything (`ui/log.rs:12`, `store.rs` `important()`) | S | Two levels only |
| Alerts | A8 Filter by alert type | No | S | |
| Alerts | A9 Read/unread, mark all, mark unread | Partial: auto-marks all read on open (`ui/log.rs:28-32`) | S | |
| Alerts | A10 Delete single alert / clear history | Partial: Clear log (`ui/log.rs:47`) | S | |
| Alerts | A11 Alert search and sort | Partial: search (`ui/log.rs:51`), time-period filter (`ui/log.rs:13`); no sort | S | |
| Alerts | A12 While You Were Away report | No | M | Detect idle (GetLastInputInfo), summarise events on return |
| Alerts | A13 Snooze | No | S | Timestamp checked inside `notify()` (`monitor.rs:748`) |
| Alerts | A14 Desktop notification toggle | Yes: `ui/settings.rs:70`, `monitor.rs:748` | - | |
| Alerts | A15 Mirror to Windows Event Log | No | S | `ReportEvent` or `eventcreate`; needs admin once to register source |
| Alerts | A16 Alert retention options | Partial: fixed 2000 events (`store.rs:12`) | S | |
| Alerts | A17 Service-health / update alerts | No | S | Skip service part (Vantage has none); update alert depends on T5 |
| Usage | U1 Usage tables by app/host/country/type | Partial: app totals and hosts per period (`ui/traffic.rs:214,529`); no country/type table, no dedicated Usage page | M | |
| Usage | U2 Data plan (billing day, allowance) | Partial: daily cap only (`store.rs` `daily_cap_mb`) | M | Add monthly plan, cycle start, remaining days |
| Usage | U3 Long history, All Time, Yesterday etc. | Partial: 24 h minute and 32 d hour buckets (`store.rs:10-11`) | M | Add daily rollups (the `daily` map exists, `store.rs` `Data.daily`, totals only) with per-app daily |
| Usage | U4 Export CSV | No | S | `csv` crate or manual; export buckets/events |
| Usage | U5 Exclude local network traffic | Partial: LAN tracked separately (`store.rs` `Bucket`, `ui/traffic.rs:665`) | S | Make a toggle |
| Usage | U6 Clear history for a period | Partial: reset all (`ui/settings.rs:96-115`, `store.rs` `reset_usage`) | S | |
| Usage | U7 Hardware resource usage per process | Yes: `ui/hardware.rs:51-65,161`, `hw.rs:131` | - | Beyond GlassWire's scope |
| Things | N1 Device list (IP, MAC, vendor, seen times) | Yes: `ui/scanner.rs:255,308-312` | - | |
| Things | N2 Device type classification | Partial: about 5 guessed types (`ui/scanner.rs:180-201`) | S | Extend keyword/OUI rules, allow manual override (N6) |
| Things | N3 Online/offline | Yes: `ui/scanner.rs:293` | - | |
| Things | N4 Auto-scan with chosen interval | Partial: fixed 5 min toggle (`monitor.rs:314-326`, `ui/scanner.rs:238`) | S | |
| Things | N5 Manual scan with timestamp | Yes: `ui/scanner.rs:229-235` | - | /24 only (`system.rs:218-222`) |
| Things | N6 Edit device name/type/notes | No | S | `DeviceRecord.name` exists (`store.rs`); add user override fields |
| Things | N7 Forget device | No | S | |
| Things | N8 Trust / untrust device | No | S | Feeds N9 |
| Things | N9 Alert only on unknown devices | No | S | Needs N8 |
| Things | N10 Network info and saved networks | Partial: gateway, local IP, adapter label (`ui/scanner.rs:222`, `monitor.rs:375-380`); no DNS, no saved-network list | M | `GetAdaptersAddresses` for DNS; WLAN API for saved WiFi |
| Things | N11 Scan thread count | No | S | Hard-coded 8-wide chunks (`system.rs:226`); low value |
| Things | N12 Device hostname lookup | Yes: `monitor.rs:354`, `system.rs:242` | - | Reverse DNS only; no mDNS/NetBIOS |
| Things | N13 Vendor lookup | Yes: `system.rs:261` | - | Downloads Wireshark manuf once |
| Security | S1 Suspicious host connection | No | L | Needs a threat list. Free feeds (abuse.ch, URLhaus) could be fetched on a schedule, but this is [CLOUD]-adjacent and needs list upkeep |
| Security | S2 ARP spoofing detection | No | M | Compare the gateway/device IP-to-MAC mapping per scan (`DeviceRecord.mac` already stored); `GetIpNetTable` for passive checks |
| Security | S3 Evil twin / suspicious WiFi | No | M | WLAN API (`WlanGetAvailableNetworkList`); heuristics for same-SSID different-BSSID/security |
| Security | S4 Open or newly unprotected WiFi | No | S | Same WLAN API, check auth algorithm |
| Security | S5 RDP detection | No | M | Poll TCP table for inbound 3389 (`net.rs:150` already lists connections) or WTS session events |
| Security | S6 VirusTotal scan | No | M | [KEY] user's free personal API key (rate limited); hash lookup before upload; uploads use bandwidth and share files, so privacy opt-in |
| Security | S7 Camera / microphone usage | No | M | Read `CapabilityAccessManager\ConsentStore` timestamps (webcam, microphone) in registry; no driver |
| Security | S8 Executable changed | No | M | Hash apps at first sight and on version change (`monitor.rs:580-592` is the hook point) |
| Security | S9 Publisher changed | No | S | `VersionInfo` already read (`net.rs:391-398`); compare with `AppRecord.publisher` |
| Security | S10 Signature lost | No | M | `WinVerifyTrust` |
| Security | S11 HTTPS MITM / cert error | No | L | Unclear how GlassWire detects it; would need TLS probing against known certs. Skip initially |
| Security | S12 DNS settings monitor | No | S | Poll adapters' DNS via `GetAdaptersAddresses` |
| Security | S13 Proxy settings monitor | No | S | Poll `HKCU\...\Internet Settings` and WinHTTP proxy |
| Security | S14 hosts / lmhosts file monitor | No | S | Hash file on a timer |
| Security | S15 Network device / driver monitor | No | M | SetupAPI or WMI `Win32_NetworkAdapter` snapshots |
| Security | S16 Credential checking | No | ? | Purpose unclear; cannot infer from the strings. Likely cloud [CLOUD]. Skip |
| Security | S17 Security-settings umbrella toggles | No | S | Settings entries for S8-S15 |
| App details | D1 App details popup | Partial: expandable row with connections, publisher, version, ports (`ui/firewall.rs:285-313`, `ui/traffic.rs:295`); no popup with first-seen, path, graph | M | |
| App details | D2 Open file location | No | S | `explorer /select,` on `AppRecord.path` |
| App details | D3 Hide / Show / Unhide all | No | S | Flag on `AppRecord`; filter in lists |
| App details | D4 Incognito | No | M | Exclude app from buckets/events/hosts in `store.add_usage` and `monitor.rs`; decide whether past data is purged |
| App details | D5 Trust / untrust app | No | S | Flag; ties to Ask-to-Connect and alerts |
| App details | D6 Search online | No | S | Open browser with query |
| App details | D7 Signed / VT status | No | M | Depends on S10 and S6 |
| App details | D8 Block/unblock from details | Yes: `ui/firewall.rs:233-240` | - | In the row, not a popup |
| App details | D9 Per-app hosts with country | Partial: hosts only (`ui/firewall.rs:250-262`) | M | Country needs GeoIP (G10) |
| App details | D10 Per-app process info and resources | Yes: `ui/hardware.rs:51-104`, PID `ui/firewall.rs:299` | - | |
| Mini/tray | M1 Tray icon and menu | Yes: `ui/mod.rs:480-523` | - | Menu has only Open and Quit |
| Mini/tray | M2 Mini Viewer | No | M | Second egui viewport (small, undecorated, live rate) |
| Mini/tray | M3 Always-on-top and transparency | No | S | `ViewportBuilder::with_always_on_top`, alpha; needs M2 |
| Mini/tray | M4 Tray popups/notifications | Yes: `monitor.rs:748` (`notify_rust`) | - | |
| Mini/tray | M5 Close-to-tray | Yes: `ui/settings.rs:125`, `store.rs` `close_to_tray` | - | |
| Mini/tray | M6 Tray menu actions (mode, snooze, mini viewer) | No | S | Extend `build_tray` after F4/A13/M2 exist |
| Remote | R1 Remote server list | No | L | Skip: needs a service on each PC, network exposure, auth |
| Remote | R2 Remote password + encryption | No | L | Skip with R1 |
| Remote | R3 IP allow-list and listening port | No | M | Skip with R1 |
| Remote | R4 Remote client alerts | No | M | Skip with R1 |
| Remote | R5 Remote firewall control | No | L | Skip with R1; also a security liability |
| Remote | R6 Multi-server switcher | No | M | Skip with R1 |
| Settings | T1 Language selection | No | M | UI strings are inline literals; needs an i18n layer and translations |
| Settings | T2 App themes | Partial: System/Dark/Light (`ui/settings.rs:50-58`) | S | |
| Settings | T3 Text size | No | S | egui `set_zoom_factor` or font scale |
| Settings | T4 Run on startup | No | S | `HKCU\...\Run` entry; elevated start needs Task Scheduler |
| Settings | T5 Auto-update with schedule | No | M | [CLOUD] needs a release host (e.g. GitHub Releases API). Optional |
| Settings | T6 Check for updates | No | S | Same endpoint as T5 |
| Settings | T7 Speed units | No | S | Duplicate of G17 |
| Settings | T8 System time format | No | S | Not verified how `theme::date_time` formats; assume fixed |
| Settings | T9 History retention | No | S | `MAX_*` constants in `store.rs:10-13` made configurable |
| Settings | T10 Windows Event Log mirror | No | S | Duplicate of A15 |
| Settings | T11 Settings password | No | M | Duplicate of F14 |
| Settings | T12 Per-monitor alert switches | Partial: 4 alert toggles + master (`ui/settings.rs:70-74`; none for data cap or firewall) | S | Grows with new monitors |
| Settings | T13 GPU acceleration toggle | No | - | Not applicable: Vantage uses glow already |
| Settings | T14 Restart-to-apply and settings-changed alerts | No | S | Low value |
| Premium | P1 Free vs Premium gating | No | - | Skip: licensing; Vantage has everything unlocked |
| Premium | P2 Trial | No | - | Skip |
| Premium | P3 Account sign in/up | No | - | Skip: [ACCT] |
| Premium | P4 Licence activation | No | - | Skip |
| Premium | P5 Management console | No | - | Skip: [CLOUD] [ACCT] |
| Premium | P6 Subscription renewal and billing | No | - | Skip |
| Premium | P7 Upsell and promo prompts | No | - | Skip |
| Premium | P8 Social sharing | No | - | Skip: sends data to third parties |
| Premium | P9 Partner ad (Domotz) | No | - | Skip |

---

## C. Recommended build order

Ordered by value to a single-PC, local-only app, and by how self-contained each batch is. Each batch reuses existing patterns (event kinds in `store.rs`, notify toggles in `ui/settings.rs`, `Action` plumbing in `ui/mod.rs:23`).

### Batch 1: Alert polish and quick wins (all S, no new subsystems)
A4 (75% warning, weekly/monthly period), A5 (device leave), A8 (filter by type), A9/A10/A11 (read/unread, delete one, sort), A13 snooze, A16/T9 retention, T12 toggles for every alert kind, T4 run on startup, T3 text size, G17/T7 units, G7 pause graph, G6 auto-scale, U4 CSV export, G16 snapshot, M6 tray menu items. Value: visible improvements with little risk.

### Batch 2: App-change security monitors (new `EventKind`s, no network I/O)
S9 publisher changed, S8 executable changed (hash), S10 signature lost, A3 already exists. Then the settings-watchers: S12 DNS, S13 proxy, S14 hosts/lmhosts, S15 adapter/driver list, S17 toggles. One polling thread and a few `EventKind` variants cover all of these. Value: matches GlassWire's core "something changed" value, fully local.

### Batch 3: Firewall control modes
F4 Block All, F5 real Lockdown, F7 unblock all, F8 prune lists, F9 uninstalled section, F12 self/Defender guards, F13 Windows Firewall off warning, then F6 profiles on top, then F17 custom host/port rules. Biggest user-facing firewall value and all within WFP (`wfp.rs`), no driver. Do F3 Ask to Connect last (see batch 7) because it is the hard one.

### Batch 4: App details and per-app actions
D1 details popup (graph, first seen, path, hosts, resource use), D2 open file location, D3 hide, D5 trust, D6 search online, D4 incognito. Prerequisite for Batch 3's profiles and Batch 7's Ask-to-Connect UX (trusted flag).

### Batch 5: Network and wireless security (Things)
N6 edit device, N7 forget, N8 trust, N9 unknown-only alerts, N2 more device types, N4 interval, N10 network info/DNS, S2 ARP spoofing, S4 open WiFi, S3 evil twin. Then S5 RDP and S7 camera/mic. Value: extends the scanner Vantage already has; ARP spoofing reuses the stored MAC table. WLAN API is the new dependency.

### Batch 6: Usage and history depth
U2 data plan (billing cycle), U3 long history (daily rollups per app), U1 Usage page by app/host/type, U5 exclude-local toggle, U6 clear a period, G8/G12 type/protocol filters, G9 Live Connections page, M2/M3 Mini Viewer. Mostly store-schema work; do the rollup migration carefully (`store.rs` uses `#[serde(default)]`, so adding fields is safe).

### Batch 7: Heavy or dependent features (do only if wanted)
- F3 Ask to Connect: a proper version needs a WFP callout driver [DRIVER]. A driverless approximation (block new apps by default, toast to allow) is feasible but the first connection fails.
- G10/G11/D9 world map, country view, country block: needs an offline GeoIP DB (optional [KEY]); the block half is heavy because country ranges are many WFP filters.
- S6/D7 VirusTotal: [KEY] personal API key, opt-in, privacy and data-use caveats (uploads share files).
- S1 suspicious host connection: needs a maintained threat feed.
- A12 While You Were Away report and A15 Event Log mirror (M and S respectively; fine anywhere).
- T1 languages, T5/T6 auto-update (needs a release host).

### Skip (does not fit a local, privacy-respecting, single-PC app)
- P1-P9 all of Premium/account/licensing/upsell/social/partner ad: not relevant; Vantage is fully unlocked and free. F18 and N-tier "Upgrade to ..." gating likewise.
- R1-R6 Remote servers/remote access/remote control: needs an always-listening service, auth and encryption, and remote firewall control is a security risk; GlassWire sells this as a product.
- G19 Apps Popularity and G20 Anomaly Detection / GW Score: [CLOUD] and [ACCT]; community data requires sending usage off the PC. A local-baseline anomaly alert is the privacy-friendly substitute if wanted.
- S16 Credential checking: unclear, likely cloud.
- S11 HTTPS MITM detection: unclear method, high false-positive risk.
- G16's social upload and P8 sharing.
- T13 GPU toggle, T14 restart prompts, F14/T11 settings password: little value on a single-user local app (T11/F14 are optional, M).
