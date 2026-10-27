//! CSV export of the Usage view (Task 8).
//!
//! One long table, UTF-8 with a BOM so Excel picks the encoding: a row per column entry, in the
//! order the screen shows. Host names come from remote PTR records, so every text cell is quoted
//! per RFC 4180 when needed and a leading formula character is neutralised with `'`.

#[derive(Clone, Debug)]
pub struct ExportRow {
    pub column: &'static str,
    pub name: String,
    pub country_code: String,
    pub down: u64,
    pub up: u64,
}

const HEADER: &str = "column,name,country_code,download_bytes,upload_bytes,total_bytes";

/// One text cell: a leading formula character gets a `'`, then RFC 4180 quoting when the cell
/// holds a comma, quote, CR or LF.
fn cell(out: &mut String, s: &str) {
    let lead = matches!(s.chars().next(), Some('=' | '+' | '-' | '@' | '\t' | '\r'));
    let quote = s.contains([',', '"', '\r', '\n']);
    if quote {
        out.push('"');
    }
    if lead {
        out.push('\'');
    }
    if quote {
        out.push_str(&s.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(s);
    }
}

pub fn to_csv(rows: &[ExportRow]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(64 + rows.len() * 48);
    out.push('\u{FEFF}');
    out.push_str(HEADER);
    out.push_str("\r\n");
    for r in rows {
        cell(&mut out, r.column);
        out.push(',');
        cell(&mut out, &r.name);
        out.push(',');
        cell(&mut out, &r.country_code);
        let _ = write!(out, ",{},{},{}\r\n", r.down, r.up, r.down.saturating_add(r.up));
    }
    out
}

/// `Vantage usage <range> [<selection>] [<network>] [<direction>] <stamp>.csv`, empty parts
/// left out, with characters Windows forbids in file names removed.
pub fn file_name(range_code: &str, sel: &str, net: &str, dir: &str, stamp: &str) -> String {
    let clean = |s: &str| -> String { s.chars().filter(|ch| !ch.is_control() && !matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')).collect::<String>().trim().to_string() };
    let mut name = String::from("Vantage usage");
    for part in [range_code, sel, net, dir, stamp] {
        let part = clean(part);
        if !part.is_empty() {
            name.push(' ');
            name.push_str(&part);
        }
    }
    let name = name.trim_end_matches(['.', ' ']);
    format!("{name}.csv")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str) -> ExportRow {
        ExportRow { column: "hosts", name: name.to_string(), country_code: String::new(), down: 1, up: 2 }
    }

    /// The data line (after the BOM and header) for one row.
    fn line(r: ExportRow) -> String {
        let csv = to_csv(&[r]);
        csv.split("\r\n").nth(1).unwrap_or_default().to_string()
    }

    #[test]
    fn csv_neutralises_each_leading_char() {
        for (name, cell) in [
            ("=HYPERLINK(\"x\")", "\"'=HYPERLINK(\"\"x\"\")\""),
            ("+1", "'+1"),
            ("-1", "'-1"),
            ("@SUM(A1)", "'@SUM(A1)"),
            ("\tcmd", "'\tcmd"),
            ("\rcmd", "\"'\rcmd\""),
        ] {
            assert_eq!(line(row(name)), format!("hosts,{cell},,1,2,3"), "{name:?}");
        }
        // only a leading character counts
        assert_eq!(line(row("a=b")), "hosts,a=b,,1,2,3");
    }

    #[test]
    fn csv_quotes_comma_newline_and_quote() {
        assert_eq!(line(row("a,b")), "hosts,\"a,b\",,1,2,3");
        assert_eq!(line(row("say \"hi\"")), "hosts,\"say \"\"hi\"\"\",,1,2,3");
        let csv = to_csv(&[row("two\nlines")]);
        assert!(csv.contains("hosts,\"two\nlines\",,1,2,3\r\n"), "{csv:?}");
    }

    #[test]
    fn csv_starts_with_bom_header() {
        let csv = to_csv(&[]);
        assert_eq!(csv, "\u{FEFF}column,name,country_code,download_bytes,upload_bytes,total_bytes\r\n");
    }

    #[test]
    fn csv_raw_integers_and_total() {
        let r = ExportRow { column: "countries", name: "Germany".into(), country_code: "DE".into(), down: 1_234_567_890_123, up: u64::MAX };
        assert_eq!(line(r), format!("countries,Germany,DE,1234567890123,{},{}", u64::MAX, u64::MAX));
        let r = ExportRow { column: "types", name: "Web".into(), country_code: String::new(), down: 1_000_000, up: 24 };
        assert_eq!(line(r), "types,Web,,1000000,24,1000024");
    }

    #[test]
    fn file_name_strips_illegal() {
        assert_eq!(file_name("24h", "", "", "", "2026-10-27 14-30"), "Vantage usage 24h 2026-10-27 14-30.csv");
        assert_eq!(
            file_name("7d", "C:\\a<b>|c?*\"d/e", "WAN", "download", "2026-10-27 14:30"),
            "Vantage usage 7d Cabcde WAN download 2026-10-27 1430.csv"
        );
        assert_eq!(file_name("5m", "x\u{7}y", "", "", "s"), "Vantage usage 5m xy s.csv");
    }
}
