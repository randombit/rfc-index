use rfc_index::{FacetKind, Result, RfcIndex, RfcQuery, SeriesKind};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const INDEX_ETAG: &str = "\"index-v1\"";
const INDEX_LAST_MODIFIED: &str = "Mon, 01 Jan 2024 00:00:00 GMT";
const ERRATA_ETAG: &str = "\"errata-v1\"";
const ERRATA_LAST_MODIFIED: &str = "Tue, 02 Jan 2024 00:00:00 GMT";

const RFC_INDEX_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rfc-index>
  <rfc-entry>
    <doc-id>RFC7000</doc-id>
    <title>Legacy Widget TLS</title>
    <author><name>O. Legacy</name></author>
    <date><month>March</month><year>2014</year></date>
    <current-status>HISTORIC</current-status>
    <publication-status>PROPOSED STANDARD</publication-status>
    <stream>IETF</stream>
    <area>sec</area>
    <wg_acronym>tls</wg_acronym>
    <format>
      <file-format>TEXT</file-format>
    </format>
    <abstract><p>Earlier TLS profile, now obsolete.</p></abstract>
    <obsoleted-by>
      <doc-id>RFC8446</doc-id>
    </obsoleted-by>
  </rfc-entry>
  <rfc-entry>
    <doc-id>RFC8446</doc-id>
    <title>Widget TLS Profile</title>
    <author><name>T. Handshake</name></author>
    <date><month>August</month><year>2018</year></date>
    <current-status>PROPOSED STANDARD</current-status>
    <publication-status>PROPOSED STANDARD</publication-status>
    <stream>IETF</stream>
    <area>sec</area>
    <wg_acronym>tls</wg_acronym>
    <format>
      <file-format>TEXT</file-format>
    </format>
    <abstract><p>TLS profile for widgets.</p></abstract>
    <obsoletes>
      <doc-id>RFC7000</doc-id>
    </obsoletes>
  </rfc-entry>
  <rfc-entry>
    <doc-id>RFC9000</doc-id>
    <title>Real-time Widget Transport</title>
    <author><name>A. Example</name></author>
    <author><name>E. Editor</name><title>Editor</title></author>
    <date><month>May</month><year>2021</year></date>
    <page-count>42</page-count>
    <draft>draft-ietf-widgets-transport-01</draft>
    <current-status>PROPOSED STANDARD</current-status>
    <publication-status>PROPOSED STANDARD</publication-status>
    <stream>IETF</stream>
    <area>art</area>
    <wg_acronym>widgets</wg_acronym>
    <doi>10.17487/RFC9000</doi>
    <format>
      <file-format>TEXT</file-format>
      <file-format>XML</file-format>
    </format>
    <abstract>
      <p>Transport for real-time widgets.</p>
      <p>Supports connection migration.</p>
    </abstract>
    <keywords>
      <kw>widgets</kw>
      <kw>realtime</kw>
    </keywords>
    <updates>
      <doc-id>RFC8446</doc-id>
    </updates>
    <errata-url>https://example.invalid/errata/rfc9000</errata-url>
  </rfc-entry>
  <rfc-entry>
    <doc-id>RFC9001</doc-id>
    <title>Widget Migration Guide</title>
    <author><name>M. Guide</name></author>
    <date><month>January</month><year>2024</year></date>
    <current-status>INFORMATIONAL</current-status>
    <publication-status>INFORMATIONAL</publication-status>
    <stream>IETF</stream>
    <format>
      <file-format>TEXT</file-format>
    </format>
    <abstract><p>Operational guidance for widget migration.</p></abstract>
    <keywords>
      <kw>migration</kw>
    </keywords>
  </rfc-entry>
  <bcp-entry>
    <doc-id>BCP0014</doc-id>
    <title>Widget Best Current Practices</title>
    <is-also>
      <doc-id>RFC9000</doc-id>
      <doc-id>RFC9001</doc-id>
    </is-also>
  </bcp-entry>
  <std-entry>
    <doc-id>STD0003</doc-id>
    <title>Widget Transport Standard</title>
    <is-also>
      <doc-id>RFC8446</doc-id>
    </is-also>
  </std-entry>
</rfc-index>
"#;

const RFC_9000_TEXT: &str = concat!(
    "1.  Introduction\r\n",
    "\r\n",
    "This is RFC 9000. It builds on RFC 8446 and RFC 9001.\r\n",
    "\u{000C}",
    "5.2.  Widget Frames\r\n",
    "\r\n",
    "moonshot frames carry ziggurat payloads.\r\n",
    "\r\n",
    "5.2.1.  Ack Widget\r\n",
    "\r\n",
    "Acknowledgements reuse RFC 8446 conventions.\r\n",
    "\r\n",
    "6.  Security Considerations\r\n",
    "\r\n",
    "Security text.\r\n",
);

const RFC_9000_XML: &str = r#"<rfc number="9000"><middle><section numbered="true" anchor="s-5.2"><name>Widget Frames</name></section></middle></rfc>"#;

const RFC_9001_TEXT: &str = concat!(
    "1.  Overview\n",
    "\n",
    "This companion guide depends on RFC 9000.\n",
    "\n",
    "2.  Deployment\n",
    "\n",
    "Deploy carefully.\n",
);

const ERRATA_JSON: &str = r#"[
  {
    "errata_id": "1000",
    "doc-id": "RFC9000",
    "errata_status_code": "Verified",
    "errata_type_code": "Technical",
    "section": "5.2",
    "orig_text": "moonshot frames",
    "correct_text": "moon-shot frames",
    "notes": "Hyphenation fix.",
    "submit_date": "2024-01-03",
    "update_date": "2024-01-04",
    "submitter_name": "A. Reviewer",
    "verifier_name": "V. Checker"
  },
  {
    "errata_id": "1001",
    "doc-id": "RFC9000",
    "errata_status_code": "Held for Document Update",
    "errata_type_code": "Editorial",
    "section": "1",
    "orig_text": "widget",
    "correct_text": "widgets",
    "notes": "Deferred until next revision."
  },
  {
    "errata_id": "oops",
    "doc-id": "RFC9001",
    "errata_status_code": "Reported",
    "errata_type_code": "Technical"
  }
]"#;

#[test]
fn sync_index_populates_queries_and_uses_head_probe_on_repeated_sync() -> Result<()> {
    let server = TestServer::spawn();
    let db = TempDb::new("sync-index");
    let mut idx = open_test_index(db.path(), &server)?;

    let first = idx.sync_index()?;
    assert_eq!(first.rfcs, 4);
    assert_eq!(first.bcps, 1);
    assert_eq!(first.stds, 1);
    assert_eq!(first.fyis, 0);
    assert!(!first.not_modified);
    assert_eq!(server.request_count("GET", "/in-notes/rfc-index.xml"), 1);
    assert_eq!(server.request_count("HEAD", "/in-notes/rfc-index.xml"), 0);

    let counts = idx.counts()?;
    assert_eq!(counts.rfcs, 4);
    assert_eq!(counts.bcps, 1);
    assert_eq!(counts.stds, 1);
    assert_eq!(counts.fyis, 0);
    assert_eq!(counts.bodies_cached, 0);
    assert!(idx.last_synced_at()?.is_some());

    let rfc = idx.get(9000)?.expect("RFC 9000 metadata");
    assert_eq!(rfc.title(), "Real-time Widget Transport");
    assert_eq!(
        rfc.abstract_text(),
        Some("Transport for real-time widgets.\n\nSupports connection migration.")
    );
    let date = rfc.date().expect("RFC 9000 date");
    assert_eq!(date.year, 2021);
    assert_eq!(date.month, Some(5));
    assert_eq!(rfc.page_count(), Some(42));
    assert_eq!(rfc.draft(), Some("draft-ietf-widgets-transport-01"));
    assert_eq!(rfc.current_status(), Some("PROPOSED STANDARD"));
    assert_eq!(rfc.publication_status(), Some("PROPOSED STANDARD"));
    assert_eq!(rfc.stream(), Some("IETF"));
    assert_eq!(rfc.area(), Some("art"));
    assert_eq!(rfc.wg(), Some("widgets"));
    assert_eq!(rfc.doi(), Some("10.17487/RFC9000"));
    assert!(rfc.has_errata());
    assert!(rfc.has_xml());
    assert_eq!(rfc.formats(), &["TEXT".to_string(), "XML".to_string()]);
    assert_eq!(rfc.authors().len(), 2);
    assert_eq!(rfc.authors()[0].name(), "A. Example");
    assert_eq!(rfc.authors()[1].role(), Some("Editor"));
    assert_eq!(
        rfc.keywords(),
        &["realtime".to_string(), "widgets".to_string()]
    );
    assert_eq!(rfc.updates(), &[8446]);
    assert_eq!(rfc.sub_series().len(), 1);
    assert_eq!(rfc.sub_series()[0].doc_id(), "BCP0014");
    assert_eq!(
        rfc.sub_series()[0].title(),
        Some("Widget Best Current Practices")
    );

    let sub = idx.get_sub_series("bcp 14")?.expect("BCP14");
    assert_eq!(sub.doc_id(), "BCP0014");
    assert_eq!(sub.series(), SeriesKind::Bcp);
    assert_eq!(sub.members(), &[9000, 9001]);

    let list = idx.list(&RfcQuery {
        title_regex: Some("real-time".into()),
        min_year: Some(2020),
        status_contains: Some("proposed".into()),
        xml_only: true,
        limit: Some(10),
        ..Default::default()
    })?;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].number(), 9000);

    let search = idx.search("Real-time", Some(5))?;
    assert_eq!(search.len(), 1);
    assert_eq!(search[0].number(), 9000);

    let second = idx.sync_index()?;
    assert!(second.not_modified);
    assert_eq!(second.bytes, 0);
    assert_eq!(server.request_count("GET", "/in-notes/rfc-index.xml"), 1);
    assert_eq!(server.request_count("HEAD", "/in-notes/rfc-index.xml"), 1);

    Ok(())
}

#[test]
fn ensure_body_caches_and_reindexes_search_and_references() -> Result<()> {
    let server = TestServer::spawn();
    let db = TempDb::new("body-cache");
    let mut idx = open_test_index(db.path(), &server)?;
    idx.sync_index()?;

    assert!(!idx.has_body(9000)?);
    assert!(idx.body(9000)?.is_none());
    assert!(idx.search("moonshot", Some(5))?.is_empty());

    let body = idx.ensure_body(9000)?;
    assert!(idx.has_body(9000)?);
    assert!(body.xml().is_some());
    assert!(!body.text().contains('\r'));
    assert!(!body.text().contains('\u{000C}'));
    assert_eq!(server.request_count("GET", "/rfc/rfc9000.txt"), 1);
    assert_eq!(server.request_count("GET", "/rfc/rfc9000.xml"), 1);

    let sections: Vec<_> = body
        .sections()
        .into_iter()
        .map(|s| (s.number.to_string(), s.title.to_string()))
        .collect();
    assert_eq!(
        sections,
        vec![
            ("1".into(), "Introduction".into()),
            ("5.2".into(), "Widget Frames".into()),
            ("5.2.1".into(), "Ack Widget".into()),
            ("6".into(), "Security Considerations".into()),
        ]
    );

    let section = body.section("5.2").expect("section 5.2");
    assert!(section.text.contains("5.2.1.  Ack Widget"));
    assert!(!section.text.contains("6.  Security Considerations"));

    let cached = idx.body(9000)?.expect("cached body");
    assert_eq!(cached.text(), body.text());
    assert_eq!(idx.references(9000)?, vec![8446, 9001]);
    assert!(idx.referenced_by(9000)?.is_empty());

    let body_search = idx.search("moonshot", Some(5))?;
    assert_eq!(body_search.len(), 1);
    assert_eq!(body_search[0].number(), 9000);
    assert!(
        body_search[0]
            .snippet()
            .to_ascii_lowercase()
            .contains("moonshot")
    );

    let cached_again = idx.ensure_body(9000)?;
    assert_eq!(cached_again.text(), body.text());
    assert_eq!(server.request_count("GET", "/rfc/rfc9000.txt"), 1);
    assert_eq!(server.request_count("GET", "/rfc/rfc9000.xml"), 1);

    let secondary = idx.ensure_body(9001)?;
    assert_eq!(secondary.xml(), None);
    assert_eq!(server.request_count("GET", "/rfc/rfc9001.txt"), 1);
    assert_eq!(server.request_count("GET", "/rfc/rfc9001.xml"), 0);
    assert_eq!(idx.referenced_by(9000)?, vec![9001]);

    Ok(())
}

#[test]
fn sync_errata_populates_queries_and_uses_head_probe_on_repeated_sync() -> Result<()> {
    let server = TestServer::spawn();
    let db = TempDb::new("sync-errata");
    let mut idx = open_test_index(db.path(), &server)?;

    let first = idx.sync_errata()?;
    assert_eq!(first.errata, 2);
    assert!(!first.not_modified);
    assert_eq!(server.request_count("GET", "/errata.json"), 1);
    assert_eq!(server.request_count("HEAD", "/errata.json"), 0);
    assert!(idx.last_errata_synced_at()?.is_some());

    let by_rfc = idx.errata(9000)?;
    assert_eq!(by_rfc.len(), 2);
    assert_eq!(by_rfc[0].eid(), 1000);
    assert!(by_rfc[0].is_verified());
    assert_eq!(by_rfc[1].eid(), 1001);
    assert!(by_rfc[1].is_held());

    let one = idx.erratum(1000)?.expect("erratum 1000");
    assert_eq!(one.rfc(), 9000);
    assert_eq!(one.kind(), "Technical");
    assert_eq!(one.section(), Some("5.2"));
    assert_eq!(one.correct_text(), Some("moon-shot frames"));
    assert_eq!(one.submitter(), Some("A. Reviewer"));
    assert_eq!(one.verifier(), Some("V. Checker"));

    let second = idx.sync_errata()?;
    assert!(second.not_modified);
    assert_eq!(second.bytes, 0);
    assert_eq!(server.request_count("GET", "/errata.json"), 1);
    assert_eq!(server.request_count("HEAD", "/errata.json"), 1);

    Ok(())
}

#[test]
fn discovery_filters_compose() -> Result<()> {
    let server = TestServer::spawn();
    let db = TempDb::new("discovery-filters");
    let mut idx = open_test_index(db.path(), &server)?;
    idx.sync_index()?;

    let by_wg = idx.list(&RfcQuery {
        wg: Some("TLS".into()),
        ..Default::default()
    })?;
    let nums: Vec<u32> = by_wg.iter().map(|r| r.number()).collect();
    assert_eq!(nums, vec![7000, 8446]);

    let by_area = idx.list(&RfcQuery {
        area: Some("sec".into()),
        ..Default::default()
    })?;
    assert_eq!(
        by_area.iter().map(|r| r.number()).collect::<Vec<_>>(),
        vec![7000, 8446]
    );

    let by_keyword = idx.list(&RfcQuery {
        keyword: Some("realtime".into()),
        ..Default::default()
    })?;
    assert_eq!(
        by_keyword.iter().map(|r| r.number()).collect::<Vec<_>>(),
        vec![9000]
    );

    let bcps = idx.list(&RfcQuery {
        series: Some(SeriesKind::Bcp),
        ..Default::default()
    })?;
    assert_eq!(
        bcps.iter().map(|r| r.number()).collect::<Vec<_>>(),
        vec![9000, 9001]
    );

    let stds = idx.list(&RfcQuery {
        series: Some(SeriesKind::Std),
        ..Default::default()
    })?;
    assert_eq!(
        stds.iter().map(|r| r.number()).collect::<Vec<_>>(),
        vec![8446]
    );

    let active = idx.list(&RfcQuery {
        wg: Some("tls".into()),
        not_obsoleted: true,
        ..Default::default()
    })?;
    assert_eq!(
        active.iter().map(|r| r.number()).collect::<Vec<_>>(),
        vec![8446]
    );

    let bounded = idx.list(&RfcQuery {
        min_year: Some(2018),
        max_year: Some(2021),
        ..Default::default()
    })?;
    assert_eq!(
        bounded.iter().map(|r| r.number()).collect::<Vec<_>>(),
        vec![8446, 9000]
    );

    let by_author = idx.list(&RfcQuery {
        author_regex: Some("legacy".into()),
        ..Default::default()
    })?;
    assert_eq!(
        by_author.iter().map(|r| r.number()).collect::<Vec<_>>(),
        vec![7000]
    );

    let by_abs = idx.list(&RfcQuery {
        abstract_regex: Some(r"connection\s+migration".into()),
        ..Default::default()
    })?;
    assert_eq!(
        by_abs.iter().map(|r| r.number()).collect::<Vec<_>>(),
        vec![9000]
    );

    Ok(())
}

#[test]
fn facets_enumerate_distinct_values_with_counts() -> Result<()> {
    let server = TestServer::spawn();
    let db = TempDb::new("facets");
    let mut idx = open_test_index(db.path(), &server)?;
    idx.sync_index()?;

    let wgs = idx.facets(FacetKind::WorkingGroup, None)?;
    let pairs: Vec<(String, u32)> = wgs
        .iter()
        .map(|f| (f.value().to_string(), f.count()))
        .collect();
    // Sorted by descending count, then lexicographic.
    assert_eq!(pairs, vec![("tls".into(), 2u32), ("widgets".into(), 1u32)]);

    let pkix_search = idx.facets(FacetKind::WorkingGroup, Some("TL"))?;
    assert_eq!(pkix_search.len(), 1);
    assert_eq!(pkix_search[0].value(), "tls");
    assert_eq!(pkix_search[0].count(), 2);

    let streams = idx.facets(FacetKind::Stream, None)?;
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].value(), "IETF");
    assert_eq!(streams[0].count(), 4);

    let keywords = idx.facets(FacetKind::Keyword, None)?;
    let kw_values: Vec<String> = keywords.iter().map(|f| f.value().to_string()).collect();
    assert!(kw_values.contains(&"widgets".to_string()));
    assert!(kw_values.contains(&"migration".to_string()));
    assert!(kw_values.contains(&"realtime".to_string()));

    let statuses = idx.facets(FacetKind::Status, None)?;
    let by_value: std::collections::HashMap<String, u32> = statuses
        .iter()
        .map(|f| (f.value().to_string(), f.count()))
        .collect();
    assert_eq!(by_value.get("PROPOSED STANDARD").copied(), Some(2));
    assert_eq!(by_value.get("HISTORIC").copied(), Some(1));
    assert_eq!(by_value.get("INFORMATIONAL").copied(), Some(1));

    let no_match = idx.facets(FacetKind::WorkingGroup, Some("nonsuch"))?;
    assert!(no_match.is_empty());

    Ok(())
}

fn open_test_index(path: &Path, server: &TestServer) -> Result<RfcIndex> {
    let mut idx = RfcIndex::open_with_base_url(path, &server.base_url())?;
    idx.set_min_fetch_interval(Duration::from_millis(0));
    Ok(idx)
}

struct TempDb {
    dir: PathBuf,
    path: PathBuf,
}

impl TempDb {
    fn new(label: &str) -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "rfc-index-it-{label}-{}-{}",
            std::process::id(),
            id
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self {
            path: dir.join("rfcs.db"),
            dir,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct TestServer {
    addr: SocketAddr,
    counts: Arc<Mutex<HashMap<(String, String), usize>>>,
    shutdown: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl TestServer {
    fn spawn() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("server addr");
        let counts = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));

        let thread_counts = Arc::clone(&counts);
        let thread_shutdown = Arc::clone(&shutdown);
        let join = thread::spawn(move || {
            loop {
                let (stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(_) => break,
                };
                if thread_shutdown.load(Ordering::SeqCst) {
                    break;
                }
                let counts = Arc::clone(&thread_counts);
                thread::spawn(move || {
                    let _ = handle_connection(stream, counts);
                });
            }
        });

        Self {
            addr,
            counts,
            shutdown,
            join: Some(join),
        }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn request_count(&self, method: &str, path: &str) -> usize {
        let counts = self.counts.lock().expect("request counts lock");
        counts
            .get(&(method.to_string(), path.to_string()))
            .copied()
            .unwrap_or(0)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    counts: Arc<Mutex<HashMap<(String, String), usize>>>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    if request_line.trim().is_empty() {
        return Ok(());
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();

    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        if header == "\r\n" || header.is_empty() {
            break;
        }
    }

    {
        let mut counts = counts.lock().expect("request counts lock");
        *counts.entry((method.clone(), path.clone())).or_insert(0) += 1;
    }

    let response = route_response(&method, &path);
    write_response(&mut stream, response, method == "HEAD")
}

struct Response {
    status: u16,
    content_type: &'static str,
    body: &'static str,
    etag: Option<&'static str>,
    last_modified: Option<&'static str>,
}

fn route_response(method: &str, path: &str) -> Response {
    match (method, path) {
        ("HEAD", "/in-notes/rfc-index.xml") | ("GET", "/in-notes/rfc-index.xml") => Response {
            status: 200,
            content_type: "application/xml",
            body: RFC_INDEX_XML,
            etag: Some(INDEX_ETAG),
            last_modified: Some(INDEX_LAST_MODIFIED),
        },
        ("HEAD", "/errata.json") | ("GET", "/errata.json") => Response {
            status: 200,
            content_type: "application/json",
            body: ERRATA_JSON,
            etag: Some(ERRATA_ETAG),
            last_modified: Some(ERRATA_LAST_MODIFIED),
        },
        ("GET", "/rfc/rfc9000.txt") => Response {
            status: 200,
            content_type: "text/plain",
            body: RFC_9000_TEXT,
            etag: None,
            last_modified: None,
        },
        ("GET", "/rfc/rfc9000.xml") => Response {
            status: 200,
            content_type: "application/xml",
            body: RFC_9000_XML,
            etag: None,
            last_modified: None,
        },
        ("GET", "/rfc/rfc9001.txt") => Response {
            status: 200,
            content_type: "text/plain",
            body: RFC_9001_TEXT,
            etag: None,
            last_modified: None,
        },
        _ => Response {
            status: 404,
            content_type: "text/plain",
            body: "not found",
            etag: None,
            last_modified: None,
        },
    }
}

fn write_response(
    stream: &mut TcpStream,
    response: Response,
    head_only: bool,
) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        404 => "Not Found",
        _ => "Error",
    };

    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        reason,
        response.content_type,
        response.body.len()
    )?;
    if let Some(etag) = response.etag {
        write!(stream, "ETag: {etag}\r\n")?;
    }
    if let Some(last_modified) = response.last_modified {
        write!(stream, "Last-Modified: {last_modified}\r\n")?;
    }
    write!(stream, "\r\n")?;
    if !head_only {
        stream.write_all(response.body.as_bytes())?;
    }
    stream.flush()
}
