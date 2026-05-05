//! Local cache, full-text search, and reference graph for IETF RFCs.
//!
//! [`RfcIndex`] owns a SQLite database and a polite HTTP client. It ingests
//! the published metadata index (`rfc-index.xml`), lazily fetches and caches
//! RFC bodies (rendered text and, when available, xml2rfc source), parses
//! sections from the rendered text, extracts body-derived reference edges,
//! and indexes everything into FTS5 for full-text search.
//!
//! ```no_run
//! use rfc_index::{RfcIndex, RfcQuery};
//!
//! # fn main() -> rfc_index::Result<()> {
//! let mut idx = RfcIndex::open_default()?;
//! idx.sync_index()?;
//!
//! let r = idx.get(9000)?.expect("RFC 9000");
//! println!("{} — {}", r.number(), r.title());
//!
//! let body = idx.ensure_body(9000)?;
//! if let Some(s) = body.section("5.2") {
//!     println!("{}", s.text);
//! }
//!
//! for hit in idx.search("connection migration", Some(5))? {
//!     println!("RFC{}: {}", hit.number(), hit.title());
//! }
//! # let _ = RfcQuery::default();
//! # Ok(())
//! # }
//! ```
//!
//! All SQL and HTTP details live behind the [`RfcIndex`] surface; the CLI
//! (`rfc`) and the MCP server (`rfc-mcp`) are thin wrappers over this same API.
#![deny(missing_docs)]

mod body;
mod db;
mod errata;
mod error;
mod http;
mod index;
mod model;
mod query;
mod refs;
mod search;

pub use error::{Error, Result};
pub use model::{
    Author, Body, Counts, Date, ErrataSyncStats, Erratum, Rfc, RfcQuery, SearchHit, SectionRef,
    SeriesKind, SubSeries, SubSeriesRef, SyncStats,
};

use reqwest::blocking::Client;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const DEFAULT_FETCH_INTERVAL_MS: u64 = 20;
const DEFAULT_BASE_URL: &str = "https://www.rfc-editor.org";

/// Handle to a local RFC index database.
///
/// Reads take `&self`; mutating operations (e.g. `sync_index`, `fetch_body`) take `&mut self`.
pub struct RfcIndex {
    conn: Connection,
    path: PathBuf,
    client: Client,
    base_url: String,
    last_fetch: Option<Instant>,
    min_interval: Duration,
}

impl RfcIndex {
    /// Open (and migrate) the database at `path`. Creates the file if missing.
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_base_url(path, DEFAULT_BASE_URL)
    }

    /// Open (and migrate) the database at `path`, fetching metadata and bodies
    /// from `base_url` instead of the default RFC Editor origin.
    pub fn open_with_base_url(path: &Path, base_url: &str) -> Result<Self> {
        let conn = db::open(path)?;
        let client = build_client()?;
        Ok(Self {
            conn,
            path: path.to_path_buf(),
            client,
            base_url: normalize_base_url(base_url)?,
            last_fetch: None,
            min_interval: Duration::from_millis(DEFAULT_FETCH_INTERVAL_MS),
        })
    }

    /// Open the default-location database (`$XDG_DATA_HOME/rfc-index/rfcs.db` on Linux).
    /// Honors `RFC_INDEX_DB` if set.
    pub fn open_default() -> Result<Self> {
        Self::open(&default_db_path())
    }

    /// Path the default-location database lives at. Honors `RFC_INDEX_DB`.
    pub fn default_db_path() -> PathBuf {
        default_db_path()
    }

    /// Path of the database currently open.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Set the minimum interval between body fetches (politeness throttle).
    /// Default is 20 ms.
    pub fn set_min_fetch_interval(&mut self, interval: Duration) {
        self.min_interval = interval;
    }

    /// Download `rfc-index.xml` and ingest all metadata. Sends conditional GET
    /// headers (If-None-Match / If-Modified-Since) when prior values are known;
    /// returns `not_modified=true` and skips ingest on 304.
    pub fn sync_index(&mut self) -> Result<SyncStats> {
        index::sync(&mut self.conn, &self.client, &self.base_url)
    }

    /// Epoch seconds of the most recent successful `sync_index`, or `None` if never synced.
    pub fn last_synced_at(&self) -> Result<Option<i64>> {
        query::last_synced_at(&self.conn)
    }

    /// Aggregate counts of indexed records.
    pub fn counts(&self) -> Result<Counts> {
        query::counts(&self.conn)
    }

    /// Look up a single RFC by number; `None` if not in the index.
    pub fn get(&self, number: u32) -> Result<Option<Rfc>> {
        query::get_rfc(&self.conn, number)
    }

    /// List RFCs matching `query`, ordered by number.
    pub fn list(&self, query: &RfcQuery) -> Result<Vec<Rfc>> {
        query::list_rfcs(&self.conn, query)
    }

    /// Look up a sub-series record (BCP/STD/FYI). Accepts the canonical doc-id
    /// (`"BCP0014"`), short form (`"BCP14"`), or with a separator (`"BCP 14"`,
    /// `"std-3"`); case-insensitive.
    pub fn get_sub_series(&self, id: &str) -> Result<Option<SubSeries>> {
        match parse_sub_series_id(id) {
            Some(canonical) => query::get_sub_series(&self.conn, &canonical),
            None => Ok(None),
        }
    }

    /// Cached body lookup. `Ok(None)` if the body has not been fetched yet.
    pub fn body(&self, number: u32) -> Result<Option<Body>> {
        query::get_body(&self.conn, number)
    }

    /// Force-fetch the body from rfc-editor.org and cache it (overwrites any
    /// existing cache entry). Honors the configured fetch interval. Updates
    /// the FTS5 index and body-derived reference graph.
    pub fn fetch_body(&mut self, number: u32) -> Result<Body> {
        let has_xml = match self.get(number)? {
            Some(r) => r.has_xml(),
            None => {
                return Err(Error::NotFound(format!(
                    "RFC {number} not in index — run sync_index first"
                )));
            }
        };
        self.throttle();
        let body = body::fetch(&self.client, number, has_xml, &self.base_url)?;
        query::save_body(&self.conn, &body)?;
        refs::index_body(&self.conn, number, body.text())?;
        search::reindex(&self.conn, number)?;
        self.last_fetch = Some(Instant::now());
        Ok(body)
    }

    /// Return the cached body, fetching it if necessary.
    pub fn ensure_body(&mut self, number: u32) -> Result<Body> {
        if let Some(b) = query::get_body(&self.conn, number)? {
            return Ok(b);
        }
        self.fetch_body(number)
    }

    /// Has a body been cached for this RFC?
    pub fn has_body(&self, number: u32) -> Result<bool> {
        query::has_body(&self.conn, number)
    }

    /// Full-text search over title, abstract, keywords, and (when cached) body.
    /// `query` is passed verbatim to FTS5 — supports phrases, AND/OR/NOT,
    /// `NEAR()`, column filters (e.g. `title:QUIC`), etc. `limit = None` (or
    /// `Some(0)`) returns all matches.
    pub fn search(&self, query: &str, limit: Option<usize>) -> Result<Vec<SearchHit>> {
        search::search(&self.conn, query, limit)
    }

    /// RFCs whose body (when cached) mentions other RFCs — body-derived
    /// outgoing edges from `n`. Empty if `n`'s body has not been fetched.
    pub fn references(&self, n: u32) -> Result<Vec<u32>> {
        refs::references_from(&self.conn, n)
    }

    /// RFCs whose cached bodies mention `n` — body-derived incoming edges.
    /// Limited to the bodies you have fetched.
    pub fn referenced_by(&self, n: u32) -> Result<Vec<u32>> {
        refs::referenced_by(&self.conn, n)
    }

    /// Download `errata.json` and replace the local errata cache. Uses the
    /// same HEAD-probe + conditional GET workaround as `sync_index`.
    pub fn sync_errata(&mut self) -> Result<ErrataSyncStats> {
        errata::sync(&mut self.conn, &self.client, &self.base_url)
    }

    /// All errata for one RFC (locally cached). Run `sync_errata` first.
    pub fn errata(&self, rfc: u32) -> Result<Vec<Erratum>> {
        errata::for_rfc(&self.conn, rfc)
    }

    /// Single erratum by its global EID, if cached.
    pub fn erratum(&self, eid: u32) -> Result<Option<Erratum>> {
        errata::by_eid(&self.conn, eid)
    }

    /// Epoch seconds of the most recent successful `sync_errata`.
    pub fn last_errata_synced_at(&self) -> Result<Option<i64>> {
        errata::last_synced_at(&self.conn)
    }

    fn throttle(&self) {
        if let Some(last) = self.last_fetch {
            let elapsed = last.elapsed();
            if elapsed < self.min_interval {
                std::thread::sleep(self.min_interval - elapsed);
            }
        }
    }
}

fn build_client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(format!(
            "rfc-index/{} (+https://github.com/randombit/rfc-index)",
            env!("CARGO_PKG_VERSION")
        ))
        .gzip(true)
        .build()?)
}

fn normalize_base_url(base_url: &str) -> Result<String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(Error::Malformed("base URL must not be empty".into()));
    }
    Ok(trimmed.to_string())
}

/// "BCP14" / "bcp 14" / "std-3" / "BCP0014" → canonical "BCP0014" (similarly
/// for STD/FYI). Returns None if the input doesn't look like a sub-series id.
fn parse_sub_series_id(s: &str) -> Option<String> {
    let upper = s.trim().to_ascii_uppercase();
    for prefix in ["BCP", "STD", "FYI"] {
        if let Some(rest) = upper.strip_prefix(prefix) {
            let digits =
                rest.trim_start_matches(|c: char| c.is_whitespace() || c == '-' || c == '_');
            if let Ok(n) = digits.parse::<u32>() {
                return Some(format!("{prefix}{n:04}"));
            }
        }
    }
    None
}

fn default_db_path() -> PathBuf {
    if let Some(p) = std::env::var_os("RFC_INDEX_DB") {
        return PathBuf::from(p);
    }
    let dir = directories::ProjectDirs::from("", "", "rfc-index")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    dir.join("rfcs.db")
}
