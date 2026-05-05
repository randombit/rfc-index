/// Metadata for a single RFC, as published in `rfc-index.xml`.
///
/// Returned by [`RfcIndex::get`](crate::RfcIndex::get) and
/// [`RfcIndex::list`](crate::RfcIndex::list). Body text/XML is not held here;
/// fetch it with [`RfcIndex::ensure_body`](crate::RfcIndex::ensure_body).
#[derive(Debug, Clone)]
pub struct Rfc {
    pub(crate) number: u32,
    pub(crate) title: String,
    pub(crate) abstract_text: Option<String>,
    pub(crate) date: Option<Date>,
    pub(crate) page_count: Option<u32>,
    pub(crate) draft: Option<String>,
    pub(crate) current_status: Option<String>,
    pub(crate) publication_status: Option<String>,
    pub(crate) stream: Option<String>,
    pub(crate) area: Option<String>,
    pub(crate) wg: Option<String>,
    pub(crate) doi: Option<String>,
    pub(crate) has_errata: bool,
    pub(crate) formats: Vec<String>,
    pub(crate) has_xml: bool,
    pub(crate) authors: Vec<Author>,
    pub(crate) keywords: Vec<String>,
    pub(crate) obsoletes: Vec<u32>,
    pub(crate) obsoleted_by: Vec<u32>,
    pub(crate) updates: Vec<u32>,
    pub(crate) updated_by: Vec<u32>,
    pub(crate) sub_series: Vec<SubSeriesRef>,
}

impl Rfc {
    /// RFC number (e.g. `9000`).
    pub fn number(&self) -> u32 {
        self.number
    }
    /// Title as published.
    pub fn title(&self) -> &str {
        &self.title
    }
    /// Abstract text (paragraphs joined with blank lines), if published.
    pub fn abstract_text(&self) -> Option<&str> {
        self.abstract_text.as_deref()
    }
    /// Publication date (year, optional month).
    pub fn date(&self) -> Option<Date> {
        self.date
    }
    /// Page count of the rendered text version, if published.
    pub fn page_count(&self) -> Option<u32> {
        self.page_count
    }
    /// Source Internet-Draft name (e.g. `draft-ietf-quic-transport-34`).
    pub fn draft(&self) -> Option<&str> {
        self.draft.as_deref()
    }
    /// Current standardization status (`PROPOSED STANDARD`, `INFORMATIONAL`, …).
    pub fn current_status(&self) -> Option<&str> {
        self.current_status.as_deref()
    }
    /// Status at original publication (often equal to [`current_status`](Self::current_status)).
    pub fn publication_status(&self) -> Option<&str> {
        self.publication_status.as_deref()
    }
    /// IETF stream the document came through (`IETF`, `IRTF`, `IAB`,
    /// `Independent`, `Editorial`, `Legacy`).
    pub fn stream(&self) -> Option<&str> {
        self.stream.as_deref()
    }
    /// IETF area code (e.g. `sec`, `wit`).
    pub fn area(&self) -> Option<&str> {
        self.area.as_deref()
    }
    /// Working-group acronym (e.g. `tls`, `quic`).
    pub fn wg(&self) -> Option<&str> {
        self.wg.as_deref()
    }
    /// DOI string (e.g. `10.17487/RFC9000`).
    pub fn doi(&self) -> Option<&str> {
        self.doi.as_deref()
    }
    /// `true` if rfc-editor.org publishes errata for this RFC. The errata
    /// themselves live in the `errata` table; query with
    /// [`RfcIndex::errata`](crate::RfcIndex::errata).
    pub fn has_errata(&self) -> bool {
        self.has_errata
    }
    /// Available source/render formats (e.g. `["HTML", "TEXT", "PDF", "XML"]`).
    pub fn formats(&self) -> &[String] {
        &self.formats
    }
    /// Convenience: `true` if [`formats`](Self::formats) contains `"XML"`.
    pub fn has_xml(&self) -> bool {
        self.has_xml
    }
    /// Authors (and editors) in publication order.
    pub fn authors(&self) -> &[Author] {
        &self.authors
    }
    /// Editor-assigned keywords/tags.
    pub fn keywords(&self) -> &[String] {
        &self.keywords
    }
    /// RFC numbers this document obsoletes.
    pub fn obsoletes(&self) -> &[u32] {
        &self.obsoletes
    }
    /// RFC numbers that obsolete this document.
    pub fn obsoleted_by(&self) -> &[u32] {
        &self.obsoleted_by
    }
    /// RFC numbers this document updates (without obsoleting).
    pub fn updates(&self) -> &[u32] {
        &self.updates
    }
    /// RFC numbers that update this document.
    pub fn updated_by(&self) -> &[u32] {
        &self.updated_by
    }
    /// BCP/STD/FYI sub-series this RFC is a member of, if any.
    pub fn sub_series(&self) -> &[SubSeriesRef] {
        &self.sub_series
    }
}

/// Publication date for an RFC. Granularity is month — RFCs do not record day
/// of publication.
#[derive(Debug, Clone, Copy)]
pub struct Date {
    /// Four-digit publication year.
    pub year: i32,
    /// Month number 1–12 if known, else `None`.
    pub month: Option<u8>,
}

/// One author or editor of an RFC.
#[derive(Debug, Clone)]
pub struct Author {
    pub(crate) name: String,
    pub(crate) role: Option<String>,
}

impl Author {
    /// Author name as printed (e.g. `J. Iyengar`).
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Role string when present (typically `"Editor"`); `None` for plain authors.
    pub fn role(&self) -> Option<&str> {
        self.role.as_deref()
    }
}

/// One of the three RFC sub-series: BCP, STD, or FYI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesKind {
    /// Best Current Practice.
    Bcp,
    /// Internet Standard.
    Std,
    /// For Your Information (legacy series, no new entries since 1999).
    Fyi,
}

impl SeriesKind {
    /// Three-letter prefix used in canonical doc-ids: `"BCP"`, `"STD"`, `"FYI"`.
    pub fn as_str(self) -> &'static str {
        match self {
            SeriesKind::Bcp => "BCP",
            SeriesKind::Std => "STD",
            SeriesKind::Fyi => "FYI",
        }
    }
}

/// A BCP/STD/FYI sub-series record with its constituent RFC numbers.
#[derive(Debug, Clone)]
pub struct SubSeries {
    pub(crate) doc_id: String,
    pub(crate) series: SeriesKind,
    pub(crate) number: u32,
    pub(crate) title: Option<String>,
    pub(crate) members: Vec<u32>,
}

impl SubSeries {
    /// Canonical, zero-padded doc-id (e.g. `"BCP0014"`).
    pub fn doc_id(&self) -> &str {
        &self.doc_id
    }
    /// Which sub-series (BCP/STD/FYI) this entry belongs to.
    pub fn series(&self) -> SeriesKind {
        self.series
    }
    /// Numeric portion of the doc-id (e.g. `14` for BCP14).
    pub fn number(&self) -> u32 {
        self.number
    }
    /// Optional human-readable sub-series title.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
    /// RFC numbers that make up this sub-series.
    pub fn members(&self) -> &[u32] {
        &self.members
    }
}

/// Lightweight back-reference from an [`Rfc`] to a sub-series it belongs to.
#[derive(Debug, Clone)]
pub struct SubSeriesRef {
    pub(crate) doc_id: String,
    pub(crate) title: Option<String>,
}

impl SubSeriesRef {
    /// Canonical doc-id of the sub-series (e.g. `"BCP0014"`).
    pub fn doc_id(&self) -> &str {
        &self.doc_id
    }
    /// Optional sub-series title.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
}

/// Filter set for [`RfcIndex::list`](crate::RfcIndex::list). Construct with
/// field literals plus `..Default::default()`.
///
/// All filters compose with logical AND. Filters backed by SQL columns
/// (year, status, wg, area, stream, xml, sub-series, obsolescence) are
/// pushed down to the query; regex filters (title, author, abstract) are
/// applied in Rust as a post-filter.
#[derive(Debug, Default, Clone)]
pub struct RfcQuery {
    /// Case-insensitive regex applied to title (post-filtered in Rust).
    pub title_regex: Option<String>,
    /// Inclusive lower bound on publication year.
    pub min_year: Option<i32>,
    /// Inclusive upper bound on publication year.
    pub max_year: Option<i32>,
    /// Case-insensitive substring match against `current_status`.
    pub status_contains: Option<String>,
    /// Restrict to RFCs whose XML source is available.
    pub xml_only: bool,
    /// Case-insensitive exact match against the working-group acronym
    /// (e.g. `"pkix"`, `"tls"`, `"quic"`).
    pub wg: Option<String>,
    /// Case-insensitive exact match against the IETF area code (e.g. `"sec"`).
    pub area: Option<String>,
    /// Case-insensitive exact match against the publication stream (e.g.
    /// `"IETF"`, `"IRTF"`, `"Independent"`).
    pub stream: Option<String>,
    /// Restrict to RFCs that carry this editor-assigned keyword
    /// (case-insensitive exact match against an entry in `rfc_keywords`).
    pub keyword: Option<String>,
    /// Case-insensitive regex applied to any author name (post-filtered).
    pub author_regex: Option<String>,
    /// Case-insensitive regex applied to the abstract text (post-filtered).
    pub abstract_regex: Option<String>,
    /// Restrict to RFCs that are members of any sub-series of this kind
    /// (BCP / STD / FYI).
    pub series: Option<SeriesKind>,
    /// Exclude RFCs that have been obsoleted by another RFC.
    pub not_obsoleted: bool,
    /// Maximum results to return. `None` (or `Some(0)`) means no limit.
    pub limit: Option<usize>,
}

/// One of the discoverable facets exposed by
/// [`RfcIndex::facets`](crate::RfcIndex::facets). Each value of a facet
/// partitions the index along one published metadata axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacetKind {
    /// IETF working-group acronym (e.g. `pkix`, `tls`).
    WorkingGroup,
    /// IETF area code (e.g. `sec`, `art`).
    Area,
    /// Publication stream (`IETF`, `IRTF`, `IAB`, `Independent`, `Editorial`,
    /// `Legacy`).
    Stream,
    /// Editor-assigned keyword (`<kw>` in the index).
    Keyword,
    /// Current standardization status (e.g. `PROPOSED STANDARD`).
    Status,
}

impl FacetKind {
    /// Lower-case slug used by the CLI / MCP surface (`"wg"`, `"area"`,
    /// `"stream"`, `"keyword"`, `"status"`).
    pub fn as_slug(self) -> &'static str {
        match self {
            FacetKind::WorkingGroup => "wg",
            FacetKind::Area => "area",
            FacetKind::Stream => "stream",
            FacetKind::Keyword => "keyword",
            FacetKind::Status => "status",
        }
    }

    /// Parse a facet slug. Accepts `wg` / `working-group`, `area`, `stream`,
    /// `keyword` / `kw`, `status`. Returns `None` for unrecognised input.
    pub fn parse(s: &str) -> Option<FacetKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "wg" | "working-group" | "working_group" | "workinggroup" => {
                Some(FacetKind::WorkingGroup)
            }
            "area" => Some(FacetKind::Area),
            "stream" => Some(FacetKind::Stream),
            "keyword" | "kw" | "keywords" => Some(FacetKind::Keyword),
            "status" => Some(FacetKind::Status),
            _ => None,
        }
    }
}

/// One bucket from [`RfcIndex::facets`](crate::RfcIndex::facets): a distinct
/// value of some facet plus the count of RFCs carrying it. RFCs without a
/// value for the facet (e.g. no working group) are omitted; counts ignore the
/// `null` bucket.
#[derive(Debug, Clone)]
pub struct Facet {
    pub(crate) value: String,
    pub(crate) count: u32,
}

impl Facet {
    /// The facet value as published (e.g. `"pkix"`, `"sec"`, `"IETF"`).
    pub fn value(&self) -> &str {
        &self.value
    }
    /// Number of RFCs that carry this facet value.
    pub fn count(&self) -> u32 {
        self.count
    }
}

/// Aggregate counts of records currently in the local database.
#[derive(Debug, Clone, Copy)]
pub struct Counts {
    /// Total RFCs in the metadata index.
    pub rfcs: u32,
    /// Total BCP entries.
    pub bcps: u32,
    /// Total STD entries.
    pub stds: u32,
    /// Total FYI entries.
    pub fyis: u32,
    /// Number of RFC bodies currently cached locally.
    pub bodies_cached: u32,
}

/// One ranked search hit returned by [`RfcIndex::search`](crate::RfcIndex::search).
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub(crate) number: u32,
    pub(crate) title: String,
    pub(crate) snippet: String,
    pub(crate) score: f64,
}

impl SearchHit {
    /// RFC number this hit refers to.
    pub fn number(&self) -> u32 {
        self.number
    }
    /// RFC title (looked up from the metadata table).
    pub fn title(&self) -> &str {
        &self.title
    }
    /// FTS5 snippet around the matched terms, with `<<` / `>>` highlight markers.
    pub fn snippet(&self) -> &str {
        &self.snippet
    }
    /// BM25 rank from FTS5. Lower (more negative) means a better match.
    pub fn score(&self) -> f64 {
        self.score
    }
}

/// Result of [`RfcIndex::sync_index`](crate::RfcIndex::sync_index).
#[derive(Debug, Clone, Copy, Default)]
pub struct SyncStats {
    /// Number of RFC entries ingested or updated.
    pub rfcs: usize,
    /// Number of BCP entries ingested or updated.
    pub bcps: usize,
    /// Number of STD entries ingested or updated.
    pub stds: usize,
    /// Number of FYI entries ingested or updated.
    pub fyis: usize,
    /// Bytes downloaded from the index URL (zero on `not_modified`).
    pub bytes: usize,
    /// `true` if validators matched (HEAD probe or 304) and ingest was skipped.
    pub not_modified: bool,
}

/// Result of [`RfcIndex::sync_errata`](crate::RfcIndex::sync_errata).
#[derive(Debug, Clone, Copy, Default)]
pub struct ErrataSyncStats {
    /// Number of errata records ingested.
    pub errata: usize,
    /// Bytes downloaded from the errata URL (zero on `not_modified`).
    pub bytes: usize,
    /// `true` if validators matched and ingest was skipped.
    pub not_modified: bool,
}

/// One published erratum for an RFC.
#[derive(Debug, Clone)]
pub struct Erratum {
    pub(crate) eid: u32,
    pub(crate) rfc: u32,
    pub(crate) status: String,
    pub(crate) kind: String,
    pub(crate) section: Option<String>,
    pub(crate) orig_text: Option<String>,
    pub(crate) correct_text: Option<String>,
    pub(crate) notes: Option<String>,
    pub(crate) submitted: Option<String>,
    pub(crate) updated: Option<String>,
    pub(crate) submitter: Option<String>,
    pub(crate) verifier: Option<String>,
}

impl Erratum {
    /// Global errata ID (the `EID` shown on rfc-editor.org).
    pub fn eid(&self) -> u32 {
        self.eid
    }
    /// RFC number this erratum applies to.
    pub fn rfc(&self) -> u32 {
        self.rfc
    }
    /// One of `"Verified"`, `"Held for Document Update"`, `"Reported"`, `"Rejected"`.
    pub fn status(&self) -> &str {
        &self.status
    }
    /// `"Technical"` or `"Editorial"`.
    pub fn kind(&self) -> &str {
        &self.kind
    }
    /// Section of the RFC the erratum addresses, if recorded.
    pub fn section(&self) -> Option<&str> {
        self.section.as_deref()
    }
    /// Original (incorrect) text from the RFC, as quoted in the erratum.
    pub fn orig_text(&self) -> Option<&str> {
        self.orig_text.as_deref()
    }
    /// Proposed corrected text.
    pub fn correct_text(&self) -> Option<&str> {
        self.correct_text.as_deref()
    }
    /// Free-form notes from the submitter or verifier.
    pub fn notes(&self) -> Option<&str> {
        self.notes.as_deref()
    }
    /// Submission date as published (`YYYY-MM-DD`).
    pub fn submitted(&self) -> Option<&str> {
        self.submitted.as_deref()
    }
    /// Last-update timestamp as published.
    pub fn updated(&self) -> Option<&str> {
        self.updated.as_deref()
    }
    /// Name of the person who submitted the erratum.
    pub fn submitter(&self) -> Option<&str> {
        self.submitter.as_deref()
    }
    /// Name of the person who verified the erratum (if any).
    pub fn verifier(&self) -> Option<&str> {
        self.verifier.as_deref()
    }
    /// Convenience: status is exactly `"Verified"` (case-insensitive).
    pub fn is_verified(&self) -> bool {
        self.status.eq_ignore_ascii_case("Verified")
    }
    /// Convenience: status is exactly `"Held for Document Update"`.
    pub fn is_held(&self) -> bool {
        self.status.eq_ignore_ascii_case("Held for Document Update")
    }
}

/// Cached body of a single RFC. Contains the rendered text and (when the RFC
/// has an XML source) the raw XML.
#[derive(Debug, Clone)]
pub struct Body {
    pub(crate) number: u32,
    pub(crate) text: String,
    pub(crate) xml: Option<String>,
    pub(crate) fetched_at: i64,
}

impl Body {
    /// RFC number this body belongs to.
    pub fn number(&self) -> u32 {
        self.number
    }
    /// Rendered text body (line endings normalized to LF, form-feeds stripped).
    pub fn text(&self) -> &str {
        &self.text
    }
    /// Raw xml2rfc v3 source, when available (modern RFCs only).
    pub fn xml(&self) -> Option<&str> {
        self.xml.as_deref()
    }
    /// Epoch seconds when the body was fetched.
    pub fn fetched_at(&self) -> i64 {
        self.fetched_at
    }

    /// All numeric top-level and nested sections detected in the rendered text,
    /// in document order.
    pub fn sections(&self) -> Vec<SectionRef<'_>> {
        crate::body::scan_sections(&self.text)
    }

    /// Look up a single section by its dotted number (e.g. `"5.2"`). The
    /// returned slice covers the section header line through (but not
    /// including) the next section that is not a descendant.
    pub fn section(&self, number: &str) -> Option<SectionRef<'_>> {
        crate::body::find_section(&self.text, number)
    }
}

/// View into one section of a [`Body`]. All fields are slices borrowed from
/// the body's text — no allocation.
#[derive(Debug, Clone, Copy)]
pub struct SectionRef<'a> {
    /// Dotted section number (e.g. `"5.2"`, `"5.2.1"`).
    pub number: &'a str,
    /// Section title as it appears in the header line.
    pub title: &'a str,
    /// Slice of the body covering this section (header line through last line
    /// of the deepest descendant before the next non-descendant header).
    pub text: &'a str,
}
