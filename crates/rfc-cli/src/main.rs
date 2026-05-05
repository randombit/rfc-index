use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser, Subcommand};
use rfc_index::{Erratum, FacetKind, Rfc, RfcIndex, RfcQuery, SeriesKind, SubSeries};

#[derive(Parser)]
#[command(name = "rfc", about = "Local RFC index and cache")]
struct Cli {
    /// Database path (overrides $RFC_INDEX_DB and the default location).
    #[arg(long, global = true, env = "RFC_INDEX_DB")]
    db: Option<std::path::PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

/// Filter set shared by `rfc index list` and `rfc fetch`. Maps directly to
/// `RfcQuery`. All filters compose with logical AND; regex filters are
/// case-insensitive.
#[derive(Args, Default, Debug, Clone)]
struct ListFilters {
    /// Case-insensitive regex applied to the RFC title.
    #[arg(long, value_name = "PATTERN")]
    title_regex: Option<String>,
    /// Inclusive lower bound on publication year.
    #[arg(long, value_name = "YEAR")]
    since: Option<i32>,
    /// Inclusive upper bound on publication year.
    #[arg(long, value_name = "YEAR")]
    until: Option<i32>,
    /// Case-insensitive substring match against `current_status`.
    #[arg(long, value_name = "STATUS")]
    status: Option<String>,
    /// Restrict to RFCs whose XML source is available.
    #[arg(long)]
    xml_only: bool,
    /// Working-group acronym (e.g. `pkix`, `tls`). Case-insensitive exact match.
    #[arg(long, value_name = "WG")]
    wg: Option<String>,
    /// IETF area code (e.g. `sec`, `art`). Case-insensitive exact match.
    #[arg(long, value_name = "AREA")]
    area: Option<String>,
    /// Publication stream (`IETF`, `IRTF`, `IAB`, `Independent`, `Editorial`,
    /// `Legacy`). Case-insensitive exact match.
    #[arg(long, value_name = "STREAM")]
    stream: Option<String>,
    /// Editor-assigned keyword. Case-insensitive exact match against an entry
    /// in the `<kw>` list.
    #[arg(long, value_name = "KEYWORD")]
    keyword: Option<String>,
    /// Case-insensitive regex applied to any author name.
    #[arg(long, value_name = "PATTERN")]
    author_regex: Option<String>,
    /// Case-insensitive regex applied to the abstract text.
    #[arg(long, value_name = "PATTERN")]
    abstract_regex: Option<String>,
    /// Restrict to RFCs that are members of any sub-series of this kind
    /// (`bcp`, `std`, `fyi`).
    #[arg(long, value_name = "KIND")]
    series: Option<String>,
    /// Exclude RFCs that have been obsoleted by another RFC.
    #[arg(long)]
    not_obsoleted: bool,
}

impl ListFilters {
    fn into_query(self, limit: Option<usize>) -> Result<RfcQuery> {
        let series = match self.series.as_deref() {
            None => None,
            Some(s) => Some(parse_series_kind(s)?),
        };
        Ok(RfcQuery {
            title_regex: self.title_regex,
            min_year: self.since,
            max_year: self.until,
            status_contains: self.status,
            xml_only: self.xml_only,
            wg: self.wg,
            area: self.area,
            stream: self.stream,
            keyword: self.keyword,
            author_regex: self.author_regex,
            abstract_regex: self.abstract_regex,
            series,
            not_obsoleted: self.not_obsoleted,
            limit,
        })
    }
}

fn parse_series_kind(s: &str) -> Result<SeriesKind> {
    match s.trim().to_ascii_uppercase().as_str() {
        "BCP" => Ok(SeriesKind::Bcp),
        "STD" => Ok(SeriesKind::Std),
        "FYI" => Ok(SeriesKind::Fyi),
        other => Err(anyhow!(
            "unknown series kind {other:?}; expected bcp/std/fyi"
        )),
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Manage the RFC metadata index (rfc-index.xml)
    Index {
        #[command(subcommand)]
        sub: IndexCmd,
    },
    /// List distinct values of a discoverable facet (working group, area,
    /// stream, keyword, status), with the count of RFCs carrying each value.
    /// Use this to learn what filter values exist before drilling in via
    /// `rfc index list`.
    Facets {
        /// Facet to enumerate: `wg`, `area`, `stream`, `keyword`, `status`.
        kind: String,
        /// Case-insensitive substring filter on the facet value.
        #[arg(long, value_name = "PATTERN")]
        contains: Option<String>,
        /// Maximum facet values to print. Default: 50. Pass 0 for no limit.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Show metadata for an RFC or sub-series (BCP/STD/FYI)
    Info {
        /// RFC number, "RFC9000", "BCP14", "STD3", "FYI4" (case-insensitive)
        id: String,
    },
    /// Print the body (or a section) of an RFC, fetching it if necessary
    Get {
        number: u32,
        /// Section number (e.g. 5.2). Prints just that section.
        #[arg(long)]
        section: Option<String>,
        /// List section numbers + titles instead of printing body
        #[arg(long, conflicts_with = "section", conflicts_with = "xml")]
        sections: bool,
        /// Print the cached XML body instead of the rendered text
        #[arg(long, conflicts_with = "section")]
        xml: bool,
        /// Fail if the body isn't cached, instead of fetching it
        #[arg(long)]
        no_fetch: bool,
    },
    /// Full-text search over title/abstract/keywords/body (FTS5)
    Search {
        /// FTS5 query: phrases with quotes, AND/OR/NOT, NEAR(), column filters
        /// like `title:QUIC`, etc.
        query: String,
        /// Maximum results. 0 means no limit. Default: 20.
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Show BM25 scores
        #[arg(long)]
        scores: bool,
    },
    /// Show body-derived references for an RFC
    Refs {
        number: u32,
        /// Show RFCs whose cached body references this RFC, instead of the
        /// references this RFC's body makes
        #[arg(long)]
        to: bool,
    },
    /// Manage and view errata
    Errata {
        #[command(subcommand)]
        sub: ErrataCmd,
    },
    /// Bulk-fetch bodies for RFCs matching the given filters
    Fetch {
        #[command(flatten)]
        filters: ListFilters,
        /// Maximum RFCs to process (default: all matches)
        #[arg(long)]
        limit: Option<usize>,
        /// Re-fetch even if body is already cached
        #[arg(long)]
        overwrite: bool,
        /// Stop on the first fetch failure
        #[arg(long)]
        stop_on_error: bool,
    },
}

#[derive(Subcommand)]
enum ErrataCmd {
    /// Bulk-download errata.json and replace the local cache
    Sync,
    /// List errata for one RFC
    Show {
        rfc: u32,
        /// Only show errata in this status (case-insensitive substring of e.g.
        /// "verified", "held", "reported", "rejected")
        #[arg(long, value_name = "STATUS")]
        status: Option<String>,
        /// Show full original/correct text (otherwise truncated to 200 chars)
        #[arg(long)]
        full: bool,
    },
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum IndexCmd {
    /// Download rfc-index.xml and ingest metadata
    Sync,
    /// Show counts and last sync time
    Status,
    /// List RFCs matching filters
    List {
        #[command(flatten)]
        filters: ListFilters,
        /// Maximum results. Omit (or pass 0) for no limit.
        #[arg(long)]
        limit: Option<usize>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut index = match cli.db {
        Some(p) => RfcIndex::open(&p),
        None => RfcIndex::open_default(),
    }
    .context("opening RFC index database")?;

    match cli.cmd {
        Cmd::Index { sub } => match sub {
            IndexCmd::Sync => cmd_sync(&mut index),
            IndexCmd::Status => cmd_status(&index),
            IndexCmd::List { filters, limit } => cmd_list(&index, filters, limit),
        },
        Cmd::Facets {
            kind,
            contains,
            limit,
        } => cmd_facets(&index, &kind, contains.as_deref(), limit),
        Cmd::Info { id } => cmd_info(&index, &id),
        Cmd::Get {
            number,
            section,
            sections,
            xml,
            no_fetch,
        } => cmd_get(&mut index, number, section, sections, xml, no_fetch),
        Cmd::Search {
            query,
            limit,
            scores,
        } => cmd_search(&index, &query, limit, scores),
        Cmd::Errata { sub } => match sub {
            ErrataCmd::Sync => cmd_errata_sync(&mut index),
            ErrataCmd::Show { rfc, status, full } => cmd_errata_show(&index, rfc, status, full),
        },
        Cmd::Refs { number, to } => cmd_refs(&index, number, to),
        Cmd::Fetch {
            filters,
            limit,
            overwrite,
            stop_on_error,
        } => cmd_fetch(&mut index, filters, limit, overwrite, stop_on_error),
    }
}

fn cmd_sync(index: &mut RfcIndex) -> Result<()> {
    eprintln!("Fetching rfc-index.xml ...");
    let stats = index.sync_index().context("syncing index")?;
    if stats.not_modified {
        println!("Index unchanged (validators match)");
    } else {
        println!(
            "Ingested {} RFCs, {} BCPs, {} STDs, {} FYIs ({} bytes downloaded)",
            stats.rfcs, stats.bcps, stats.stds, stats.fyis, stats.bytes
        );
    }
    Ok(())
}

fn cmd_status(index: &RfcIndex) -> Result<()> {
    let c = index.counts()?;
    let synced = index.last_synced_at()?;
    println!("DB:            {}", index.path().display());
    println!("RFCs:          {}", c.rfcs);
    println!("BCP/STD/FYI:   {} / {} / {}", c.bcps, c.stds, c.fyis);
    println!("Bodies cached: {}", c.bodies_cached);
    match synced {
        Some(ts) => println!("Last sync:     {}", format_epoch_utc(ts)),
        None => println!("Last sync:     never"),
    }
    Ok(())
}

/// Format an epoch-seconds value as "YYYY-MM-DD HH:MM UTC". Uses Howard
/// Hinnant's civil-from-days algorithm; correct from year 1 onward, no deps.
fn format_epoch_utc(epoch: i64) -> String {
    let days = epoch.div_euclid(86400);
    let secs = epoch.rem_euclid(86400);
    let (year, month, day) = civil_from_days(days);
    let hour = secs / 3600;
    let minute = (secs / 60) % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    (y, m as u32, d as u32)
}

fn cmd_list(index: &RfcIndex, filters: ListFilters, limit: Option<usize>) -> Result<()> {
    let q = filters.into_query(limit)?;
    let rfcs = index.list(&q)?;
    if rfcs.is_empty() {
        eprintln!("(no matches)");
        return Ok(());
    }
    for r in rfcs {
        let year = r
            .date()
            .map(|d| d.year.to_string())
            .unwrap_or_else(|| "----".into());
        let status_s = r.current_status().unwrap_or("");
        let xml_mark = if r.has_xml() { "x" } else { "-" };
        println!(
            "RFC{:<5} {} [{} {}] {}",
            r.number(),
            xml_mark,
            year,
            status_s,
            r.title()
        );
    }
    Ok(())
}

fn cmd_facets(index: &RfcIndex, kind: &str, contains: Option<&str>, limit: usize) -> Result<()> {
    let parsed = FacetKind::parse(kind)
        .ok_or_else(|| anyhow!("unknown facet {kind:?}; expected wg/area/stream/keyword/status"))?;
    let facets = index.facets(parsed, contains)?;
    if facets.is_empty() {
        eprintln!("(no facet values)");
        return Ok(());
    }
    let max_value_len = facets.iter().map(|f| f.value().len()).max().unwrap_or(0);
    let take = if limit == 0 { facets.len() } else { limit };
    let total = facets.len();
    for f in facets.into_iter().take(take) {
        println!(
            "{:<width$}  {}",
            f.value(),
            f.count(),
            width = max_value_len
        );
    }
    if total > take {
        eprintln!("({} more — pass --limit 0 to show all)", total - take);
    }
    Ok(())
}

fn cmd_info(index: &RfcIndex, id: &str) -> Result<()> {
    let trimmed = id.trim();

    // Sub-series alias: "BCP14", "std-3", "FYI 4", etc.
    let upper = trimmed.to_ascii_uppercase();
    if upper.starts_with("BCP") || upper.starts_with("STD") || upper.starts_with("FYI") {
        let sub = index
            .get_sub_series(trimmed)?
            .ok_or_else(|| anyhow!("{trimmed} not found — try `rfc index sync`"))?;
        return print_sub_series(index, &sub);
    }

    // Otherwise treat as an RFC number, optionally with a leading "RFC".
    let digits = upper.strip_prefix("RFC").unwrap_or(&upper).trim();
    let n: u32 = digits
        .parse()
        .map_err(|_| anyhow!("'{id}' is not an RFC number or sub-series id"))?;
    let r = index
        .get(n)?
        .ok_or_else(|| anyhow!("RFC {n} not in index — try `rfc index sync`"))?;
    print_rfc(&r);
    Ok(())
}

fn print_sub_series(index: &RfcIndex, sub: &SubSeries) -> Result<()> {
    let header = format!("{} {}", sub.series().as_str(), sub.number());
    match sub.title() {
        Some(t) => println!("{header} — {t}"),
        None => println!("{header}"),
    }
    if sub.members().is_empty() {
        println!("(no member RFCs)");
        return Ok(());
    }
    println!("Members:");
    for n in sub.members() {
        match index.get(*n)? {
            Some(r) => println!("  RFC{:<5}  {}", n, r.title()),
            None => println!("  RFC{n:<5}  (not in index)"),
        }
    }
    Ok(())
}

fn cmd_errata_sync(index: &mut RfcIndex) -> Result<()> {
    eprintln!("Fetching errata.json ...");
    let stats = index.sync_errata().context("syncing errata")?;
    if stats.not_modified {
        println!("Errata unchanged (validators match)");
    } else {
        println!(
            "Ingested {} errata ({} bytes downloaded)",
            stats.errata, stats.bytes
        );
    }
    Ok(())
}

fn cmd_errata_show(
    index: &RfcIndex,
    rfc: u32,
    status_filter: Option<String>,
    full: bool,
) -> Result<()> {
    let last = index.last_errata_synced_at()?;
    if last.is_none() {
        eprintln!("(errata cache empty — run `rfc errata sync` first)");
    }
    let mut errata = index.errata(rfc)?;
    if let Some(status) = &status_filter {
        let needle = status.to_lowercase();
        errata.retain(|e| e.status().to_lowercase().contains(&needle));
    }
    if errata.is_empty() {
        eprintln!("(no matching errata for RFC {rfc})");
        return Ok(());
    }
    for e in errata {
        print_erratum(&e, full);
        println!();
    }
    Ok(())
}

fn print_erratum(e: &Erratum, full: bool) {
    let section = e.section().unwrap_or("-");
    println!(
        "EID {}  RFC{}  [{} / {}]  §{}",
        e.eid(),
        e.rfc(),
        e.status(),
        e.kind(),
        section,
    );
    if let Some(s) = e.submitter() {
        let date = e.submitted().unwrap_or("");
        println!("  submitted: {s} ({date})");
    }
    if let Some(v) = e.verifier() {
        println!("  verifier:  {v}");
    }
    if let Some(orig) = e.orig_text() {
        println!("  original:");
        print_block(orig, full);
    }
    if let Some(corr) = e.correct_text() {
        println!("  corrected:");
        print_block(corr, full);
    }
    if let Some(notes) = e.notes() {
        if !notes.trim().is_empty() {
            println!("  notes:");
            print_block(notes, full);
        }
    }
}

fn print_block(text: &str, full: bool) {
    let limit = if full { usize::MAX } else { 200 };
    let mut shown = 0usize;
    for line in text.lines() {
        if !full && shown + line.len() > limit {
            let remaining = limit.saturating_sub(shown);
            let cut: String = line.chars().take(remaining).collect();
            println!("    {cut}…");
            println!("    [truncated; --full to show]");
            return;
        }
        println!("    {line}");
        shown += line.len() + 1;
    }
}

fn cmd_get(
    index: &mut RfcIndex,
    n: u32,
    section: Option<String>,
    list_sections: bool,
    want_xml: bool,
    no_fetch: bool,
) -> Result<()> {
    let body = if no_fetch {
        index.body(n)?.ok_or_else(|| {
            anyhow!("RFC {n} body not cached (use `rfc fetch` or drop --no-fetch)")
        })?
    } else {
        index.ensure_body(n)?
    };

    if list_sections {
        for s in body.sections() {
            println!("{}\t{}", s.number, s.title);
        }
        return Ok(());
    }

    if want_xml {
        match body.xml() {
            Some(x) => {
                print!("{x}");
                Ok(())
            }
            None => Err(anyhow!("RFC {n} has no XML source")),
        }
    } else if let Some(num) = section {
        match body.section(&num) {
            Some(s) => {
                print!("{}", s.text);
                Ok(())
            }
            None => Err(anyhow!(
                "section {num} not found in RFC {n} (try `rfc get {n} --sections`)"
            )),
        }
    } else {
        print!("{}", body.text());
        Ok(())
    }
}

fn cmd_search(index: &RfcIndex, query: &str, limit: usize, show_scores: bool) -> Result<()> {
    let lim = if limit == 0 { None } else { Some(limit) };
    let hits = index.search(query, lim)?;
    if hits.is_empty() {
        eprintln!("(no matches)");
        return Ok(());
    }
    for h in hits {
        if show_scores {
            println!("RFC{:<5} [{:.2}] {}", h.number(), h.score(), h.title());
        } else {
            println!("RFC{:<5} {}", h.number(), h.title());
        }
        for line in h.snippet().lines() {
            println!("    {line}");
        }
    }
    Ok(())
}

fn cmd_refs(index: &RfcIndex, n: u32, incoming: bool) -> Result<()> {
    let nums = if incoming {
        index.referenced_by(n)?
    } else {
        index.references(n)?
    };
    if nums.is_empty() {
        let dir = if incoming { "incoming" } else { "outgoing" };
        eprintln!("(no {dir} body refs for RFC {n} — body may not be cached; try `rfc get {n}`)");
        return Ok(());
    }
    for r in nums {
        let title: String = index
            .get(r)?
            .map(|x| x.title().to_string())
            .unwrap_or_else(|| "(not in index)".into());
        println!("RFC{r:<5} {title}");
    }
    Ok(())
}

fn cmd_fetch(
    index: &mut RfcIndex,
    filters: ListFilters,
    limit: Option<usize>,
    overwrite: bool,
    stop_on_error: bool,
) -> Result<()> {
    let q = filters.into_query(limit)?;
    let rfcs = index.list(&q)?;
    if rfcs.is_empty() {
        eprintln!("(no matches)");
        return Ok(());
    }

    let mut fetched = 0usize;
    let mut skipped = 0usize;
    let mut failed: Vec<(u32, String)> = Vec::new();

    eprintln!("Pre-seeding {} RFCs ...", rfcs.len());
    for r in &rfcs {
        let n = r.number();
        if !overwrite && index.has_body(n)? {
            skipped += 1;
            continue;
        }
        eprint!("RFC{n} ... ");
        match index.fetch_body(n) {
            Ok(b) => {
                let xml_mark = if b.xml().is_some() { " (+xml)" } else { "" };
                eprintln!("ok{}  [{} bytes]", xml_mark, b.text().len());
                fetched += 1;
            }
            Err(e) => {
                eprintln!("FAIL: {e}");
                failed.push((n, e.to_string()));
                if stop_on_error {
                    break;
                }
            }
        }
    }

    println!(
        "Fetched: {}  Skipped: {}  Failed: {}",
        fetched,
        skipped,
        failed.len()
    );
    if !failed.is_empty() {
        for (n, msg) in &failed {
            println!("  RFC{n}: {msg}");
        }
        std::process::exit(2);
    }
    Ok(())
}

fn print_rfc(r: &Rfc) {
    println!("RFC {} — {}", r.number(), r.title());

    let date = match r.date() {
        Some(d) => match d.month {
            Some(m) => format!("{:04}-{:02}", d.year, m),
            None => format!("{:04}", d.year),
        },
        None => "unknown".into(),
    };
    let pages = r
        .page_count()
        .map(|p| format!("  ({p} pages)"))
        .unwrap_or_default();
    println!("Date:    {date}{pages}");

    if let Some(s) = r.current_status() {
        println!("Status:  {s}");
    }
    if let Some(s) = r.publication_status() {
        if Some(s) != r.current_status() {
            println!("PubStatus: {s}");
        }
    }
    if let Some(s) = r.stream() {
        print!("Stream:  {s}");
        if let Some(a) = r.area() {
            print!("  area={a}");
        }
        if let Some(w) = r.wg() {
            print!("  wg={w}");
        }
        println!();
    }
    if let Some(d) = r.draft() {
        println!("Draft:   {d}");
    }
    if let Some(d) = r.doi() {
        println!("DOI:     {d}");
    }
    let xml_note = if r.has_xml() { " (XML available)" } else { "" };
    println!("Formats: {}{}", r.formats().join(","), xml_note);

    if !r.authors().is_empty() {
        let line = r
            .authors()
            .iter()
            .map(|a| match a.role() {
                Some(role) => format!("{} ({})", a.name(), role),
                None => a.name().to_string(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!("Authors: {line}");
    }

    print_relation("Obsoletes", r.obsoletes());
    print_relation("Obsoleted by", r.obsoleted_by());
    print_relation("Updates", r.updates());
    print_relation("Updated by", r.updated_by());

    for sub in r.sub_series() {
        match sub.title() {
            Some(t) => println!("Part of: {} — {}", sub.doc_id(), t),
            None => println!("Part of: {}", sub.doc_id()),
        }
    }

    if r.has_errata() {
        println!(
            "Errata:  yes  (https://www.rfc-editor.org/errata/rfc{})",
            r.number()
        );
    }

    if let Some(abs) = r.abstract_text() {
        println!();
        println!("Abstract:");
        for line in abs.lines() {
            println!("  {line}");
        }
    }
}

fn print_relation(label: &str, nums: &[u32]) {
    if nums.is_empty() {
        return;
    }
    let s = nums
        .iter()
        .map(|n| format!("RFC{n}"))
        .collect::<Vec<_>>()
        .join(", ");
    println!("{label}: {s}");
}
