use crate::error::{Error, Result};
use crate::model::{Body, SectionRef};
use reqwest::StatusCode;
use reqwest::blocking::Client;

pub(crate) fn fetch(client: &Client, number: u32, has_xml: bool, base_url: &str) -> Result<Body> {
    let text_url = format!("{base_url}/rfc/rfc{number}.txt");
    let text = fetch_string(client, &text_url)?
        .ok_or_else(|| Error::NotFound(format!("RFC {number} text body (404)")))?;

    let xml = if has_xml {
        let xml_url = format!("{base_url}/rfc/rfc{number}.xml");
        fetch_string(client, &xml_url)?
    } else {
        None
    };

    Ok(Body {
        number,
        text: normalize_text(&text),
        xml,
        fetched_at: crate::db::now_epoch(),
    })
}

fn fetch_string(client: &Client, url: &str) -> Result<Option<String>> {
    let resp = client.get(url).send()?;
    if resp.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let resp = resp.error_for_status()?;
    Ok(Some(resp.text()?))
}

/// Normalize line endings and strip form-feed page-break characters.
fn normalize_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\r' => {}       // collapse CRLF to LF
            '\u{000C}' => {} // form-feed page breaks
            c => out.push(c),
        }
    }
    out
}

/// Walk lines of `text`, emitting one SectionRef per detected numeric section header.
pub(crate) fn scan_sections(text: &str) -> Vec<SectionRef<'_>> {
    let starts = section_starts(text);
    let mut out = Vec::with_capacity(starts.len());
    for i in 0..starts.len() {
        let (start_byte, num_range, title_range) = starts[i];
        let end_byte = starts.get(i + 1).map(|s| s.0).unwrap_or(text.len());
        out.push(SectionRef {
            number: &text[num_range.0..num_range.1],
            title: &text[title_range.0..title_range.1],
            text: &text[start_byte..end_byte],
        });
    }
    out
}

pub(crate) fn find_section<'a>(text: &'a str, target: &str) -> Option<SectionRef<'a>> {
    let starts = section_starts(text);
    let mut found_idx: Option<usize> = None;
    for (i, (_, num_range, _)) in starts.iter().enumerate() {
        if &text[num_range.0..num_range.1] == target {
            found_idx = Some(i);
            break;
        }
    }
    let idx = found_idx?;
    let (start_byte, num_range, title_range) = starts[idx];

    // End boundary: first subsequent section whose number is not a descendant of target.
    let prefix = format!("{target}.");
    let mut end_byte = text.len();
    for next in &starts[idx + 1..] {
        let n = &text[next.1.0..next.1.1];
        if !n.starts_with(&prefix) {
            end_byte = next.0;
            break;
        }
    }

    Some(SectionRef {
        number: &text[num_range.0..num_range.1],
        title: &text[title_range.0..title_range.1],
        text: &text[start_byte..end_byte],
    })
}

/// `(line_start_byte, num_range, title_range)` for one detected section header.
type SectionStart = (usize, (usize, usize), (usize, usize));

/// Returns one `SectionStart` per detected header. A section header is a line
/// at column 0 of the form `N(.N)*\.  Title` (two or more spaces between
/// number and title).
fn section_starts(text: &str) -> Vec<SectionStart> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let line_start = i;
        let line_end = match memchr_lf(bytes, i) {
            Some(p) => p,
            None => bytes.len(),
        };

        if let Some((num, title)) = parse_header_line(&text[line_start..line_end]) {
            let num_start = line_start + num.0;
            let num_end = line_start + num.1;
            let title_start = line_start + title.0;
            let title_end = line_start + title.1;
            out.push((line_start, (num_start, num_end), (title_start, title_end)));
        }

        i = if line_end < bytes.len() {
            line_end + 1
        } else {
            line_end
        };
    }
    out
}

fn memchr_lf(bytes: &[u8], from: usize) -> Option<usize> {
    bytes[from..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|p| p + from)
}

/// Parse one line. Returns `Some((num_range, title_range))` (relative byte offsets
/// within `line`) iff it matches a numeric section header, else `None`.
///
/// Accepted shapes, to cover both modern xml2rfc and older RFC conventions:
///   `N(.N)*[.]  Title` at column 0           — modern (e.g., RFC 9000)
///   `N(.N)*[.] Title`  at column 0           — older single-space (e.g., RFC 1034)
///   `   N.N+[.]  Title` indented up to 3 sp  — older indented sub-sections (e.g., RFC 1123)
fn parse_header_line(line: &str) -> Option<((usize, usize), (usize, usize))> {
    let bytes = line.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    // Optional leading whitespace (only meaningful for nested sub-sections; see below).
    let mut i = 0usize;
    while i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }
    let leading = i;
    // Cap indent to avoid sweeping in deeply-nested list items as headers.
    if leading > 3 {
        return None;
    }

    let num_start = i;
    if i >= bytes.len() || !bytes[i].is_ascii_digit() {
        return None;
    }

    // Walk N(.N)* prefix.
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let mut has_dot_separator = false;
    while i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
        has_dot_separator = true;
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    let num_end = i;
    if num_end == num_start {
        return None;
    }

    // Indented lines must be sub-section headers (have at least one '.N' separator);
    // otherwise we'd pick up plain numbered list items like "   1. foo".
    if leading > 0 && !has_dot_separator {
        return None;
    }

    // Optional terminating '.' after the number — present in modern xml2rfc and
    // most older styles, but RFC 1123 sub-section headers ("2.1  Title") omit it.
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
    }

    // Require at least one space before the title.
    let space_start = i;
    while i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }
    if i - space_start < 1 {
        return None;
    }

    if i >= bytes.len() {
        return None;
    }

    let title_start = i;
    let mut title_end = bytes.len();
    while title_end > title_start && bytes[title_end - 1] == b' ' {
        title_end -= 1;
    }
    if title_end <= title_start {
        return None;
    }

    // Reject table-of-contents entries: real section titles never contain a
    // run of 4+ consecutive dots (the dotleader between title and page number).
    let title_bytes = &bytes[title_start..title_end];
    let mut dot_run = 0usize;
    for &b in title_bytes {
        if b == b'.' {
            dot_run += 1;
            if dot_run >= 4 {
                return None;
            }
        } else {
            dot_run = 0;
        }
    }

    Some(((num_start, num_end), (title_start, title_end)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_top_level_and_nested_sections() {
        let text = "\
1.  Introduction

   intro text

2.  Background

   bg text

5.2.  Frame Types

   frames

5.2.1.  ACK Frame

   ack

6.  References
";
        let sections: Vec<_> = scan_sections(text)
            .into_iter()
            .map(|s| (s.number.to_string(), s.title.to_string()))
            .collect();
        assert_eq!(
            sections,
            vec![
                ("1".into(), "Introduction".into()),
                ("2".into(), "Background".into()),
                ("5.2".into(), "Frame Types".into()),
                ("5.2.1".into(), "ACK Frame".into()),
                ("6".into(), "References".into()),
            ]
        );
    }

    #[test]
    fn section_lookup_includes_descendants() {
        let text = "\
5.  Foo

   foo

5.2.  Bar

   bar

5.2.1.  Baz

   baz

6.  Qux

   qux
";
        let s = find_section(text, "5.2").unwrap();
        assert_eq!(s.number, "5.2");
        assert!(s.text.contains("5.2.1.  Baz"));
        assert!(!s.text.contains("6.  Qux"));
    }

    #[test]
    fn ignores_indented_lookalikes() {
        // A line that looks like a header but is indented isn't one.
        let text = "\
1.  Real

   1.  This is content not a header

2.  Other
";
        let starts = scan_sections(text);
        let nums: Vec<_> = starts.iter().map(|s| s.number).collect();
        assert_eq!(nums, vec!["1", "2"]);
    }

    #[test]
    fn accepts_single_space_after_number_rfc1034_style() {
        // RFC 1034 and other older RFCs use one space between "N.M." and the title.
        let text = "\
3.5. Preferred name syntax

   text

3.6. Resource Records

   more text

3.6.1. Textual expression of RRs

   even more
";
        let got: Vec<_> = scan_sections(text)
            .into_iter()
            .map(|s| (s.number.to_string(), s.title.to_string()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("3.5".into(), "Preferred name syntax".into()),
                ("3.6".into(), "Resource Records".into()),
                ("3.6.1".into(), "Textual expression of RRs".into()),
            ]
        );
    }

    #[test]
    fn accepts_indented_subsections_rfc1123_style() {
        // RFC 1123 sub-sections are indented and omit the trailing '.' on the number.
        let text = "\
2.  GENERAL ISSUES

   This section contains general requirements.

   2.1  Host Names and Numbers

      The syntax of a legal Internet host name...

   2.2  Using Domain Name Service

      Host domain names MUST be translated...
";
        let got: Vec<_> = scan_sections(text)
            .into_iter()
            .map(|s| (s.number.to_string(), s.title.to_string()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("2".into(), "GENERAL ISSUES".into()),
                ("2.1".into(), "Host Names and Numbers".into()),
                ("2.2".into(), "Using Domain Name Service".into()),
            ]
        );
    }

    #[test]
    fn indented_top_level_numbers_are_not_headers() {
        // A bare "N." with leading whitespace is a list item, not a section header.
        let text = "\
1.  Real Section

   1. This is a list item

2.  Other Section
";
        let nums: Vec<_> = scan_sections(text)
            .into_iter()
            .map(|s| s.number.to_string())
            .collect();
        assert_eq!(nums, vec!["1".to_string(), "2".to_string()]);
    }

    #[test]
    fn find_section_works_for_legacy_styles() {
        // RFC 1034 style.
        let rfc1034 = "\
3.4. Example name space

   example content

3.5. Preferred name syntax

   syntax content

3.6. Resource Records

   rr content
";
        let s = find_section(rfc1034, "3.5").expect("3.5 should be findable");
        assert_eq!(s.number, "3.5");
        assert_eq!(s.title, "Preferred name syntax");
        assert!(s.text.contains("syntax content"));
        assert!(!s.text.contains("rr content"));

        // RFC 1123 style.
        let rfc1123 = "\
2.  GENERAL ISSUES

   This section contains general requirements.

   2.1  Host Names and Numbers

      host name content

   2.2  Using Domain Name Service

      dns content
";
        let s = find_section(rfc1123, "2.1").expect("2.1 should be findable");
        assert_eq!(s.number, "2.1");
        assert_eq!(s.title, "Host Names and Numbers");
        assert!(s.text.contains("host name content"));
        assert!(!s.text.contains("dns content"));
    }

    #[test]
    fn skips_toc_dotleader_entries() {
        let text = "\
1.  INTRODUCTION ..................................................... 1

2.  OVERVIEW ......................................................... 5

1.  Introduction

   real intro

2.  Overview

   real overview
";
        let titles: Vec<_> = scan_sections(text)
            .into_iter()
            .map(|s| s.title.to_string())
            .collect();
        assert_eq!(
            titles,
            vec!["Introduction".to_string(), "Overview".to_string()]
        );
    }
}
