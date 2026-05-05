use crate::db::now_epoch;
use crate::error::{Error, Result};
use crate::http::{HeadProbe, head_probe, validators_match};
use crate::model::{ErrataSyncStats, Erratum};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;

pub(crate) fn sync(
    conn: &mut Connection,
    client: &Client,
    base_url: &str,
) -> Result<ErrataSyncStats> {
    let errata_url = format!("{base_url}/errata.json");
    let prior_etag = read_meta(conn, "errata_etag")?;
    let prior_lm = read_meta(conn, "errata_last_modified")?;

    if prior_etag.is_some() || prior_lm.is_some() {
        if let Some(HeadProbe {
            etag,
            last_modified,
        }) = head_probe(client, &errata_url)?
        {
            if validators_match(&prior_etag, &etag, &prior_lm, &last_modified) {
                write_meta(conn, "errata_synced_at", &now_epoch().to_string())?;
                return Ok(ErrataSyncStats {
                    not_modified: true,
                    ..ErrataSyncStats::default()
                });
            }
        }
    }

    let mut req = client.get(&errata_url);
    if let Some(etag) = &prior_etag {
        req = req.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    if let Some(lm) = &prior_lm {
        req = req.header(reqwest::header::IF_MODIFIED_SINCE, lm);
    }
    let resp = req.send()?;

    if resp.status() == StatusCode::NOT_MODIFIED {
        write_meta(conn, "errata_synced_at", &now_epoch().to_string())?;
        return Ok(ErrataSyncStats {
            not_modified: true,
            ..ErrataSyncStats::default()
        });
    }
    let resp = resp.error_for_status()?;

    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let last_modified = resp
        .headers()
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let bytes = resp.bytes()?;
    let raw_size = bytes.len();
    let parsed: Vec<RawErratum> = serde_json::from_slice(&bytes)
        .map_err(|e| crate::Error::Malformed(format!("errata.json: {e}")))?;

    let count = ingest(conn, &parsed)?;

    if let Some(e) = etag {
        write_meta(conn, "errata_etag", &e)?;
    }
    if let Some(lm) = last_modified {
        write_meta(conn, "errata_last_modified", &lm)?;
    }
    write_meta(conn, "errata_synced_at", &now_epoch().to_string())?;

    Ok(ErrataSyncStats {
        errata: count,
        bytes: raw_size,
        not_modified: false,
    })
}

fn ingest(conn: &mut Connection, raw: &[RawErratum]) -> Result<usize> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM errata", [])?;
    let mut count = 0usize;
    {
        let mut ins = tx.prepare(
            "INSERT INTO errata (
                eid, rfc, status, kind, section, orig_text, correct_text, notes,
                submitted, updated, submitter, verifier
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        )?;
        for e in raw {
            let Some(eid) = e.errata_id.as_ref().and_then(|s| s.parse::<u32>().ok()) else {
                continue;
            };
            let Some(rfc) = parse_rfc_doc_id(&e.doc_id) else {
                continue;
            };
            ins.execute(params![
                eid,
                rfc,
                e.errata_status_code.as_deref().unwrap_or(""),
                e.errata_type_code.as_deref().unwrap_or(""),
                e.section,
                e.orig_text,
                e.correct_text,
                e.notes,
                e.submit_date,
                e.update_date,
                e.submitter_name,
                e.verifier_name,
            ])?;
            count += 1;
        }
    }
    tx.commit()?;
    Ok(count)
}

pub(crate) fn for_rfc(conn: &Connection, rfc: u32) -> Result<Vec<Erratum>> {
    let mut stmt = conn.prepare(
        "SELECT eid, rfc, status, kind, section, orig_text, correct_text, notes,
                submitted, updated, submitter, verifier
         FROM errata WHERE rfc=?1 ORDER BY eid",
    )?;
    let rows = stmt.query_map([rfc], row_to_erratum)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn by_eid(conn: &Connection, eid: u32) -> Result<Option<Erratum>> {
    let mut stmt = conn.prepare(
        "SELECT eid, rfc, status, kind, section, orig_text, correct_text, notes,
                submitted, updated, submitter, verifier
         FROM errata WHERE eid=?1",
    )?;
    Ok(stmt.query_row([eid], row_to_erratum).optional()?)
}

pub(crate) fn last_synced_at(conn: &Connection) -> Result<Option<i64>> {
    parse_epoch_meta("errata_synced_at", read_meta(conn, "errata_synced_at")?)
}

fn row_to_erratum(r: &rusqlite::Row<'_>) -> rusqlite::Result<Erratum> {
    Ok(Erratum {
        eid: r.get(0)?,
        rfc: r.get(1)?,
        status: r.get(2)?,
        kind: r.get(3)?,
        section: r.get(4)?,
        orig_text: r.get(5)?,
        correct_text: r.get(6)?,
        notes: r.get(7)?,
        submitted: r.get(8)?,
        updated: r.get(9)?,
        submitter: r.get(10)?,
        verifier: r.get(11)?,
    })
}

fn read_meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
        .ok())
}

fn write_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO meta(key,value) VALUES(?1,?2) \
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [key, value],
    )?;
    Ok(())
}

fn parse_epoch_meta(key: &str, value: Option<String>) -> Result<Option<i64>> {
    match value {
        Some(s) => s.parse::<i64>().map(Some).map_err(|e| {
            Error::Malformed(format!(
                "meta[{key}] is not valid epoch seconds ({s:?}): {e}"
            ))
        }),
        None => Ok(None),
    }
}

fn parse_rfc_doc_id(s: &str) -> Option<u32> {
    s.strip_prefix("RFC").and_then(|rest| rest.parse().ok())
}

#[derive(Debug, Deserialize)]
struct RawErratum {
    errata_id: Option<String>,
    #[serde(rename = "doc-id")]
    doc_id: String,
    errata_status_code: Option<String>,
    errata_type_code: Option<String>,
    section: Option<String>,
    orig_text: Option<String>,
    correct_text: Option<String>,
    notes: Option<String>,
    submit_date: Option<String>,
    update_date: Option<String>,
    submitter_name: Option<String>,
    verifier_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_errata_timestamp_is_reported() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO meta(key, value) VALUES ('errata_synced_at', 'bad');",
        )
        .unwrap();

        match last_synced_at(&conn) {
            Err(Error::Malformed(msg)) => assert!(msg.contains("errata_synced_at")),
            other => panic!("expected malformed error, got {other:?}"),
        }
    }
}
