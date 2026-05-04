use crate::error::{Error, Result};
use crate::model::{Body, SectionRef};
use reqwest::StatusCode;
use reqwest::blocking::Client;

pub(crate) fn fetch(client: &Client, number: u32, has_xml: bool) -> Result<Body> {
    let text_url = format!("https://www.rfc-editor.org/rfc/rfc{number}.txt");
    let text = fetch_string(client, &text_url)?
        .ok_or_else(|| Error::NotFound(format!("RFC {number} text body (404)")))?;

    let xml = if has_xml {
        let xml_url = format!("https://www.rfc-editor.org/rfc/rfc{number}.xml");
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
fn parse_header_line(line: &str) -> Option<((usize, usize), (usize, usize))> {
    let bytes = line.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_digit() {
        return None;
    }

    // Walk N(.N)* prefix.
    let mut i = 0usize;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    while i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    let num_end = i;
    if num_end == 0 {
        return None;
    }

    // Required terminating '.' after the number.
    if i >= bytes.len() || bytes[i] != b'.' {
        return None;
    }
    i += 1;

    // Require two or more spaces (xml2rfc convention) before the title.
    let space_start = i;
    while i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }
    if i - space_start < 2 {
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

    Some(((0, num_end), (title_start, title_end)))
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
    fn requires_two_spaces_after_number() {
        let text = "\
1. NotAHeader

1.  Header
";
        let nums: Vec<_> = scan_sections(text)
            .into_iter()
            .map(|s| s.number.to_string())
            .collect();
        assert_eq!(nums, vec!["1".to_string()]);
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
