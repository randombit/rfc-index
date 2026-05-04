use crate::error::Result;
use crate::model::SearchHit;
use rusqlite::{Connection, OptionalExtension, params};

/// Re-index one RFC's full-text row from current `rfcs`/`rfc_keywords`/`rfc_bodies` state.
pub(crate) fn reindex(conn: &Connection, number: u32) -> Result<()> {
    let row: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT title, abstract_text FROM rfcs WHERE number=?1",
            [number],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
        )
        .optional()?;
    let Some((title, abstract_text)) = row else {
        return Ok(());
    };

    let keywords: String = {
        let mut stmt =
            conn.prepare("SELECT keyword FROM rfc_keywords WHERE rfc=?1 ORDER BY keyword")?;
        let rows: Vec<String> = stmt
            .query_map([number], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?;
        rows.join(", ")
    };

    let body: String = conn
        .query_row(
            "SELECT body_text FROM rfc_bodies WHERE rfc=?1",
            [number],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .unwrap_or_default();

    conn.execute("DELETE FROM rfc_search WHERE rowid=?1", [number])?;
    conn.execute(
        "INSERT INTO rfc_search(rowid, title, abstract, keywords, body) \
         VALUES (?1,?2,?3,?4,?5)",
        params![
            number,
            title,
            abstract_text.unwrap_or_default(),
            keywords,
            body
        ],
    )?;
    Ok(())
}

pub(crate) fn search(
    conn: &Connection,
    query: &str,
    limit: Option<usize>,
) -> Result<Vec<SearchHit>> {
    // First attempt: pass the query straight to FTS5 so power users can use
    // its full syntax (column filters, AND/OR/NOT, NEAR(), prefix `*`, …).
    let first = raw_search(conn, query, limit);
    if first.is_ok() {
        return first;
    }
    // Casual queries can contain characters that FTS5 treats as syntax —
    // hyphens (NOT prefix), colons (column filter), parens, etc. — yielding
    // confusing errors like `no such column: time` for "Real-time". Retry
    // with each whitespace token wrapped in double quotes so it's parsed as a
    // literal phrase. Falls through to the original error if nothing changed.
    let sanitized = quote_each_token(query);
    if sanitized == query {
        return first;
    }
    match raw_search(conn, &sanitized, limit) {
        Ok(hits) => Ok(hits),
        Err(_) => first,
    }
}

fn raw_search(conn: &Connection, query: &str, limit: Option<usize>) -> Result<Vec<SearchHit>> {
    // SQLite treats LIMIT -1 as no limit.
    let lim: i64 = match limit {
        Some(0) | None => -1,
        Some(n) => n as i64,
    };
    let mut stmt = conn.prepare(
        "SELECT rowid, \
                snippet(rfc_search, -1, '<<', '>>', '...', 16), \
                bm25(rfc_search) \
         FROM rfc_search \
         WHERE rfc_search MATCH ?1 \
         ORDER BY bm25(rfc_search) \
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![query, lim], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, f64>(2)?,
        ))
    })?;

    let hits: Vec<(u32, String, f64)> = rows.collect::<rusqlite::Result<_>>()?;
    let mut out = Vec::with_capacity(hits.len());
    for (number, snippet, score) in hits {
        let title: String =
            conn.query_row("SELECT title FROM rfcs WHERE number=?1", [number], |r| {
                r.get(0)
            })?;
        out.push(SearchHit {
            number,
            title,
            snippet,
            score,
        });
    }
    Ok(out)
}

/// Wrap each whitespace-separated token in double quotes (after stripping any
/// embedded quotes), turning the query into a sequence of FTS5 phrase literals
/// implicitly AND-joined. Used as a fallback when raw FTS5 parse fails.
fn quote_each_token(q: &str) -> String {
    q.split_whitespace()
        .map(|t| {
            let stripped = t.trim_matches('"');
            format!("\"{stripped}\"")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_each_handles_hyphens_and_colons() {
        assert_eq!(quote_each_token("Real-time"), r#""Real-time""#);
        assert_eq!(
            quote_each_token("Secure Real-time"),
            r#""Secure" "Real-time""#
        );
        assert_eq!(quote_each_token("title:QUIC"), r#""title:QUIC""#);
    }

    #[test]
    fn quote_each_strips_existing_quotes() {
        assert_eq!(quote_each_token(r#""foo""#), r#""foo""#);
    }
}
