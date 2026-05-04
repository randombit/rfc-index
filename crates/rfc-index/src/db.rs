use crate::Result;
use rusqlite::Connection;
use std::path::Path;

pub(crate) fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(conn)
}

pub(crate) fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS rfcs (
  number INTEGER PRIMARY KEY,
  title TEXT NOT NULL,
  abstract_text TEXT,
  date_year INTEGER,
  date_month INTEGER,
  page_count INTEGER,
  draft TEXT,
  current_status TEXT,
  publication_status TEXT,
  stream TEXT,
  area TEXT,
  wg TEXT,
  doi TEXT,
  has_errata INTEGER NOT NULL DEFAULT 0,
  formats TEXT NOT NULL DEFAULT '',
  has_xml INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS rfcs_date_year_idx ON rfcs(date_year);
CREATE INDEX IF NOT EXISTS rfcs_status_idx ON rfcs(current_status);

CREATE TABLE IF NOT EXISTS rfc_authors (
  rfc INTEGER NOT NULL REFERENCES rfcs(number) ON DELETE CASCADE,
  ord INTEGER NOT NULL,
  name TEXT NOT NULL,
  role TEXT,
  PRIMARY KEY (rfc, ord)
);

CREATE TABLE IF NOT EXISTS rfc_keywords (
  rfc INTEGER NOT NULL REFERENCES rfcs(number) ON DELETE CASCADE,
  keyword TEXT NOT NULL,
  PRIMARY KEY (rfc, keyword)
);

CREATE TABLE IF NOT EXISTS rfc_relations (
  from_rfc INTEGER NOT NULL,
  to_rfc INTEGER NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('obsoletes','obsoleted_by','updates','updated_by')),
  PRIMARY KEY (from_rfc, to_rfc, kind)
);
CREATE INDEX IF NOT EXISTS rfc_relations_to_idx ON rfc_relations(to_rfc, kind);

CREATE TABLE IF NOT EXISTS rfc_bodies (
  rfc INTEGER PRIMARY KEY REFERENCES rfcs(number) ON DELETE CASCADE,
  body_text TEXT,
  body_xml TEXT,
  fetched_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS sub_series (
  doc_id TEXT PRIMARY KEY,
  series TEXT NOT NULL CHECK (series IN ('BCP','STD','FYI')),
  number INTEGER NOT NULL,
  title TEXT
);

CREATE TABLE IF NOT EXISTS sub_series_members (
  doc_id TEXT NOT NULL REFERENCES sub_series(doc_id) ON DELETE CASCADE,
  rfc INTEGER NOT NULL,
  PRIMARY KEY (doc_id, rfc)
);
CREATE INDEX IF NOT EXISTS sub_series_members_rfc_idx ON sub_series_members(rfc);

CREATE TABLE IF NOT EXISTS errata (
  eid INTEGER PRIMARY KEY,
  rfc INTEGER NOT NULL,
  status TEXT NOT NULL,
  kind TEXT NOT NULL,
  section TEXT,
  orig_text TEXT,
  correct_text TEXT,
  notes TEXT,
  submitted TEXT,
  updated TEXT,
  submitter TEXT,
  verifier TEXT
);
CREATE INDEX IF NOT EXISTS errata_rfc_idx ON errata(rfc);
CREATE INDEX IF NOT EXISTS errata_status_idx ON errata(status);

-- v2: full-text search over title/abstract/keywords/body. Rowid = RFC number.
CREATE VIRTUAL TABLE IF NOT EXISTS rfc_search USING fts5(
    title,
    abstract,
    keywords,
    body,
    tokenize = 'porter unicode61'
);

-- v2: directed edges from one RFC body to another, extracted from body text
-- (`[RFCN]` / "RFC N" mentions). Distinct from rfc_relations (which carries
-- the index-declared obsoletes/updates metadata).
CREATE TABLE IF NOT EXISTS rfc_body_refs (
  from_rfc INTEGER NOT NULL,
  to_rfc INTEGER NOT NULL,
  PRIMARY KEY (from_rfc, to_rfc)
);
CREATE INDEX IF NOT EXISTS rfc_body_refs_to_idx ON rfc_body_refs(to_rfc);
"#;
