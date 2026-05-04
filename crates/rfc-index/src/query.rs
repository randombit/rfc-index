use crate::error::{Error, Result};
use crate::model::{
    Author, Body, Counts, Date, Rfc, RfcQuery, SeriesKind, SubSeries, SubSeriesRef,
};
use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) fn last_synced_at(conn: &Connection) -> Result<Option<i64>> {
    let v: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key='index_synced_at'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    parse_epoch_meta("index_synced_at", v)
}

pub(crate) fn counts(conn: &Connection) -> Result<Counts> {
    let rfcs: u32 = conn.query_row("SELECT COUNT(*) FROM rfcs", [], |r| r.get(0))?;
    let bcps: u32 = conn.query_row(
        "SELECT COUNT(*) FROM sub_series WHERE series='BCP'",
        [],
        |r| r.get(0),
    )?;
    let stds: u32 = conn.query_row(
        "SELECT COUNT(*) FROM sub_series WHERE series='STD'",
        [],
        |r| r.get(0),
    )?;
    let fyis: u32 = conn.query_row(
        "SELECT COUNT(*) FROM sub_series WHERE series='FYI'",
        [],
        |r| r.get(0),
    )?;
    let bodies_cached: u32 = conn.query_row("SELECT COUNT(*) FROM rfc_bodies", [], |r| r.get(0))?;
    Ok(Counts {
        rfcs,
        bcps,
        stds,
        fyis,
        bodies_cached,
    })
}

pub(crate) fn get_rfc(conn: &Connection, number: u32) -> Result<Option<Rfc>> {
    let mut stmt = conn.prepare(
        "SELECT title, abstract_text, date_year, date_month, page_count, draft,
                current_status, publication_status, stream, area, wg, doi,
                has_errata, formats, has_xml
         FROM rfcs WHERE number=?1",
    )?;
    let row = stmt
        .query_row([number], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<i32>>(2)?,
                r.get::<_, Option<u8>>(3)?,
                r.get::<_, Option<u32>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, Option<String>>(8)?,
                r.get::<_, Option<String>>(9)?,
                r.get::<_, Option<String>>(10)?,
                r.get::<_, Option<String>>(11)?,
                r.get::<_, i64>(12)? != 0,
                r.get::<_, String>(13)?,
                r.get::<_, i64>(14)? != 0,
            ))
        })
        .optional()?;
    let Some((
        title,
        abstract_text,
        year,
        month,
        page_count,
        draft,
        current_status,
        publication_status,
        stream,
        area,
        wg,
        doi,
        has_errata,
        formats_csv,
        has_xml,
    )) = row
    else {
        return Ok(None);
    };

    let date = year.map(|y| Date { year: y, month });
    let formats: Vec<String> = if formats_csv.is_empty() {
        Vec::new()
    } else {
        formats_csv.split(',').map(|s| s.to_string()).collect()
    };
    let authors = load_authors(conn, number)?;
    let keywords = load_keywords(conn, number)?;
    let obsoletes = load_relations(conn, number, "obsoletes")?;
    let obsoleted_by = load_relations(conn, number, "obsoleted_by")?;
    let updates = load_relations(conn, number, "updates")?;
    let updated_by = load_relations(conn, number, "updated_by")?;
    let sub_series = load_sub_series_for_rfc(conn, number)?;

    Ok(Some(Rfc {
        number,
        title,
        abstract_text,
        date,
        page_count,
        draft,
        current_status,
        publication_status,
        stream,
        area,
        wg,
        doi,
        has_errata,
        formats,
        has_xml,
        authors,
        keywords,
        obsoletes,
        obsoleted_by,
        updates,
        updated_by,
        sub_series,
    }))
}

fn load_authors(conn: &Connection, n: u32) -> Result<Vec<Author>> {
    let mut stmt = conn.prepare("SELECT name, role FROM rfc_authors WHERE rfc=?1 ORDER BY ord")?;
    let rows = stmt.query_map([n], |r| {
        Ok(Author {
            name: r.get(0)?,
            role: r.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn load_keywords(conn: &Connection, n: u32) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT keyword FROM rfc_keywords WHERE rfc=?1 ORDER BY keyword")?;
    let rows = stmt.query_map([n], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn load_relations(conn: &Connection, n: u32, kind: &str) -> Result<Vec<u32>> {
    let mut stmt = conn.prepare(
        "SELECT to_rfc FROM rfc_relations WHERE from_rfc=?1 AND kind=?2 ORDER BY to_rfc",
    )?;
    let rows = stmt.query_map(params![n, kind], |r| r.get::<_, u32>(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn load_sub_series_for_rfc(conn: &Connection, n: u32) -> Result<Vec<SubSeriesRef>> {
    let mut stmt = conn.prepare(
        "SELECT s.doc_id, s.title FROM sub_series s
         JOIN sub_series_members m ON m.doc_id = s.doc_id
         WHERE m.rfc = ?1 ORDER BY s.doc_id",
    )?;
    let rows = stmt.query_map([n], |r| {
        Ok(SubSeriesRef {
            doc_id: r.get(0)?,
            title: r.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn list_rfcs(conn: &Connection, q: &RfcQuery) -> Result<Vec<Rfc>> {
    let title_re = match &q.title_regex {
        Some(p) => Some(Regex::new(&format!("(?i){p}"))?),
        None => None,
    };

    let mut sql = String::from("SELECT number FROM rfcs WHERE 1=1");
    if q.min_year.is_some() {
        sql.push_str(" AND date_year >= ?");
    }
    if q.status_contains.is_some() {
        sql.push_str(" AND lower(current_status) LIKE ?");
    }
    if q.xml_only {
        sql.push_str(" AND has_xml = 1");
    }
    sql.push_str(" ORDER BY number");

    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(y) = q.min_year {
        params.push(Box::new(y));
    }
    if let Some(s) = &q.status_contains {
        params.push(Box::new(format!("%{}%", s.to_lowercase())));
    }

    let mut stmt = conn.prepare(&sql)?;
    let numbers: Vec<u32> = stmt
        .query_map(
            rusqlite::params_from_iter(params.iter().map(|b| b.as_ref())),
            |r| r.get::<_, u32>(0),
        )?
        .collect::<rusqlite::Result<_>>()?;

    // `Some(0)` means "no limit" by convention (consistent with the CLI). Real
    // limits are checked AFTER pushing so they cap correctly.
    let limit = match q.limit {
        Some(0) | None => None,
        Some(n) => Some(n),
    };

    let mut out = Vec::new();
    for n in numbers {
        let Some(rfc) = get_rfc(conn, n)? else {
            continue;
        };
        if let Some(re) = &title_re {
            if !re.is_match(rfc.title()) {
                continue;
            }
        }
        out.push(rfc);
        if let Some(lim) = limit {
            if out.len() >= lim {
                break;
            }
        }
    }
    Ok(out)
}

pub(crate) fn has_body(conn: &Connection, n: u32) -> Result<bool> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM rfc_bodies WHERE rfc=?1", [n], |r| {
        r.get(0)
    })?;
    Ok(count > 0)
}

pub(crate) fn get_body(conn: &Connection, n: u32) -> Result<Option<Body>> {
    let row = conn
        .query_row(
            "SELECT body_text, body_xml, fetched_at FROM rfc_bodies WHERE rfc=?1",
            [n],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((text, xml, fetched_at)) = row else {
        return Ok(None);
    };
    let Some(text) = text else {
        return Ok(None);
    };
    Ok(Some(Body {
        number: n,
        text,
        xml,
        fetched_at,
    }))
}

pub(crate) fn save_body(conn: &Connection, body: &Body) -> Result<()> {
    conn.execute(
        "INSERT INTO rfc_bodies(rfc, body_text, body_xml, fetched_at)
         VALUES (?1,?2,?3,?4)
         ON CONFLICT(rfc) DO UPDATE SET
            body_text=excluded.body_text,
            body_xml=excluded.body_xml,
            fetched_at=excluded.fetched_at",
        params![body.number(), body.text(), body.xml(), body.fetched_at()],
    )?;
    Ok(())
}

pub(crate) fn get_sub_series(conn: &Connection, doc_id: &str) -> Result<Option<SubSeries>> {
    let mut stmt = conn.prepare("SELECT series, number, title FROM sub_series WHERE doc_id=?1")?;
    let row = stmt
        .query_row([doc_id], |r| {
            let series: String = r.get(0)?;
            let number: u32 = r.get(1)?;
            let title: Option<String> = r.get(2)?;
            Ok((series, number, title))
        })
        .optional()?;
    let Some((series_s, number, title)) = row else {
        return Ok(None);
    };
    let series = match series_s.as_str() {
        "BCP" => SeriesKind::Bcp,
        "STD" => SeriesKind::Std,
        "FYI" => SeriesKind::Fyi,
        _ => {
            return Err(Error::Malformed(format!(
                "invalid sub-series kind in DB: {series_s}"
            )));
        }
    };
    let mut mstmt =
        conn.prepare("SELECT rfc FROM sub_series_members WHERE doc_id=?1 ORDER BY rfc")?;
    let members: Vec<u32> = mstmt
        .query_map([doc_id], |r| r.get::<_, u32>(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some(SubSeries {
        doc_id: doc_id.to_string(),
        series,
        number,
        title,
        members,
    }))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_sync_timestamp_is_reported() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO meta(key, value) VALUES ('index_synced_at', 'nope');",
        )
        .unwrap();

        match last_synced_at(&conn) {
            Err(Error::Malformed(msg)) => assert!(msg.contains("index_synced_at")),
            other => panic!("expected malformed error, got {other:?}"),
        }
    }
}
