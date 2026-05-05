use crate::db::now_epoch;
use crate::error::Result;
use crate::http::{HeadProbe, head_probe, validators_match};
use crate::model::{SeriesKind, SyncStats};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use roxmltree::{Document, Node};
use rusqlite::{Connection, params};

pub(crate) fn sync(conn: &mut Connection, client: &Client, base_url: &str) -> Result<SyncStats> {
    let index_url = format!("{base_url}/in-notes/rfc-index.xml");
    let prior_etag = read_meta(conn, "index_etag")?;
    let prior_last_modified = read_meta(conn, "index_last_modified")?;

    // Workaround: rfc-editor.org's front-end currently returns 200 with the
    // full body on conditional GET even with correct etag
    if prior_etag.is_some() || prior_last_modified.is_some() {
        if let Some(HeadProbe {
            etag,
            last_modified,
        }) = head_probe(client, &index_url)?
        {
            if validators_match(&prior_etag, &etag, &prior_last_modified, &last_modified) {
                write_meta(conn, "index_synced_at", &now_epoch().to_string())?;
                return Ok(SyncStats {
                    not_modified: true,
                    ..SyncStats::default()
                });
            }
        }
    }

    let mut req = client.get(&index_url);
    if let Some(etag) = &prior_etag {
        req = req.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    if let Some(lm) = &prior_last_modified {
        req = req.header(reqwest::header::IF_MODIFIED_SINCE, lm);
    }
    let resp = req.send()?;

    if resp.status() == StatusCode::NOT_MODIFIED {
        write_meta(conn, "index_synced_at", &now_epoch().to_string())?;
        return Ok(SyncStats {
            not_modified: true,
            ..SyncStats::default()
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

    let xml = resp.text()?;
    let bytes = xml.len();
    let stats = ingest(conn, &xml)?;

    if let Some(etag) = etag {
        write_meta(conn, "index_etag", &etag)?;
    }
    if let Some(lm) = last_modified {
        write_meta(conn, "index_last_modified", &lm)?;
    }
    write_meta(conn, "index_synced_at", &now_epoch().to_string())?;

    Ok(SyncStats { bytes, ..stats })
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

struct ParsedRfc {
    number: u32,
    title: String,
    abstract_text: Option<String>,
    date_year: Option<i32>,
    date_month: Option<u8>,
    page_count: Option<u32>,
    draft: Option<String>,
    current_status: Option<String>,
    publication_status: Option<String>,
    stream: Option<String>,
    area: Option<String>,
    wg: Option<String>,
    doi: Option<String>,
    has_errata: bool,
    formats: Vec<String>,
    has_xml: bool,
    authors: Vec<ParsedAuthor>,
    keywords: Vec<String>,
    obsoletes: Vec<u32>,
    obsoleted_by: Vec<u32>,
    updates: Vec<u32>,
    updated_by: Vec<u32>,
}

struct ParsedAuthor {
    name: String,
    role: Option<String>,
}

struct ParsedSub {
    doc_id: String,
    series: SeriesKind,
    number: u32,
    title: Option<String>,
    members: Vec<u32>,
}

fn ingest(conn: &mut Connection, xml: &str) -> Result<SyncStats> {
    let doc = Document::parse(xml)?;
    let root = doc.root_element();

    let tx = conn.transaction()?;

    let mut rfc_count = 0usize;
    let mut bcp_count = 0usize;
    let mut std_count = 0usize;
    let mut fyi_count = 0usize;

    {
        let mut up_rfc = tx.prepare(
            "INSERT INTO rfcs (number, title, abstract_text, date_year, date_month,
                page_count, draft, current_status, publication_status, stream, area, wg, doi,
                has_errata, formats, has_xml)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
             ON CONFLICT(number) DO UPDATE SET
                title=excluded.title,
                abstract_text=excluded.abstract_text,
                date_year=excluded.date_year,
                date_month=excluded.date_month,
                page_count=excluded.page_count,
                draft=excluded.draft,
                current_status=excluded.current_status,
                publication_status=excluded.publication_status,
                stream=excluded.stream,
                area=excluded.area,
                wg=excluded.wg,
                doi=excluded.doi,
                has_errata=excluded.has_errata,
                formats=excluded.formats,
                has_xml=excluded.has_xml",
        )?;
        let mut del_authors = tx.prepare("DELETE FROM rfc_authors WHERE rfc=?1")?;
        let mut ins_author =
            tx.prepare("INSERT INTO rfc_authors(rfc, ord, name, role) VALUES (?1,?2,?3,?4)")?;
        let mut del_keywords = tx.prepare("DELETE FROM rfc_keywords WHERE rfc=?1")?;
        let mut ins_keyword =
            tx.prepare("INSERT OR IGNORE INTO rfc_keywords(rfc, keyword) VALUES (?1,?2)")?;
        let mut del_relations = tx.prepare("DELETE FROM rfc_relations WHERE from_rfc=?1")?;
        let mut ins_relation = tx.prepare(
            "INSERT OR IGNORE INTO rfc_relations(from_rfc, to_rfc, kind) VALUES (?1,?2,?3)",
        )?;

        let mut up_sub = tx.prepare(
            "INSERT INTO sub_series(doc_id, series, number, title) VALUES (?1,?2,?3,?4)
             ON CONFLICT(doc_id) DO UPDATE SET title=excluded.title",
        )?;
        let mut del_members = tx.prepare("DELETE FROM sub_series_members WHERE doc_id=?1")?;
        let mut ins_member =
            tx.prepare("INSERT OR IGNORE INTO sub_series_members(doc_id, rfc) VALUES (?1,?2)")?;

        for node in root.children().filter(|n| n.is_element()) {
            match node.tag_name().name() {
                "rfc-entry" => {
                    if let Some(p) = parse_rfc_entry(node) {
                        up_rfc.execute(params![
                            p.number,
                            p.title,
                            p.abstract_text,
                            p.date_year,
                            p.date_month,
                            p.page_count,
                            p.draft,
                            p.current_status,
                            p.publication_status,
                            p.stream,
                            p.area,
                            p.wg,
                            p.doi,
                            p.has_errata as i64,
                            p.formats.join(","),
                            p.has_xml as i64,
                        ])?;
                        del_authors.execute([p.number])?;
                        for (i, a) in p.authors.iter().enumerate() {
                            ins_author.execute(params![p.number, i as i64, a.name, a.role])?;
                        }
                        del_keywords.execute([p.number])?;
                        for k in &p.keywords {
                            ins_keyword.execute(params![p.number, k])?;
                        }
                        del_relations.execute([p.number])?;
                        for n in &p.obsoletes {
                            ins_relation.execute(params![p.number, n, "obsoletes"])?;
                        }
                        for n in &p.obsoleted_by {
                            ins_relation.execute(params![p.number, n, "obsoleted_by"])?;
                        }
                        for n in &p.updates {
                            ins_relation.execute(params![p.number, n, "updates"])?;
                        }
                        for n in &p.updated_by {
                            ins_relation.execute(params![p.number, n, "updated_by"])?;
                        }
                        crate::search::reindex(&tx, p.number)?;
                        rfc_count += 1;
                    }
                }
                tag @ ("bcp-entry" | "std-entry" | "fyi-entry") => {
                    let kind = match tag {
                        "bcp-entry" => SeriesKind::Bcp,
                        "std-entry" => SeriesKind::Std,
                        _ => SeriesKind::Fyi,
                    };
                    if let Some(s) = parse_sub_entry(node, kind) {
                        up_sub.execute(params![s.doc_id, s.series.as_str(), s.number, s.title])?;
                        del_members.execute([&s.doc_id])?;
                        for r in &s.members {
                            ins_member.execute(params![s.doc_id, r])?;
                        }
                        match kind {
                            SeriesKind::Bcp => bcp_count += 1,
                            SeriesKind::Std => std_count += 1,
                            SeriesKind::Fyi => fyi_count += 1,
                        }
                    }
                }
                _ => {}
            }
        }
    }

    tx.commit()?;
    Ok(SyncStats {
        rfcs: rfc_count,
        bcps: bcp_count,
        stds: std_count,
        fyis: fyi_count,
        bytes: 0,
        not_modified: false,
    })
}

fn parse_rfc_entry(node: Node) -> Option<ParsedRfc> {
    let doc_id = child_text(node, "doc-id")?;
    let number = parse_rfc_doc_id(&doc_id)?;
    let title = child_text(node, "title").unwrap_or_default();

    let (date_year, date_month) = match child_node(node, "date") {
        Some(d) => {
            let year = child_text(d, "year").and_then(|s| s.parse().ok());
            let month = child_text(d, "month").as_deref().and_then(month_to_num);
            (year, month)
        }
        None => (None, None),
    };

    let abstract_text = child_node(node, "abstract").map(|a| {
        a.children()
            .filter(|c| c.is_element() && c.tag_name().name() == "p")
            .filter_map(|p| p.text().map(|s| s.trim().to_string()))
            .collect::<Vec<_>>()
            .join("\n\n")
    });

    let formats: Vec<String> = child_node(node, "format")
        .map(|f| {
            f.children()
                .filter(|c| c.is_element() && c.tag_name().name() == "file-format")
                .filter_map(|c| c.text().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let has_xml = formats.iter().any(|f| f.eq_ignore_ascii_case("XML"));

    let authors = node
        .children()
        .filter(|c| c.is_element() && c.tag_name().name() == "author")
        .filter_map(|a| {
            let name = child_text(a, "name")?;
            let role = child_text(a, "title");
            Some(ParsedAuthor { name, role })
        })
        .collect();

    let keywords = child_node(node, "keywords")
        .map(|k| {
            k.children()
                .filter(|c| c.is_element() && c.tag_name().name() == "kw")
                .filter_map(|c| c.text().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let collect_doc_ids = |name: &str| -> Vec<u32> {
        child_node(node, name)
            .map(|n| {
                n.children()
                    .filter(|c| c.is_element() && c.tag_name().name() == "doc-id")
                    .filter_map(|c| c.text())
                    .filter_map(parse_rfc_doc_id)
                    .collect()
            })
            .unwrap_or_default()
    };

    Some(ParsedRfc {
        number,
        title,
        abstract_text,
        date_year,
        date_month,
        page_count: child_text(node, "page-count").and_then(|s| s.parse().ok()),
        draft: child_text(node, "draft"),
        current_status: child_text(node, "current-status"),
        publication_status: child_text(node, "publication-status"),
        stream: child_text(node, "stream"),
        area: child_text(node, "area"),
        wg: child_text(node, "wg_acronym"),
        doi: child_text(node, "doi"),
        has_errata: child_node(node, "errata-url").is_some(),
        formats,
        has_xml,
        authors,
        keywords,
        obsoletes: collect_doc_ids("obsoletes"),
        obsoleted_by: collect_doc_ids("obsoleted-by"),
        updates: collect_doc_ids("updates"),
        updated_by: collect_doc_ids("updated-by"),
    })
}

fn parse_sub_entry(node: Node, kind: SeriesKind) -> Option<ParsedSub> {
    let doc_id = child_text(node, "doc-id")?;
    let number = parse_sub_doc_id(&doc_id, kind.as_str())?;
    let title = child_text(node, "title");
    let members = child_node(node, "is-also")
        .map(|n| {
            n.children()
                .filter(|c| c.is_element() && c.tag_name().name() == "doc-id")
                .filter_map(|c| c.text())
                .filter_map(parse_rfc_doc_id)
                .collect()
        })
        .unwrap_or_default();
    Some(ParsedSub {
        doc_id,
        series: kind,
        number,
        title,
        members,
    })
}

fn child_node<'a, 'b>(node: Node<'a, 'b>, name: &str) -> Option<Node<'a, 'b>> {
    node.children()
        .find(|c| c.is_element() && c.tag_name().name() == name)
}

fn child_text(node: Node, name: &str) -> Option<String> {
    child_node(node, name).and_then(|c| c.text().map(|s| s.trim().to_string()))
}

fn parse_rfc_doc_id(s: impl AsRef<str>) -> Option<u32> {
    s.as_ref()
        .strip_prefix("RFC")
        .and_then(|rest| rest.parse().ok())
}

fn parse_sub_doc_id(s: &str, prefix: &str) -> Option<u32> {
    s.strip_prefix(prefix).and_then(|rest| rest.parse().ok())
}

fn month_to_num(m: &str) -> Option<u8> {
    match m.trim().to_ascii_lowercase().as_str() {
        "january" => Some(1),
        "february" => Some(2),
        "march" => Some(3),
        "april" => Some(4),
        "may" => Some(5),
        "june" => Some(6),
        "july" => Some(7),
        "august" => Some(8),
        "september" => Some(9),
        "october" => Some(10),
        "november" => Some(11),
        "december" => Some(12),
        _ => None,
    }
}
