use crate::error::Result;
use regex::Regex;
use rusqlite::{Connection, params};
use std::collections::BTreeSet;
use std::sync::OnceLock;

/// Extract distinct RFC numbers mentioned in body text. Catches `[RFC8446]`,
/// `[RFC 8446, ...]`, and bare "RFC 8446" / "RFCs 8446". Filters out
/// self-references by `self_number`.
pub(crate) fn extract(text: &str, self_number: u32) -> Vec<u32> {
    let re = pattern();
    let mut nums: BTreeSet<u32> = BTreeSet::new();
    for cap in re.captures_iter(text) {
        if let Some(m) = cap.get(1) {
            if let Ok(n) = m.as_str().parse::<u32>() {
                if n != self_number {
                    nums.insert(n);
                }
            }
        }
    }
    nums.into_iter().collect()
}

fn pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bRFCs?\s*(\d{1,5})\b").unwrap())
}

/// Replace all body-derived edges from `from_rfc` with the given target list.
pub(crate) fn save(conn: &Connection, from_rfc: u32, targets: &[u32]) -> Result<()> {
    conn.execute("DELETE FROM rfc_body_refs WHERE from_rfc=?1", [from_rfc])?;
    let mut ins =
        conn.prepare("INSERT OR IGNORE INTO rfc_body_refs(from_rfc, to_rfc) VALUES (?1, ?2)")?;
    for t in targets {
        ins.execute(params![from_rfc, t])?;
    }
    Ok(())
}

pub(crate) fn index_body(conn: &Connection, from_rfc: u32, text: &str) -> Result<()> {
    let targets = extract(text, from_rfc);
    save(conn, from_rfc, &targets)
}

pub(crate) fn references_from(conn: &Connection, n: u32) -> Result<Vec<u32>> {
    let mut stmt =
        conn.prepare("SELECT to_rfc FROM rfc_body_refs WHERE from_rfc=?1 ORDER BY to_rfc")?;
    let rows = stmt.query_map([n], |r| r.get::<_, u32>(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn referenced_by(conn: &Connection, n: u32) -> Result<Vec<u32>> {
    let mut stmt =
        conn.prepare("SELECT from_rfc FROM rfc_body_refs WHERE to_rfc=?1 ORDER BY from_rfc")?;
    let rows = stmt.query_map([n], |r| r.get::<_, u32>(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_bracketed_and_bare_forms() {
        let text = "\
See [RFC8446] and [RFC 8447]. As described in RFC 5246 and RFCs 9000.
Also mentioned: [RFC8446, Section 4.1] and rfc 12345 (lowercase ignored).
";
        let got = extract(text, 9999);
        assert_eq!(got, vec![5246, 8446, 8447, 9000]);
    }

    #[test]
    fn skips_self_reference() {
        let text = "This is RFC 9000. See also RFC 8446.";
        let got = extract(text, 9000);
        assert_eq!(got, vec![8446]);
    }

    #[test]
    fn dedupes_repeats() {
        let text = "[RFC8446] and again [RFC8446] then RFC 8446";
        let got = extract(text, 1);
        assert_eq!(got, vec![8446]);
    }
}
