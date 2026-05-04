use crate::error::Result;
use reqwest::blocking::Client;

pub(crate) struct HeadProbe {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

pub(crate) fn head_probe(client: &Client, url: &str) -> Result<Option<HeadProbe>> {
    let resp = client.head(url).send()?;
    if !resp.status().is_success() {
        return Ok(None);
    }
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
    Ok(Some(HeadProbe {
        etag,
        last_modified,
    }))
}

/// Prefer etag (strong); fall back to last-modified only if no prior etag is stored.
pub(crate) fn validators_match(
    prior_etag: &Option<String>,
    new_etag: &Option<String>,
    prior_lm: &Option<String>,
    new_lm: &Option<String>,
) -> bool {
    if prior_etag.is_some() {
        return matches!((prior_etag, new_etag), (Some(p), Some(c)) if p == c);
    }
    if let (Some(p), Some(c)) = (prior_lm, new_lm) {
        return p == c;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::validators_match;

    #[test]
    fn prefers_etag_when_previously_stored() {
        assert!(!validators_match(
            &Some("etag-1".into()),
            &None,
            &Some("lm-1".into()),
            &Some("lm-1".into()),
        ));
    }

    #[test]
    fn falls_back_to_last_modified_only_without_prior_etag() {
        assert!(validators_match(
            &None,
            &None,
            &Some("lm-1".into()),
            &Some("lm-1".into()),
        ));
    }
}
