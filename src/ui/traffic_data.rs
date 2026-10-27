//! Traffic aggregation: groups, filters, selection and cache (Task 6).
//!
//! `aggregate` turns stored buckets (or live samples for 5 minutes) into everything the Traffic
//! page draws: graph columns, list rows for the active Group by, the three Usage columns, the
//! strip totals and the scrubber history. It is pure; `AggCache` keeps the last result so
//! history is never re-aggregated every frame.
//!
//! Every step is reduced to "atoms": one per stored flow, plus one per app for its residual
//! (`apps[k] − Σ flows[k]`, the bytes recorded without flows), which is "Not classified" as a
//! type, a country and a host. Rows, columns and totals all add the same atoms, so they
//! reconcile by construction.
#![allow(dead_code)] // consumed by Task 7; removed in Task 11

use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasherDefault, Hasher};
use std::net::IpAddr;
use std::sync::Arc;

use chrono::{Datelike, Local, NaiveDate, TimeZone};

use crate::geo;
use crate::monitor::{other_name, LiveSample, OTHER_KEY};
use crate::net;
use crate::ports::{self, UNCLASSIFIED};
use crate::store::{Bucket, Data, HOUR, MINUTE};

/// Host-column key for bytes no connection accounts for.
pub const NO_HOST: &str = "__nohost";
/// Publishers-mode key for apps without a publisher.
pub const NO_PUBLISHER: &str = "__nopublisher";
const NO_HOST_LABEL: &str = "Other traffic (no host)";
const NO_PUBLISHER_LABEL: &str = "Unknown publisher";
/// Minute buckets kept by the store (one day).
const MINUTES_KEPT: i64 = 24 * 60;
const DAY: i64 = 24 * HOUR;
/// Preset spans: 5m, 3h, 24h, 7d, 30d as (step, steps).
const PRESETS: [(i64, i64); 5] = [(1, 300), (MINUTE, 180), (MINUTE, 1440), (HOUR, 168), (HOUR, 720)];
/// Graph tooltip: contributors kept per step.
const STEP_TOP: usize = 4;
/// Hosts rows: app keys kept per row.
const HOST_APPS: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Group {
    Apps,
    Publishers,
    Types,
    Countries,
}

impl Group {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Publishers,
            2 => Self::Types,
            3 => Self::Countries,
            _ => Self::Apps,
        }
    }
    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Net {
    All,
    Lan,
    Wan,
}

impl Net {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Lan,
            2 => Self::Wan,
            _ => Self::All,
        }
    }
    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

/// Direction filter. `In` is download, `Out` is upload.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DirF {
    Both,
    In,
    Out,
}

impl DirF {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::In,
            2 => Self::Out,
            _ => Self::Both,
        }
    }
    pub fn to_u8(self) -> u8 {
        self as u8
    }
    /// Multiplier per side: 1 keeps the side, 0 zeroes it.
    fn mask(self) -> [u64; 2] {
        match self {
            Self::Both => [1, 1],
            Self::In => [1, 0],
            Self::Out => [0, 1],
        }
    }
}

/// Group by and network filter actually used. Without administrator rights there are no flows,
/// so the admin-only choices fall back to Apps and WAN & LAN (the saved values are untouched).
pub fn effective(group: Group, net: Net, admin: bool) -> (Group, Net) {
    if admin { (group, net) } else { (Group::Apps, Net::All) }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Sel {
    All,
    App(String),
    Group(Group, String),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Span {
    /// 0 = 5m, 1 = 3h, 2 = 24h, 3 = 7d, 4 = 30d.
    Preset(u8),
    /// Local midnight (unix) of the day.
    Day(i64),
    /// Monday's local midnight.
    Week(i64),
    /// The 1st's local midnight.
    Month(i64),
    AllTime,
    /// `[from, to)`, local midnights.
    Custom(i64, i64),
}

/// Where a window's steps come from.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Source {
    Live,
    Minutes,
    Hours,
}

#[derive(Clone, Copy)]
enum Cal {
    Day,
    Week,
    Month,
}

fn local_date(t: i64) -> NaiveDate {
    match Local.timestamp_opt(t, 0).earliest() {
        Some(dt) => dt.date_naive(),
        None => chrono::DateTime::from_timestamp(t, 0).unwrap_or_default().date_naive(),
    }
}

/// Unix time of `d`'s local midnight (or the first valid hour after it, when a DST change
/// skips midnight).
fn midnight_of(d: NaiveDate) -> i64 {
    for h in 0..3 {
        if let Some(dt) = d.and_hms_opt(h, 0, 0).and_then(|n| Local.from_local_datetime(&n).earliest()) {
            return dt.timestamp();
        }
    }
    d.and_hms_opt(0, 0, 0).map(|n| n.and_utc().timestamp()).unwrap_or_default()
}

/// Local midnight that starts the day containing `t`.
pub fn local_midnight(t: i64) -> i64 {
    midnight_of(local_date(t))
}

fn cal_start(c: Cal, t: i64) -> i64 {
    let d = local_date(t);
    midnight_of(match c {
        Cal::Day => d,
        Cal::Week => d - chrono::Duration::days(i64::from(d.weekday().num_days_from_monday())),
        Cal::Month => d.with_day(1).unwrap_or(d),
    })
}

/// End (exclusive) of the period that starts at `start`.
fn cal_end(c: Cal, start: i64) -> i64 {
    let d = local_date(start);
    let next = match c {
        Cal::Day => d.succ_opt(),
        Cal::Week => d.checked_add_signed(chrono::Duration::days(7)),
        Cal::Month => {
            let (y, m) = if d.month() == 12 { (d.year() + 1, 1) } else { (d.year(), d.month() + 1) };
            NaiveDate::from_ymd_opt(y, m, 1)
        }
    };
    next.map(midnight_of).unwrap_or(start + DAY)
}

fn cal_of(span: Span) -> Option<(Cal, i64)> {
    match span {
        Span::Day(s) => Some((Cal::Day, s)),
        Span::Week(s) => Some((Cal::Week, s)),
        Span::Month(s) => Some((Cal::Month, s)),
        _ => None,
    }
}

fn source_list(data: &Data, src: Source) -> &VecDeque<Bucket> {
    if src == Source::Hours { &data.hours } else { &data.minutes }
}

/// The span's window as `(start, end, step, source)`, `end` exclusive, with no `end_offset`.
///
/// Presets end at the current step and always have their full length (missing steps are zero).
/// Day, Week, Month, All Time and Custom read minute buckets when the window is at most 24 hours
/// and lies inside minute retention, otherwise hour buckets; they are clipped to the stored
/// buckets, so `start >= end` means there is nothing to show.
pub fn span_window(span: Span, now: i64, data: &Data) -> (i64, i64, i64, Source) {
    if let Span::Preset(p) = span {
        let p = usize::from(p.min(4));
        let (step, count) = PRESETS[p];
        let src = match p {
            0 => Source::Live,
            1 | 2 => Source::Minutes,
            _ => Source::Hours,
        };
        let end = now - now.rem_euclid(step) + step;
        return (end - count * step, end, step, src);
    }
    let oldest = [&data.minutes, &data.hours].iter().filter_map(|l| l.front().map(|b| b.t)).min().unwrap_or(now);
    let (s, e) = match span {
        Span::Day(s) => (s, cal_end(Cal::Day, s)),
        Span::Week(s) => (s, cal_end(Cal::Week, s)),
        Span::Month(s) => (s, cal_end(Cal::Month, s)),
        Span::AllTime => (oldest, now + 1),
        Span::Custom(a, b) => (a, b),
        Span::Preset(_) => unreachable!(),
    };
    let minute_floor = now - now.rem_euclid(MINUTE) - (MINUTES_KEPT - 1) * MINUTE;
    let (step, src) = if e - s <= DAY && s >= minute_floor { (MINUTE, Source::Minutes) } else { (HOUR, Source::Hours) };
    let list = source_list(data, src);
    let (Some(first), Some(last)) = (list.front(), list.back()) else { return (s, s, step, src) };
    let start = s.max(first.t);
    let start = start - start.rem_euclid(step);
    let end = e.min(last.t + step);
    (start, end.max(start), step, src)
}

/// Moves a Day, Week or Month span to the previous (`dir < 0`) or next (`dir > 0`) period that
/// has stored buckets. It never moves past the current period, and stays put when there is no
/// such period. Other spans are returned unchanged.
pub fn span_step(span: Span, dir: i8, data: &Data) -> Span {
    let Some((cal, start)) = cal_of(span) else { return span };
    let make = |s: i64| match cal {
        Cal::Day => Span::Day(s),
        Cal::Week => Span::Week(s),
        Cal::Month => Span::Month(s),
    };
    let current = cal_start(cal, crate::store::now());
    let lists = [&data.minutes, &data.hours];
    if dir < 0 {
        let newest_before = lists
            .iter()
            .filter_map(|l| {
                let i = l.partition_point(|b| b.t < start);
                i.checked_sub(1).map(|i| l[i].t)
            })
            .max();
        match newest_before {
            Some(t) => make(cal_start(cal, t).min(current)),
            None => span,
        }
    } else if dir > 0 {
        let end = cal_end(cal, start);
        let oldest_after = lists
            .iter()
            .filter_map(|l| {
                let i = l.partition_point(|b| b.t < end);
                l.get(i).map(|b| b.t)
            })
            .min();
        match oldest_after {
            Some(t) => make(cal_start(cal, t).min(current).max(start)),
            None => span,
        }
    } else {
        span
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Query {
    pub span: Span,
    pub group: Group,
    pub net: Net,
    pub dir: DirF,
    pub sel: Sel,
    pub admin: bool,
    /// Steps back from now; 0 = live. Only for Preset spans.
    pub end_offset: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Row {
    pub key: String,
    pub label: String,
    pub down: u64,
    pub up: u64,
    pub cc: Option<String>,
    pub icon_key: Option<String>,
    /// Hosts rows: up to 3 app keys, most bytes first.
    pub apps: Vec<String>,
    /// Hosts rows: apps beyond `apps`.
    pub more_apps: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Agg {
    pub times: Vec<i64>,
    pub step: i64,
    /// Bytes per step.
    pub down: Vec<f64>,
    pub up: Vec<f64>,
    /// Per step, the active group's top 4 row keys with their bytes.
    pub step_top: Vec<Vec<(String, [u64; 2])>>,
    /// List rows for the active Group by (not narrowed by the selection).
    pub rows: Vec<Row>,
    /// Usage columns, narrowed by the selection.
    pub hosts: Vec<Row>,
    pub types: Vec<Row>,
    pub countries: Vec<Row>,
    pub total: [u64; 2],
    /// Strip rows: follow selection and direction, ignore the network filter.
    pub lan: [u64; 2],
    pub wan: [u64; 2],
    /// The whole stored history for the range unit, as (t, down + up), for the scrubber.
    pub history: Vec<(i64, f64)>,
    /// Largest useful `end_offset` (Preset spans only).
    pub max_offset: usize,
}

/// FxHash-style hasher for the interner (std's SipHash is several times slower on short keys,
/// and the map lives for one call only).
#[derive(Default, Clone, Copy)]
struct Fx(u64);

impl Fx {
    #[inline]
    fn add(&mut self, w: u64) {
        self.0 = (self.0.rotate_left(5) ^ w).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

impl Hasher for Fx {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add(u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut b = [0u8; 8];
            b[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(b));
        }
    }
    #[inline]
    fn write_u8(&mut self, n: u8) {
        self.add(u64::from(n));
    }
    #[inline]
    fn write_usize(&mut self, n: usize) {
        self.add(n as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        // The multiply leaves the low bits weak, and hashbrown indexes with them.
        self.0.rotate_left(26)
    }
}

type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<Fx>>;

#[inline]
fn add2(a: &mut [u64; 2], b: [u64; 2]) {
    a[0] = a[0].saturating_add(b[0]);
    a[1] = a[1].saturating_add(b[1]);
}

#[inline]
fn sub2(a: [u64; 2], b: [u64; 2]) -> [u64; 2] {
    [a[0].saturating_sub(b[0]), a[1].saturating_sub(b[1])]
}

#[inline]
fn max2(a: [u64; 2], b: [u64; 2]) -> [u64; 2] {
    [a[0].max(b[0]), a[1].max(b[1])]
}

#[inline]
fn mul2(a: [u64; 2], m: [u64; 2]) -> [u64; 2] {
    [a[0] * m[0], a[1] * m[1]]
}

#[inline]
fn nonzero(v: [u64; 2]) -> bool {
    v != [0, 0]
}

/// Strings seen by one `aggregate` call, as dense ids, so per-flow work indexes arrays instead
/// of hashing strings. A bucket's string table is resolved once per entry it uses.
struct Interner<'a> {
    ids: FxMap<&'a str, u32>,
    strs: Vec<&'a str>,
}

// Fixed ids, interned first.
const ID_UNCL: u32 = 0;
const ID_OTHER: u32 = 1;
const ID_NO_HOST: u32 = 2;
const ID_NO_PUB: u32 = 3;
const ID_EMPTY: u32 = 4;
const ID_LAN: u32 = 5;
const NONE: u32 = u32::MAX;

impl<'a> Interner<'a> {
    fn new() -> Self {
        let mut ix = Self { ids: FxMap::default(), strs: Vec::new() };
        for s in [UNCLASSIFIED, OTHER_KEY, NO_HOST, NO_PUBLISHER, "", geo::LAN] {
            ix.id(s);
        }
        ix
    }

    #[inline]
    fn id(&mut self, s: &'a str) -> u32 {
        if let Some(&i) = self.ids.get(s) {
            return i;
        }
        let i = self.strs.len() as u32;
        self.strs.push(s);
        self.ids.insert(s, i);
        i
    }

    #[inline]
    fn str(&self, i: u32) -> &'a str {
        self.strs[i as usize]
    }
}

/// How many entries ahead the bucket readers prefetch.
const AHEAD: usize = 6;

/// Hints the CPU to start loading `p`. Each stored string has its own heap allocation, so on
/// 30 days (megabytes of buckets, mostly not in cache) resolving strings in order waits on one
/// cache miss after another; asking for the string `AHEAD` entries early overlaps those misses.
#[inline(always)]
fn prefetch(p: *const u8) {
    // SAFETY: a prefetch is only a hint; it never faults and reads nothing the program sees.
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::x86_64::_mm_prefetch(p.cast::<i8>(), std::arch::x86_64::_MM_HINT_T0);
    }
    #[cfg(not(target_arch = "x86_64"))]
    let _ = p;
}

/// Grows `v` as needed and returns slot `i`.
#[inline]
fn slot<T: Default + Clone>(v: &mut Vec<T>, i: u32) -> &mut T {
    let i = i as usize;
    if i >= v.len() {
        v.resize(i + 1, T::default());
    }
    &mut v[i]
}

/// One app's bytes in one step, of one host, type and country (interned ids). Residual atoms
/// use `UNCLASSIFIED` for all three and count as WAN.
#[derive(Clone, Copy)]
struct Atom {
    app: u32,
    h: u32,
    ty: u32,
    cc: u32,
    v: [u64; 2],
    lan: bool,
}

impl Atom {
    fn residual(app: u32, v: [u64; 2], lan: bool) -> Self {
        Self { app, h: ID_UNCL, ty: ID_UNCL, cc: ID_UNCL, v, lan }
    }
}

/// One step's raw inputs (no filters applied).
#[derive(Default)]
struct Step<'a> {
    flows: Vec<Atom>,
    residual: Vec<Atom>,
    /// Σ over apps of max(apps[k], Σ flows[k]), plus unattributed bytes beyond the apps.
    total: [u64; 2],
    /// The bucket's exact LAN bytes (live samples: Σ LAN flows).
    lan: [u64; 2],
    hosts: Option<&'a HashMap<String, [u64; 2]>>,
    /// Bucket string index → id.
    local: Vec<u32>,
    /// The bucket's `flows` and `apps` entries, gathered so their keys can be prefetched.
    flow_lists: Vec<(&'a String, &'a Vec<[u64; 5]>)>,
    app_totals: Vec<(&'a String, [u64; 2])>,
    /// Per app id: Σ flows this step, and which ids were touched.
    fsum: Vec<[u64; 2]>,
    touched: Vec<u32>,
}

impl<'a> Step<'a> {
    fn clear(&mut self) {
        self.flows.clear();
        self.residual.clear();
        self.total = [0, 0];
        self.lan = [0, 0];
        self.hosts = None;
    }

    /// Same entries as `Bucket::flows_of` (out-of-range indices skipped, so their bytes become
    /// the app's residual), read in place instead of cloned to keep 30 days within budget.
    fn fill_bucket(&mut self, b: &'a Bucket, ix: &mut Interner<'a>) {
        self.clear();
        // The whole table in order: nearly every entry is used, and walking it in order is
        // much kinder to the cache than resolving entries in flow order.
        self.local.clear();
        for (i, s) in b.strs.iter().enumerate() {
            if let Some(n) = b.strs.get(i + AHEAD) {
                prefetch(n.as_ptr());
            }
            self.local.push(ix.id(s));
        }
        let mut lists = std::mem::take(&mut self.flow_lists);
        lists.clear();
        lists.extend(&b.flows);
        let local = &self.local;
        let res = |i: u64| usize::try_from(i).ok().and_then(|i| local.get(i).copied());
        for (i, &(app, list)) in lists.iter().enumerate() {
            if let Some((k, l)) = lists.get(i + AHEAD) {
                prefetch(k.as_ptr());
                prefetch(l.as_ptr().cast());
            }
            let app = ix.id(app);
            let mut sum = [0, 0];
            for e in list {
                let (Some(h), Some(ty), Some(cc)) = (res(e[0]), res(e[1]), res(e[2])) else { continue };
                let v = [e[3], e[4]];
                add2(&mut sum, v);
                self.flows.push(Atom { app, h, ty, cc, v, lan: cc == ID_LAN });
            }
            add2(slot(&mut self.fsum, app), sum);
            self.touched.push(app);
        }
        self.flow_lists = lists;
        let mut totals = std::mem::take(&mut self.app_totals);
        totals.clear();
        totals.extend(b.apps.iter().map(|(k, a)| (k, *a)));
        for (i, &(app, a)) in totals.iter().enumerate() {
            if let Some((k, _)) = totals.get(i + AHEAD) {
                prefetch(k.as_ptr());
            }
            let app = ix.id(app);
            self.push_app(app, a);
        }
        self.app_totals = totals;
        self.finish([b.down, b.up]);
        self.lan = [b.lan_down, b.lan_up];
        self.hosts = Some(&b.hosts);
    }

    fn fill_live(&mut self, l: &'a LiveSample, ix: &mut Interner<'a>) {
        self.clear();
        for (app, f) in &l.flows {
            let app = ix.id(app);
            let (h, ty, cc) = (ix.id(&f.h), ix.id(&f.ty), ix.id(&f.cc));
            let v = [f.d, f.u];
            let lan = cc == ID_LAN;
            if lan {
                add2(&mut self.lan, v);
            }
            add2(slot(&mut self.fsum, app), v);
            self.touched.push(app);
            self.flows.push(Atom { app, h, ty, cc, v, lan });
        }
        for (app, a) in &l.apps {
            let app = ix.id(app);
            self.push_app(app, *a);
        }
        self.finish([l.down, l.up]);
    }

    /// Adds an app's total; the part its flows don't cover is its residual.
    fn push_app(&mut self, app: u32, a: [u64; 2]) {
        let sum = std::mem::take(slot(&mut self.fsum, app));
        let r = sub2(a, sum);
        if nonzero(r) {
            self.residual.push(Atom::residual(app, r, false));
        }
        add2(&mut self.total, max2(a, sum));
    }

    /// Counts flows of apps missing from the app totals, then treats interface bytes beyond the
    /// per-app totals as unattributed (`__other`).
    fn finish(&mut self, iface: [u64; 2]) {
        for i in std::mem::take(&mut self.touched) {
            let sum = std::mem::take(slot(&mut self.fsum, i));
            add2(&mut self.total, sum);
        }
        let extra = sub2(iface, self.total);
        if nonzero(extra) {
            self.residual.push(Atom::residual(ID_OTHER, extra, false));
            add2(&mut self.total, extra);
        }
    }
}

/// One host's flows in the current step (valid when `stamp` matches).
#[derive(Clone, Copy, Default)]
struct Split {
    stamp: u32,
    sum: [u64; 2],
    head: u32,
}

#[derive(Clone, Default)]
struct HostCell {
    cc: Option<u32>,
    v: [u64; 2],
    apps: Vec<(u32, u64)>,
}

/// Running sums for one `aggregate` call, indexed by interned id.
struct Acc<'a> {
    data: &'a Data,
    ix: Interner<'a>,
    group: Group,
    net: Net,
    /// `None`: no selection. Otherwise the group it filters on and the target id.
    sel: Option<(Group, u32)>,
    mask: [u64; 2],
    /// App id → publisher id + 1 (0 until looked up).
    publishers: Vec<u32>,
    rows: Vec<[u64; 2]>,
    /// Publishers mode: bytes per app, to pick each publisher's icon.
    app_bytes: Vec<[u64; 2]>,
    types: Vec<[u64; 2]>,
    countries: Vec<[u64; 2]>,
    /// Host id → one cell per country (`None`: from the `hosts` map only).
    hosts: Vec<Vec<HostCell>>,
    step_rows: Vec<[u64; 2]>,
    step_touched: Vec<u32>,
    /// Host id → this step's flows of that host.
    split: Vec<Split>,
    /// Flow index → next flow index of the same host (`NONE` ends the chain).
    next: Vec<u32>,
    stamp: u32,
    /// The current bucket's `hosts` map, gathered so its keys can be prefetched.
    map_hosts: Vec<(&'a String, [u64; 2])>,
    view: Vec<Atom>,
    total: [u64; 2],
    lan: [u64; 2],
    wan: [u64; 2],
}

fn ip_is_lan(host: &str) -> bool {
    host.parse::<IpAddr>().is_ok_and(|ip| net::is_private(&ip))
}

impl<'a> Acc<'a> {
    fn new(data: &'a Data, q: &'a Query) -> Self {
        let (group, net) = effective(q.group, q.net, q.admin);
        let mut ix = Interner::new();
        let sel = match &q.sel {
            Sel::All => None,
            Sel::App(k) => Some((Group::Apps, ix.id(k))),
            Sel::Group(g, k) => Some((*g, ix.id(k))),
        };
        Self {
            data,
            ix,
            group,
            net,
            sel,
            mask: q.dir.mask(),
            publishers: Vec::new(),
            rows: Vec::new(),
            app_bytes: Vec::new(),
            types: Vec::new(),
            countries: Vec::new(),
            hosts: Vec::new(),
            step_rows: Vec::new(),
            step_touched: Vec::new(),
            split: Vec::new(),
            next: Vec::new(),
            stamp: 0,
            map_hosts: Vec::new(),
            view: Vec::new(),
            total: [0, 0],
            lan: [0, 0],
            wan: [0, 0],
        }
    }

    fn net_ok(&self, lan: bool) -> bool {
        match self.net {
            Net::All => true,
            Net::Lan => lan,
            Net::Wan => !lan,
        }
    }

    fn publisher(&mut self, app: u32) -> u32 {
        if app == ID_OTHER || app == ID_UNCL {
            return app;
        }
        let known = *slot(&mut self.publishers, app);
        if known != 0 {
            return known - 1;
        }
        let data = self.data;
        let p = match data.apps.get(self.ix.str(app)).and_then(|r| r.publisher.as_deref()).filter(|p| !p.trim().is_empty()) {
            Some(p) => self.ix.id(p),
            None => ID_NO_PUB,
        };
        *slot(&mut self.publishers, app) = p + 1;
        p
    }

    fn key_of(&mut self, group: Group, a: &Atom) -> u32 {
        match group {
            Group::Apps => a.app,
            Group::Publishers => self.publisher(a.app),
            Group::Types => a.ty,
            Group::Countries => a.cc,
        }
    }

    fn selected(&mut self, a: &Atom) -> bool {
        match self.sel {
            None => true,
            Some((g, id)) => self.key_of(g, a) == id,
        }
    }

    /// The step's atoms after the network filter, the selection (`all` = ignore it) and the
    /// direction mask, into `out`. With no selection, a network filter swaps the per-app
    /// residuals for the bucket-level LAN or WAN residual, so rows match the exact bucket LAN.
    fn view(&mut self, st: &Step<'a>, all: bool, out: &mut Vec<Atom>) {
        out.clear();
        let mask = self.mask;
        let push = |out: &mut Vec<Atom>, a: Atom| {
            let v = mul2(a.v, mask);
            if nonzero(v) {
                out.push(Atom { v, ..a });
            }
        };
        if all {
            match self.net {
                Net::All => {
                    for a in st.flows.iter().chain(&st.residual) {
                        push(out, *a);
                    }
                }
                Net::Lan | Net::Wan => {
                    let lan = self.net == Net::Lan;
                    let mut flows = [0, 0];
                    for a in st.flows.iter().filter(|a| a.lan == lan) {
                        add2(&mut flows, a.v);
                        push(out, *a);
                    }
                    let kind = if lan { st.lan } else { sub2(st.total, st.lan) };
                    push(out, Atom::residual(ID_UNCL, sub2(kind, flows), lan));
                }
            }
        } else {
            for a in st.flows.iter().chain(&st.residual) {
                if self.net_ok(a.lan) && self.selected(a) {
                    push(out, *a);
                }
            }
        }
    }

    fn add_host(&mut self, h: u32, cc: Option<u32>, app: Option<u32>, v: [u64; 2]) {
        let cells = slot(&mut self.hosts, h);
        let cell = match cells.iter().position(|c| c.cc == cc) {
            Some(i) => &mut cells[i],
            None => {
                cells.push(HostCell { cc, ..Default::default() });
                cells.last_mut().unwrap()
            }
        };
        add2(&mut cell.v, v);
        if let Some(app) = app {
            let b = v[0].saturating_add(v[1]);
            // Most hosts see one app; check the first entry before scanning.
            match cell.apps.first_mut() {
                Some(x) if x.0 == app => x.1 = x.1.saturating_add(b),
                _ => match cell.apps.iter_mut().find(|x| x.0 == app) {
                    Some(x) => x.1 = x.1.saturating_add(b),
                    None => cell.apps.push((app, b)),
                },
            }
        }
    }

    fn host_atom(&mut self, a: &Atom) {
        if a.h == ID_UNCL {
            self.add_host(ID_UNCL, None, None, a.v);
        } else if a.h == ID_EMPTY {
            self.add_host(ID_NO_HOST, None, Some(a.app), a.v);
        } else {
            self.add_host(a.h, Some(a.cc), Some(a.app), a.v);
        }
    }

    /// No selection, a bucket with a `hosts` map: the map gives each host's bytes; flows split a
    /// host by country when they account for no more than the map does, and any rest of the
    /// host stays as a row without a country.
    fn hosts_from_map(&mut self, st: &Step<'a>, map: &'a HashMap<String, [u64; 2]>) {
        let mask = self.mask;
        self.stamp += 1;
        let stamp = self.stamp;
        // Per host: this step's flow bytes and a chain through `next` of its flow indices.
        self.next.clear();
        self.next.resize(st.flows.len(), NONE);
        for (i, a) in st.flows.iter().enumerate() {
            let v = mul2(a.v, mask);
            if a.h != ID_EMPTY && nonzero(v) {
                let e = slot(&mut self.split, a.h);
                if e.stamp != stamp {
                    *e = Split { stamp, sum: [0, 0], head: NONE };
                }
                add2(&mut e.sum, v);
                self.next[i] = e.head;
                e.head = i as u32;
            }
        }
        let mut entries = std::mem::take(&mut self.map_hosts);
        entries.clear();
        entries.extend(map.iter().map(|(h, hv)| (h, mul2(*hv, mask))).filter(|(_, hv)| nonzero(*hv)));
        for (n, &(h, hv)) in entries.iter().enumerate() {
            if let Some((k, _)) = entries.get(n + AHEAD) {
                prefetch(k.as_ptr());
            }
            let id = self.ix.id(h);
            let e = *slot(&mut self.split, id);
            let (rest, mut i) = if e.stamp == stamp && e.sum[0] <= hv[0] && e.sum[1] <= hv[1] { (sub2(hv, e.sum), e.head) } else { (hv, NONE) };
            if nonzero(rest) && (self.net == Net::All || self.net_ok(ip_is_lan(h))) {
                self.add_host(id, None, None, rest);
            }
            while i != NONE {
                let a = st.flows[i as usize];
                if self.net_ok(a.lan) {
                    self.add_host(a.h, Some(a.cc), Some(a.app), mul2(a.v, mask));
                }
                i = self.next[i as usize];
            }
        }
        self.map_hosts = entries;
    }

    fn row_atom(&mut self, a: &Atom) {
        let k = self.key_of(self.group, a);
        let e = slot(&mut self.step_rows, k);
        if !nonzero(*e) {
            self.step_touched.push(k);
        }
        add2(e, a.v);
        if self.group == Group::Publishers && a.app != ID_OTHER && a.app != ID_UNCL {
            add2(slot(&mut self.app_bytes, a.app), a.v);
        }
    }

    fn column_atom(&mut self, a: &Atom, col: &mut [u64; 2]) {
        add2(col, a.v);
        add2(slot(&mut self.types, a.ty), a.v);
        add2(slot(&mut self.countries, a.cc), a.v);
    }

    /// Adds one step; returns its graph column and its top contributors.
    fn step(&mut self, st: &Step<'a>) -> ([u64; 2], Vec<(String, [u64; 2])>) {
        let all = self.sel.is_none();
        // No filter at all: the raw atoms are the view (saves a copy per step).
        let raw = all && self.net == Net::All && self.mask == [1, 1];
        let mut view = std::mem::take(&mut self.view);

        // List rows (never narrowed by the selection).
        if raw {
            for a in st.flows.iter().chain(&st.residual) {
                self.row_atom(a);
            }
        } else {
            self.view(st, true, &mut view);
            for a in &view {
                self.row_atom(a);
            }
        }
        let mut top: Vec<(u32, [u64; 2])> = Vec::with_capacity(self.step_touched.len());
        for k in std::mem::take(&mut self.step_touched) {
            let v = std::mem::take(&mut self.step_rows[k as usize]);
            add2(slot(&mut self.rows, k), v);
            top.push((k, v));
        }
        let ix = &self.ix;
        top.sort_by(|a, b| (b.1[0] + b.1[1]).cmp(&(a.1[0] + a.1[1])).then_with(|| ix.str(a.0).cmp(ix.str(b.0))));
        top.truncate(STEP_TOP);
        let top = top.into_iter().map(|(k, v)| (ix.str(k).to_string(), v)).collect();

        // Columns, narrowed by the selection.
        if !all {
            self.view(st, false, &mut view);
        }
        let mut col = [0, 0];
        if raw {
            for a in st.flows.iter().chain(&st.residual) {
                self.column_atom(a, &mut col);
            }
        } else {
            for a in &view {
                self.column_atom(a, &mut col);
            }
        }
        match st.hosts {
            Some(map) if all => self.hosts_from_map(st, map),
            _ if raw => {
                for a in st.flows.iter().chain(&st.residual) {
                    self.host_atom(a);
                }
            }
            _ => {
                for a in &view {
                    self.host_atom(a);
                }
            }
        }
        add2(&mut self.total, col);

        // Strip: LAN and WAN follow the selection and direction, not the network filter.
        if all {
            let lan = mul2(st.lan, self.mask);
            add2(&mut self.lan, lan);
            add2(&mut self.wan, sub2(mul2(st.total, self.mask), lan));
        } else {
            for a in st.flows.iter().chain(&st.residual) {
                if self.selected(a) {
                    let v = mul2(a.v, self.mask);
                    add2(if a.lan { &mut self.lan } else { &mut self.wan }, v);
                }
            }
        }
        self.view = view;
        (col, top)
    }

    fn row_label(&self, key: &str) -> String {
        match key {
            UNCLASSIFIED => ports::label(UNCLASSIFIED),
            OTHER_KEY if matches!(self.group, Group::Apps | Group::Publishers) => other_name(true).to_string(),
            _ => match self.group {
                Group::Apps => app_label(self.data, key),
                Group::Publishers if key == NO_PUBLISHER => NO_PUBLISHER_LABEL.to_string(),
                Group::Publishers => key.to_string(),
                Group::Types => ports::label(key),
                Group::Countries => geo::name(key),
            },
        }
    }

    fn finish(mut self, agg: &mut Agg) {
        // Publishers: each one's icon is its app with the most bytes.
        let mut icons: Vec<(u32, u64)> = Vec::new();
        if self.group == Group::Publishers {
            for app in 0..self.app_bytes.len() as u32 {
                let v = self.app_bytes[app as usize];
                if !nonzero(v) {
                    continue;
                }
                let b = v[0] + v[1];
                let p = self.publisher(app);
                let e = slot(&mut icons, p);
                if e.0 == 0 && e.1 == 0 || b > e.1 || (b == e.1 && self.ix.str(app) < self.ix.str(e.0)) {
                    *e = (app, b);
                }
            }
        }
        let ix = &self.ix;
        let mut rows = Vec::new();
        for (k, v) in self.rows.iter().enumerate() {
            if !nonzero(*v) {
                continue;
            }
            let id = k as u32;
            let key = ix.str(id);
            let icon_key = match self.group {
                _ if id == ID_OTHER || id == ID_UNCL => None,
                Group::Apps => Some(key.to_string()),
                Group::Publishers => icons.get(k).filter(|e| e.1 > 0).map(|e| ix.str(e.0).to_string()),
                Group::Types | Group::Countries => None,
            };
            let cc = (self.group == Group::Countries && id != ID_UNCL).then(|| key.to_string());
            rows.push(Row { key: key.to_string(), label: self.row_label(key), down: v[0], up: v[1], cc, icon_key, ..Default::default() });
        }
        agg.rows = sorted(rows);
        let named = |v: &[[u64; 2]]| -> Vec<(&'a str, [u64; 2])> { v.iter().enumerate().filter(|(_, v)| nonzero(**v)).map(|(k, v)| (ix.str(k as u32), *v)).collect() };
        agg.types = sorted(named(&self.types).into_iter().map(|(k, v)| Row { key: k.to_string(), label: ports::label(k), down: v[0], up: v[1], ..Default::default() }).collect());
        agg.countries = sorted(
            named(&self.countries)
                .into_iter()
                .map(|(k, v)| {
                    let (label, cc) = if k == UNCLASSIFIED { (ports::label(UNCLASSIFIED), None) } else { (geo::name(k), Some(k.to_string())) };
                    Row { key: k.to_string(), label, down: v[0], up: v[1], cc, ..Default::default() }
                })
                .collect(),
        );

        // Hosts: whatever no host row accounts for goes to "Other traffic (no host)".
        let mut listed = [0, 0];
        for c in self.hosts.iter().flatten() {
            add2(&mut listed, c.v);
        }
        let rest = sub2(self.total, listed);
        if nonzero(rest) {
            self.add_host(ID_NO_HOST, None, None, rest);
        }
        let ix = &self.ix;
        let mut hosts = Vec::new();
        for (h, cells) in self.hosts.iter_mut().enumerate() {
            let h = h as u32;
            for c in cells.iter_mut() {
                c.apps.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| ix.str(a.0).cmp(ix.str(b.0))));
                let more_apps = c.apps.len().saturating_sub(HOST_APPS);
                let apps = c.apps.iter().take(HOST_APPS).map(|(a, _)| ix.str(*a).to_string()).collect();
                let cc = c.cc.map(|i| ix.str(i).to_string());
                let (key, label) = match h {
                    ID_NO_HOST => (NO_HOST.to_string(), NO_HOST_LABEL.to_string()),
                    ID_UNCL => (UNCLASSIFIED.to_string(), ports::label(UNCLASSIFIED)),
                    _ => (format!("{}|{}", ix.str(h), cc.as_deref().unwrap_or("")), ix.str(h).to_string()),
                };
                hosts.push(Row { key, label, down: c.v[0], up: c.v[1], cc, apps, more_apps, ..Default::default() });
            }
        }
        agg.hosts = sorted(hosts);
        agg.total = self.total;
        agg.lan = self.lan;
        agg.wan = self.wan;
    }
}

/// Display name of an app key: its recorded name, else the file name.
fn app_label(data: &Data, key: &str) -> String {
    match data.apps.get(key).map(|r| r.name.as_str()).filter(|n| !n.is_empty()) {
        Some(n) => n.to_string(),
        None => key.rsplit(['\\', ':']).next().unwrap_or(key).to_string(),
    }
}

/// Non-empty rows, most bytes first, then by label and key.
fn sorted(mut rows: Vec<Row>) -> Vec<Row> {
    rows.retain(|r| r.down != 0 || r.up != 0);
    rows.sort_by(|a, b| (b.down + b.up).cmp(&(a.down + a.up)).then_with(|| a.label.cmp(&b.label)).then_with(|| a.key.cmp(&b.key)));
    rows
}

/// The newest live second under `q`'s selection, network and direction: the strip's current
/// speed. Kept out of the cached `Agg` (whose key needn't change on an idle second) and cheap
/// enough to call every frame: it reads one live sample, never the history.
pub fn live_rate(data: &Data, live: &VecDeque<LiveSample>, q: &Query) -> [u64; 2] {
    let Some(l) = live.back() else { return [0, 0] };
    let mut acc = Acc::new(data, q);
    let mut st = Step::default();
    st.fill_live(l, &mut acc.ix);
    let mut view = Vec::new();
    acc.view(&st, acc.sel.is_none(), &mut view);
    view.iter().fold([0, 0], |mut s, a| {
        add2(&mut s, a.v);
        s
    })
}

/// Everything the Traffic page shows for `q`. `now` is the newest live sample's time (or the
/// clock when there is none).
pub fn aggregate(data: &Data, live: &VecDeque<LiveSample>, q: &Query, now: i64) -> Agg {
    run(data, live, q, now)
}

fn run<'a>(data: &'a Data, live: &'a VecDeque<LiveSample>, q: &'a Query, now: i64) -> Agg {
    let (mut start, mut end, step, src) = span_window(q.span, now, data);
    let mut agg = Agg { step, ..Default::default() };

    // Scrubber history and how far back the window may go.
    let oldest = match src {
        Source::Live => {
            agg.history = live.iter().map(|l| (l.t, (l.down + l.up) as f64)).collect();
            live.front().map(|l| l.t)
        }
        _ => {
            let list = source_list(data, src);
            agg.history = list.iter().map(|b| (b.t, (b.down + b.up) as f64)).collect();
            list.front().map(|b| b.t)
        }
    };
    if matches!(q.span, Span::Preset(_)) {
        agg.max_offset = oldest.map_or(0, |o| usize::try_from((start - o) / step).unwrap_or(0));
        let off = q.end_offset.min(agg.max_offset) as i64;
        start -= off * step;
        end -= off * step;
    }

    let mut acc = Acc::new(data, q);
    let mut st = Step::default();

    if start >= end {
        return agg;
    }
    let n = usize::try_from((end - start + step - 1) / step).unwrap_or(0);
    agg.times.reserve(n);
    agg.down.reserve(n);
    agg.up.reserve(n);
    agg.step_top.reserve(n);
    let list = source_list(data, src);
    let mut li = live.partition_point(|l| l.t < start);
    let mut bi = list.partition_point(|b| b.t < start);
    let mut t = start;
    while t < end {
        let filled = match src {
            Source::Live => {
                while li < live.len() && live[li].t < t {
                    li += 1;
                }
                match live.get(li).filter(|l| l.t == t) {
                    Some(l) => {
                        st.fill_live(l, &mut acc.ix);
                        true
                    }
                    None => false,
                }
            }
            _ => {
                while bi < list.len() && list[bi].t < t {
                    bi += 1;
                }
                match list.get(bi).filter(|b| b.t == t) {
                    Some(b) => {
                        st.fill_bucket(b, &mut acc.ix);
                        true
                    }
                    None => false,
                }
            }
        };
        let (col, top) = if filled { acc.step(&st) } else { ([0, 0], Vec::new()) };
        agg.times.push(t);
        agg.down.push(col[0] as f64);
        agg.up.push(col[1] as f64);
        agg.step_top.push(top);
        t += step;
    }
    acc.finish(&mut agg);
    agg
}

/// `(query, newest t, newest bytes)`: 5 minutes keys on the newest live second; bucket spans on
/// the newest bucket of their source and its down + up, so they don't redo history every second.
pub type CacheKey = (Query, i64, u64);

pub fn cache_key(q: &Query, data: &Data, live: &VecDeque<LiveSample>, now: i64) -> CacheKey {
    match span_window(q.span, now, data).3 {
        Source::Live => (q.clone(), live.back().map_or(0, |l| l.t), 0),
        src => {
            let b = source_list(data, src).back();
            (q.clone(), b.map_or(0, |b| b.t), b.map_or(0, |b| b.down.saturating_add(b.up)))
        }
    }
}

/// Keeps the last aggregate and hands it back until the key changes.
#[derive(Default)]
pub struct AggCache {
    last: Option<(CacheKey, Arc<Agg>)>,
}

impl AggCache {
    pub fn get(&mut self, key: CacheKey, f: impl FnOnce() -> Agg) -> Arc<Agg> {
        if let Some((k, a)) = &self.last {
            if *k == key {
                return a.clone();
            }
        }
        let a = Arc::new(f());
        self.last = Some((key, a.clone()));
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo;
    use crate::monitor::{other_name, OTHER_KEY};
    use crate::ports::{UNATTRIBUTED, UNCLASSIFIED};
    use crate::store::{AppRecord, Bucket, Flow, HOUR, MINUTE};
    use chrono::{Local, NaiveDate, TimeZone};

    /// Start of an hour.
    const T0: i64 = 1_789_999_200;

    /// (app, host, type, country, down, up)
    type F<'a> = (&'a str, &'a str, &'a str, &'a str, u64, u64);

    /// A bucket at `t` with `apps` as given (down/up = Σ apps) and `flows` added through
    /// `add_flow`. LAN flows also count in `lan_down`/`lan_up`, named hosts in `hosts`.
    fn bucket(t: i64, apps: &[(&str, [u64; 2])], flows: &[F]) -> Bucket {
        let mut b = Bucket { t, ..Default::default() };
        for (k, v) in apps {
            b.apps.insert(k.to_string(), *v);
            b.down += v[0];
            b.up += v[1];
        }
        for &(app, h, ty, cc, d, u) in flows {
            b.add_flow(app, &Flow { h: h.into(), ty: ty.into(), cc: cc.into(), d, u });
            if cc == geo::LAN {
                b.lan_down += d;
                b.lan_up += u;
            }
            if !h.is_empty() {
                let e = b.hosts.entry(h.to_string()).or_default();
                e[0] += d;
                e[1] += u;
            }
        }
        b
    }

    fn query(span: Span, group: Group, sel: Sel) -> Query {
        Query { span, group, net: Net::All, dir: DirF::Both, sel, admin: true, end_offset: 0 }
    }

    fn no_live() -> VecDeque<LiveSample> {
        VecDeque::new()
    }

    fn sample(t: i64, apps: &[(&str, [u64; 2])], flows: &[F]) -> LiveSample {
        let b = bucket(t, apps, flows);
        let flows = flows.iter().map(|&(app, h, ty, cc, d, u)| (app.to_string(), Flow { h: h.into(), ty: ty.into(), cc: cc.into(), d, u })).collect();
        LiveSample { t, down: b.down, up: b.up, apps: apps.iter().map(|(k, v)| (k.to_string(), *v)).collect(), flows }
    }

    fn sum(rows: &[Row]) -> [u64; 2] {
        rows.iter().fold([0, 0], |s, r| [s[0] + r.down, s[1] + r.up])
    }

    fn row<'a>(rows: &'a [Row], key: &str) -> Option<&'a Row> {
        rows.iter().find(|r| r.key == key)
    }

    fn bytes(rows: &[Row], key: &str) -> Option<[u64; 2]> {
        row(rows, key).map(|r| [r.down, r.up])
    }

    fn keys(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.key.as_str()).collect()
    }

    fn series_sum(a: &Agg) -> [u64; 2] {
        [a.down.iter().sum::<f64>() as u64, a.up.iter().sum::<f64>() as u64]
    }

    fn midnight(y: i32, m: u32, d: u32) -> i64 {
        let naive = NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(0, 0, 0).unwrap();
        Local.from_local_datetime(&naive).earliest().unwrap().timestamp()
    }

    /// Two hour buckets: an old one written before flows existed and a fully classified new one.
    /// Old: a 1000/100 and `__other` 300/30, LAN 200/20, host map `old.example.net` 800/80.
    /// New: a 450/45 https US + 150/15 smb LAN, `__other` 90/9 unattributed.
    fn mixed() -> Data {
        let mut d = Data::default();
        let mut old = bucket(T0, &[("a", [1000, 100]), (OTHER_KEY, [300, 30])], &[]);
        old.lan_down = 200;
        old.lan_up = 20;
        old.hosts.insert("old.example.net".into(), [800, 80]);
        d.hours.push_back(old);
        d.hours.push_back(bucket(
            T0 + HOUR,
            &[("a", [600, 60]), (OTHER_KEY, [90, 9])],
            &[("a", "x.example.net", "https", "US", 450, 45), ("a", "nas.local", "smb", geo::LAN, 150, 15), (OTHER_KEY, "", UNATTRIBUTED, geo::UNKNOWN, 90, 9)],
        ));
        d
    }
    const MIXED_NOW: i64 = T0 + HOUR + 100;
    const MIXED_TOTAL: [u64; 2] = [1990, 199];

    #[test]
    fn apps_mode_matches_today() {
        let mut d = Data::default();
        for i in 0..10u64 {
            d.minutes.push_back(bucket(T0 + i as i64 * MINUTE, &[("a", [100 + i, 10]), ("b", [50, 5 + i]), (OTHER_KEY, [7, 1])], &[]));
        }
        let now = T0 + 9 * MINUTE + 30;
        let agg = aggregate(&d, &no_live(), &query(Span::Preset(1), Group::Apps, Sel::All), now);
        assert_eq!(agg.step, MINUTE);
        assert_eq!(agg.times.len(), 180);
        assert_eq!(agg.times[0], T0 + 9 * MINUTE - 179 * MINUTE);
        assert_eq!(*agg.times.last().unwrap(), T0 + 9 * MINUTE);
        assert_eq!((agg.down.len(), agg.up.len(), agg.step_top.len()), (180, 180, 180));
        for (i, t) in agg.times.iter().enumerate() {
            let want = d.minutes.iter().find(|b| b.t == *t).map(|b| (b.down as f64, b.up as f64)).unwrap_or((0.0, 0.0));
            assert_eq!((agg.down[i], agg.up[i]), want, "step {t}");
        }
        assert_eq!(keys(&agg.rows), ["a", "b", OTHER_KEY]);
        assert_eq!(bytes(&agg.rows, "a"), Some([1045, 100]));
        assert_eq!(bytes(&agg.rows, "b"), Some([500, 95]));
        assert_eq!(bytes(&agg.rows, OTHER_KEY), Some([70, 10]));
        assert_eq!(row(&agg.rows, OTHER_KEY).unwrap().label, other_name(true));
        assert_eq!(row(&agg.rows, "a").unwrap().icon_key.as_deref(), Some("a"));
        assert_eq!(agg.total, [1615, 205]);
        assert_eq!(series_sum(&agg), agg.total);
        let last = agg.step_top.last().unwrap();
        assert_eq!(last, &vec![("a".to_string(), [109, 10]), ("b".to_string(), [50, 14]), (OTHER_KEY.to_string(), [7, 1])]);
        assert!(agg.step_top[0].is_empty());

        // Selecting an app draws only its bytes; the list keeps every app.
        let agg = aggregate(&d, &no_live(), &query(Span::Preset(1), Group::Apps, Sel::App("a".into())), now);
        assert_eq!(agg.total, [1045, 100]);
        assert_eq!(series_sum(&agg), [1045, 100]);
        assert_eq!(agg.rows.len(), 3);
    }

    #[test]
    fn types_rows_sum_to_total_with_old_history() {
        let d = mixed();
        let agg = aggregate(&d, &no_live(), &query(Span::Preset(3), Group::Types, Sel::All), MIXED_NOW);
        assert_eq!(agg.total, MIXED_TOTAL);
        assert_eq!(sum(&agg.rows), agg.total);
        assert_eq!(bytes(&agg.rows, UNCLASSIFIED), Some([1300, 130]), "the old bucket is not classified");
        assert_eq!(row(&agg.rows, UNCLASSIFIED).unwrap().label, "Not classified");
        assert_eq!(bytes(&agg.rows, "https"), Some([450, 45]));
        assert_eq!(bytes(&agg.rows, UNATTRIBUTED), Some([90, 9]));
        assert_eq!(sum(&agg.types), agg.total);
    }

    #[test]
    fn countries_old_history_is_not_classified() {
        let d = mixed();
        let agg = aggregate(&d, &no_live(), &query(Span::Preset(3), Group::Countries, Sel::All), MIXED_NOW);
        assert_eq!(bytes(&agg.rows, UNCLASSIFIED), Some([1300, 130]));
        assert_eq!(bytes(&agg.rows, geo::UNKNOWN), Some([90, 9]), "only the new unattributed bytes are ZZ");
        assert_eq!(bytes(&agg.rows, "US"), Some([450, 45]));
        assert_eq!(row(&agg.rows, "US").unwrap().cc.as_deref(), Some("US"));
        assert_eq!(row(&agg.rows, UNCLASSIFIED).unwrap().label, "Not classified");
        assert_eq!(sum(&agg.rows), agg.total);
        assert_eq!(sum(&agg.countries), agg.total);
    }

    #[test]
    fn columns_reconcile() {
        let d = mixed();
        for sel in [Sel::All, Sel::App("a".into()), Sel::App(OTHER_KEY.into())] {
            let agg = aggregate(&d, &no_live(), &query(Span::Preset(3), Group::Apps, sel.clone()), MIXED_NOW);
            assert!(agg.total != [0, 0], "{sel:?}");
            assert_eq!(series_sum(&agg), agg.total, "{sel:?}");
            assert_eq!(sum(&agg.hosts), agg.total, "{sel:?} hosts");
            assert_eq!(sum(&agg.types), agg.total, "{sel:?} types");
            assert_eq!(sum(&agg.countries), agg.total, "{sel:?} countries");
        }
        let all = aggregate(&d, &no_live(), &query(Span::Preset(3), Group::Apps, Sel::All), MIXED_NOW);
        assert_eq!(all.total, MIXED_TOTAL);
        // Host maps: the old host has no country; new hosts are split by country from flows.
        assert_eq!(row(&all.hosts, "old.example.net|").map(|r| ([r.down, r.up], r.cc.clone(), r.apps.len())), Some(([800, 80], None, 0)));
        assert_eq!(row(&all.hosts, "x.example.net|US").map(|r| ([r.down, r.up], r.cc.clone(), r.apps.clone())), Some(([450, 45], Some("US".into()), vec!["a".to_string()])));
        assert_eq!(bytes(&all.hosts, NO_HOST), Some([590, 59]));
        assert_eq!(row(&all.hosts, NO_HOST).unwrap().label, "Other traffic (no host)");
        let a = aggregate(&d, &no_live(), &query(Span::Preset(3), Group::Apps, Sel::App("a".into())), MIXED_NOW);
        assert_eq!(a.total, [1600, 160]);
        assert_eq!(bytes(&a.hosts, UNCLASSIFIED), Some([1000, 100]));
        assert_eq!(bytes(&a.hosts, "nas.local|LAN"), Some([150, 15]));
    }

    #[test]
    fn publishers_group_and_other_row() {
        let mut d = Data::default();
        let rec = |p: Option<&str>| AppRecord { name: "x".into(), publisher: p.map(str::to_string), ..Default::default() };
        d.apps.insert("a".into(), rec(Some("P")));
        d.apps.insert("b".into(), rec(Some("P")));
        d.apps.insert("c".into(), rec(None));
        d.minutes.push_back(bucket(T0, &[("a", [100, 10]), ("b", [300, 30]), ("c", [50, 5]), ("d", [20, 2]), (OTHER_KEY, [70, 7])], &[]));
        let agg = aggregate(&d, &no_live(), &query(Span::Preset(1), Group::Publishers, Sel::All), T0 + 30);
        assert_eq!(agg.rows[0].key, "P");
        assert_eq!(agg.rows[0].label, "P");
        assert_eq!(bytes(&agg.rows, "P"), Some([400, 40]));
        assert_eq!(row(&agg.rows, "P").unwrap().icon_key.as_deref(), Some("b"), "largest app's icon");
        assert_eq!(bytes(&agg.rows, NO_PUBLISHER), Some([70, 7]));
        assert_eq!(row(&agg.rows, NO_PUBLISHER).unwrap().label, "Unknown publisher");
        let other = row(&agg.rows, OTHER_KEY).unwrap();
        assert_eq!(([other.down, other.up], other.label.as_str()), ([70, 7], other_name(true)));
        assert_eq!(agg.rows.len(), 3);
        assert_eq!(sum(&agg.rows), agg.total);
    }

    #[test]
    fn group_selection_filters_series_and_columns() {
        let mut d = Data::default();
        d.apps.insert("a".into(), AppRecord { publisher: Some("P".into()), ..Default::default() });
        d.minutes.push_back(bucket(
            T0,
            &[("a", [180, 18]), ("b", [40, 4]), (OTHER_KEY, [20, 2])],
            &[
                ("a", "x.example.net", "https", "IL", 100, 10),
                ("a", "y.example.net", "https", "IT", 50, 5),
                ("b", "z.example.net", "ssh", "IL", 40, 4),
                (OTHER_KEY, "", UNATTRIBUTED, geo::UNKNOWN, 20, 2),
            ],
        ));
        let run = |sel: Sel| aggregate(&d, &no_live(), &query(Span::Preset(1), Group::Countries, sel), T0 + 30);

        let il = run(Sel::Group(Group::Countries, "IL".into()));
        assert_eq!(il.total, [140, 14]);
        assert_eq!(series_sum(&il), [140, 14]);
        let mut hk = keys(&il.hosts);
        hk.sort();
        assert_eq!(hk, ["x.example.net|IL", "z.example.net|IL"]);
        assert_eq!(bytes(&il.types, "https"), Some([100, 10]));
        assert_eq!(bytes(&il.types, "ssh"), Some([40, 4]));
        assert_eq!(il.types.len(), 2);
        assert_eq!(keys(&il.countries), ["IL"]);
        assert_eq!(il.rows.len(), 4, "the list itself is not filtered by the selection");

        let p = run(Sel::Group(Group::Publishers, "P".into()));
        assert_eq!(p.total, [180, 18]);
        assert_eq!(series_sum(&p), [180, 18]);
        assert_eq!(bytes(&p.types, "https"), Some([150, 15]));
        assert_eq!(bytes(&p.types, UNCLASSIFIED), Some([30, 3]));
        assert_eq!(bytes(&p.hosts, UNCLASSIFIED), Some([30, 3]));
        assert!(row(&p.hosts, "z.example.net|IL").is_none());

        let un = run(Sel::Group(Group::Types, UNCLASSIFIED.into()));
        assert_eq!(un.total, [30, 3]);
        assert_eq!(keys(&un.types), [UNCLASSIFIED]);
        assert_eq!(keys(&un.hosts), [UNCLASSIFIED]);
        assert_eq!(keys(&un.countries), [UNCLASSIFIED]);
    }

    #[test]
    fn net_filter_rows_reconcile() {
        let d = mixed();
        let base = aggregate(&d, &no_live(), &query(Span::Preset(3), Group::Apps, Sel::All), MIXED_NOW);
        assert_eq!(base.lan, [350, 35]);
        assert_eq!(base.wan, [1640, 164]);
        for net in [Net::Lan, Net::Wan] {
            for group in [Group::Apps, Group::Publishers, Group::Types, Group::Countries] {
                let q = Query { net, ..query(Span::Preset(3), group, Sel::All) };
                let agg = aggregate(&d, &no_live(), &q, MIXED_NOW);
                let want = if net == Net::Lan { base.lan } else { base.wan };
                assert_eq!(agg.total, want, "{net:?} {group:?}");
                assert_eq!(series_sum(&agg), agg.total, "{net:?} {group:?}");
                assert_eq!(sum(&agg.rows), agg.total, "{net:?} {group:?} rows");
                assert_eq!(sum(&agg.types), agg.total, "{net:?} {group:?} types");
                assert_eq!(sum(&agg.countries), agg.total, "{net:?} {group:?} countries");
                assert_eq!(sum(&agg.hosts), agg.total, "{net:?} {group:?} hosts");
                assert_eq!((agg.lan, agg.wan), (base.lan, base.wan), "the strip ignores the network filter");
            }
        }
        let lan = aggregate(&d, &no_live(), &Query { net: Net::Lan, ..query(Span::Preset(3), Group::Types, Sel::All) }, MIXED_NOW);
        assert_eq!(bytes(&lan.rows, "smb"), Some([150, 15]));
        assert_eq!(bytes(&lan.rows, UNCLASSIFIED), Some([200, 20]), "old LAN bytes");
        // With a selection the residual counts as WAN.
        let a = |net| aggregate(&d, &no_live(), &Query { net, ..query(Span::Preset(3), Group::Apps, Sel::App("a".into())) }, MIXED_NOW);
        let (al, aw, all) = (a(Net::Lan), a(Net::Wan), a(Net::All));
        assert_eq!(al.total, [150, 15]);
        assert_eq!(aw.total, [1450, 145]);
        assert_eq!((al.lan, al.wan), (all.lan, all.wan));
        assert_eq!(all.lan, [150, 15]);
        assert_eq!(all.wan, [1450, 145]);
    }

    #[test]
    fn direction_in_zeroes_up() {
        let mut d = mixed();
        d.minutes.push_back(bucket(T0 + HOUR, &[("a", [5, 50])], &[("a", "x.example.net", "https", "US", 5, 50)]));
        let mut live = VecDeque::new();
        live.push_back(sample(MIXED_NOW, &[("a", [7, 70])], &[("a", "x.example.net", "https", "US", 7, 70)]));
        for span in [Span::Preset(0), Span::Preset(1), Span::Preset(3)] {
            for group in [Group::Apps, Group::Types] {
                let q = Query { dir: DirF::In, ..query(span, group, Sel::All) };
                let agg = aggregate(&d, &live, &q, MIXED_NOW);
                assert!(agg.total[0] > 0, "{span:?}");
                assert_eq!(agg.total[1], 0);
                assert_eq!(live_rate(&d, &live, &q), [7, 0]);
                assert!(!agg.rows.is_empty());
                for rows in [&agg.rows, &agg.hosts, &agg.types, &agg.countries] {
                    assert!(rows.iter().all(|r| r.up == 0), "{span:?} {rows:?}");
                }
                assert!(agg.up.iter().all(|v| *v == 0.0));
                assert!(agg.step_top.iter().flatten().all(|(_, v)| v[1] == 0));
                assert!(agg.step_top.iter().flatten().count() > 0);
                assert_eq!((agg.lan[1], agg.wan[1]), (0, 0));
            }
        }
    }

    #[test]
    fn rate_follows_selection_and_dir() {
        let mut live = VecDeque::new();
        live.push_back(sample(T0 - 1, &[("a", [999, 999])], &[]));
        live.push_back(sample(
            T0,
            &[("a", [100, 10]), ("b", [50, 5])],
            &[("a", "nas", "smb", geo::LAN, 30, 3), ("a", "y", "https", "US", 70, 7), ("b", "z", "https", "US", 50, 5)],
        ));
        let d = Data::default();
        let rate = |sel: Sel, net: Net, dir: DirF| live_rate(&d, &live, &Query { net, dir, ..query(Span::Preset(3), Group::Apps, sel) });
        assert_eq!(rate(Sel::All, Net::All, DirF::Both), [150, 15]);
        assert_eq!(rate(Sel::App("a".into()), Net::All, DirF::Both), [100, 10]);
        assert_eq!(rate(Sel::App("a".into()), Net::All, DirF::Out), [0, 10]);
        assert_eq!(rate(Sel::App("a".into()), Net::Lan, DirF::Both), [30, 3]);
        assert_eq!(rate(Sel::All, Net::Wan, DirF::Both), [120, 12]);
        assert_eq!(rate(Sel::Group(Group::Countries, "US".into()), Net::All, DirF::In), [120, 0]);
    }

    #[test]
    fn rate_is_fresh_after_an_idle_second() {
        let mut d = Data::default();
        d.minutes.push_back(bucket(T0, &[("a", [100, 10])], &[]));
        let mut live = VecDeque::new();
        live.push_back(sample(T0 + 1, &[("a", [40, 4])], &[]));
        let q = query(Span::Preset(3), Group::Apps, Sel::All);
        let mut cache = AggCache::default();
        let key = cache_key(&q, &d, &live, T0 + 1);
        cache.get(key.clone(), || aggregate(&d, &live, &q, T0 + 1));
        assert_eq!(live_rate(&d, &live, &q), [40, 4]);
        // an idle second leaves the bucket (and so the cache key) alone, but not the rate
        live.push_back(sample(T0 + 2, &[], &[]));
        assert_eq!(cache_key(&q, &d, &live, T0 + 2), key);
        assert_eq!(live_rate(&d, &live, &q), [0, 0]);
    }

    #[test]
    fn hosts_split_by_country() {
        let mut d = Data::default();
        d.minutes.push_back(bucket(T0, &[("a", [100, 0])], &[("a", "x", "https", "IT", 10, 0), ("a", "x", "https", "IL", 90, 0)]));
        for sel in [Sel::App("a".into()), Sel::All] {
            let agg = aggregate(&d, &no_live(), &query(Span::Preset(1), Group::Apps, sel.clone()), T0 + 30);
            assert_eq!(keys(&agg.hosts), ["x|IL", "x|IT"], "{sel:?}");
            assert_eq!(agg.hosts[0].cc.as_deref(), Some("IL"));
            assert_eq!(agg.hosts[1].cc.as_deref(), Some("IT"));
            assert_eq!((agg.hosts[0].down, agg.hosts[1].down), (90, 10));
            assert_eq!(agg.hosts[0].label, "x");
        }
    }

    #[test]
    fn host_apps_top3() {
        let mut d = Data::default();
        let bytes = [("a1", 10), ("a2", 50), ("a3", 30), ("a4", 40), ("a5", 20)];
        let apps: Vec<(&str, [u64; 2])> = bytes.iter().map(|&(k, b)| (k, [b, 0])).collect();
        let flows: Vec<F> = bytes.iter().map(|&(k, b)| (k, "h", "https", "US", b, 0)).collect();
        d.minutes.push_back(bucket(T0, &apps, &flows));
        for sel in [Sel::All, Sel::Group(Group::Countries, "US".into())] {
            let agg = aggregate(&d, &no_live(), &query(Span::Preset(1), Group::Apps, sel), T0 + 30);
            let h = row(&agg.hosts, "h|US").unwrap();
            assert_eq!(h.apps, ["a2", "a4", "a3"]);
            assert_eq!(h.more_apps, 2);
            assert_eq!(h.down, 150);
        }
    }

    #[test]
    fn span_day_uses_hours_when_old() {
        let now = midnight(2026, 6, 15) + 12 * HOUR;
        let mut d = Data::default();
        let mut t = midnight(2026, 6, 10);
        while t <= now {
            d.hours.push_back(bucket(t, &[("a", [10, 1])], &[]));
            t += HOUR;
        }
        let day = midnight(2026, 6, 12);
        assert_eq!(span_window(Span::Day(day), now, &d), (day, midnight(2026, 6, 13), HOUR, Source::Hours));
        let agg = aggregate(&d, &no_live(), &query(Span::Day(day), Group::Apps, Sel::All), now);
        assert_eq!(agg.times.len(), 24);
        assert_eq!(agg.step, HOUR);
        assert_eq!(agg.total, [240, 24]);
        assert_eq!(agg.max_offset, 0);
        // Today fits in minute retention.
        assert_eq!(span_window(Span::Day(midnight(2026, 6, 15)), now, &d).3, Source::Minutes);
        assert_eq!(local_midnight(now), midnight(2026, 6, 15));
    }

    #[test]
    fn span_step_skips_to_data() {
        let now = crate::store::now();
        let h = now - now.rem_euclid(HOUR);
        let old = h - 5 * 24 * HOUR;
        let mut d = Data::default();
        d.hours.push_back(bucket(old, &[("a", [1, 1])], &[]));
        d.hours.push_back(bucket(h, &[("a", [1, 1])], &[]));
        let today = Span::Day(local_midnight(now));
        let back = Span::Day(local_midnight(old));
        assert_ne!(today, back);
        assert_eq!(span_step(today, -1, &d), back);
        assert_eq!(span_step(today, 1, &d), today, "never past the current period");
        assert_eq!(span_step(back, 1, &d), today);
        assert_eq!(span_step(back, -1, &d), back, "no earlier data");
        assert_eq!(span_step(Span::Preset(2), -1, &d), Span::Preset(2));
        let Span::Week(w) = span_step(Span::Week(local_midnight(now) + 7 * 24 * HOUR), -1, &d) else { panic!("week") };
        assert!(w <= now && now - w < 8 * 24 * HOUR);
    }

    #[test]
    fn custom_span_clipped() {
        let mut d = Data::default();
        for i in 0..48 {
            d.hours.push_back(bucket(T0 + i * HOUR, &[("a", [1, 0])], &[]));
        }
        let now = T0 + 100 * HOUR;
        let span = Span::Custom(T0 - 5 * 24 * HOUR, T0 + 24 * HOUR);
        assert_eq!(span_window(span, now, &d), (T0, T0 + 24 * HOUR, HOUR, Source::Hours));
        let agg = aggregate(&d, &no_live(), &query(span, Group::Apps, Sel::All), now);
        assert_eq!(agg.times[0], T0);
        assert_eq!(agg.times.len(), 24);
        assert_eq!(agg.total, [24, 0]);
        // A range with no stored data is empty.
        let none = aggregate(&d, &no_live(), &query(Span::Custom(T0 - 9 * 24 * HOUR, T0 - 8 * 24 * HOUR), Group::Apps, Sel::All), now);
        assert!(none.times.is_empty() && none.rows.is_empty() && none.total == [0, 0]);
    }

    #[test]
    fn end_offset_shifts_window() {
        let mut d = Data::default();
        for i in 0..300u64 {
            d.minutes.push_back(bucket(T0 + i as i64 * MINUTE, &[("a", [i + 1, 0])], &[]));
        }
        let now = T0 + 299 * MINUTE + 10;
        let q = Query { end_offset: 2, ..query(Span::Preset(1), Group::Apps, Sel::All) };
        let agg = aggregate(&d, &no_live(), &q, now);
        assert_eq!(agg.times.len(), 180);
        assert_eq!(*agg.times.last().unwrap(), T0 + 297 * MINUTE);
        assert_eq!(agg.total[0], (118..=297u64).map(|i| i + 1).sum::<u64>());
        assert_eq!(agg.max_offset, 300 - 180);
        assert_eq!(agg.history.len(), 300);
        assert_eq!(agg.history[299], (T0 + 299 * MINUTE, 300.0));
        let far = aggregate(&d, &no_live(), &Query { end_offset: 5000, ..q.clone() }, now);
        assert_eq!(far.times[0], T0, "clamped to the oldest bucket");

        // 5m: one step is one live sample.
        let mut live = VecDeque::new();
        for i in 0..400 {
            live.push_back(sample(T0 + i, &[("a", [1, 0])], &[]));
        }
        let now = T0 + 399;
        let q = Query { end_offset: 3, ..query(Span::Preset(0), Group::Apps, Sel::All) };
        let agg = aggregate(&d, &live, &q, now);
        assert_eq!(agg.times.len(), 300);
        assert_eq!(*agg.times.last().unwrap(), now - 3);
        assert_eq!(agg.max_offset, 100);
        assert_eq!(agg.history.len(), 400);
        assert_eq!(agg.step, 1);
    }

    #[test]
    fn effective_limited() {
        assert_eq!(effective(Group::Types, Net::Lan, false), (Group::Apps, Net::All));
        assert_eq!(effective(Group::Countries, Net::Wan, true), (Group::Countries, Net::Wan));
        for v in 0..4 {
            assert_eq!(Group::from_u8(v).to_u8(), v);
        }
        for v in 0..3 {
            assert_eq!(Net::from_u8(v).to_u8(), v);
            assert_eq!(DirF::from_u8(v).to_u8(), v);
        }
        assert_eq!((Group::from_u8(9), Net::from_u8(9), DirF::from_u8(9)), (Group::Apps, Net::All, DirF::Both));
        assert_eq!((Group::from_u8(3), Net::from_u8(2), DirF::from_u8(1)), (Group::Countries, Net::Wan, DirF::In));
    }

    #[test]
    fn limited_direction_works_other_present() {
        let mut d = Data::default();
        d.minutes.push_back(bucket(T0, &[(OTHER_KEY, [100, 20])], &[]));
        let mut live = VecDeque::new();
        live.push_back(sample(T0, &[(OTHER_KEY, [9, 3])], &[]));
        // A saved admin-only group and network fall back to Apps / WAN & LAN.
        let q = Query { dir: DirF::Out, net: Net::Lan, admin: false, ..query(Span::Preset(1), Group::Types, Sel::All) };
        let agg = aggregate(&d, &live, &q, T0 + 30);
        assert_eq!(keys(&agg.rows), [OTHER_KEY]);
        assert_eq!(bytes(&agg.rows, OTHER_KEY), Some([0, 20]));
        assert_eq!(agg.total, [0, 20]);
        assert_eq!(live_rate(&d, &live, &q), [0, 3]);
        assert_eq!(agg.wan, [0, 20]);
        assert_eq!(sum(&agg.hosts), agg.total);
    }

    #[test]
    fn cache_reuses_until_key_changes() {
        let mut d = Data::default();
        d.minutes.push_back(bucket(T0, &[("a", [1, 1])], &[]));
        let mut live = VecDeque::new();
        live.push_back(sample(T0 + 5, &[("a", [1, 1])], &[]));
        let now = T0 + 5;
        let q = query(Span::Preset(1), Group::Apps, Sel::All);
        let mut cache = AggCache::default();
        let mut calls = 0;
        let k1 = cache_key(&q, &d, &live, now);
        assert_eq!((k1.1, k1.2), (T0, 2));
        let a = cache.get(k1.clone(), || {
            calls += 1;
            aggregate(&d, &live, &q, now)
        });
        let b = cache.get(k1.clone(), || {
            calls += 1;
            Agg::default()
        });
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(calls, 1);
        // A new live second alone doesn't change a bucket range's key.
        live.push_back(sample(T0 + 6, &[("a", [1, 1])], &[]));
        assert_eq!(cache_key(&q, &d, &live, now + 1), k1);
        // New bytes in the newest bucket do.
        d.minutes.back_mut().unwrap().down += 5;
        let k2 = cache_key(&q, &d, &live, now);
        assert_ne!(k2, k1);
        cache.get(k2.clone(), || {
            calls += 1;
            Agg::default()
        });
        assert_eq!(calls, 2);
        let off = Query { end_offset: 1, ..q.clone() };
        assert_ne!(cache_key(&off, &d, &live, now), k2, "end_offset is part of the key");
        let live_q = query(Span::Preset(0), Group::Apps, Sel::All);
        let k = cache_key(&live_q, &d, &live, now);
        assert_eq!((k.1, k.2), (T0 + 6, 0));
    }

    #[test]
    #[ignore]
    fn aggregate_30d_heavy_under_budget() {
        let d = crate::store::heavy_fixture();
        let now = d.hours.back().unwrap().t + 1800;
        let live = no_live();
        for (group, sel) in [(Group::Apps, Sel::All), (Group::Countries, Sel::All), (Group::Types, Sel::App(d.hours[0].flows.keys().next().unwrap().clone()))] {
            let q = query(Span::Preset(4), group, sel);
            let mut times = Vec::new();
            let mut agg = Agg::default();
            for _ in 0..5 {
                let start = std::time::Instant::now();
                agg = aggregate(&d, &live, &q, now);
                times.push(start.elapsed());
            }
            assert_eq!(agg.times.len(), 720);
            assert_eq!(sum(&agg.types), agg.total);
            let best = *times.iter().min().unwrap();
            eprintln!("aggregate 30d heavy {group:?}: {times:?}");
            assert!(best < std::time::Duration::from_millis(10), "{group:?} took {best:?}");
        }
    }
}
