# Changelog

## Unreleased

- Discovery surface for finding RFCs by metadata axis without needing
  bodies cached. `RfcQuery` gains `wg`, `area`, `stream`, `keyword`,
  `author_regex`, `abstract_regex`, `series`, `not_obsoleted`, and
  `max_year` filters; new `RfcIndex::facets(FacetKind, contains)` lists
  distinct values present in the index. CLI: `rfc facets <kind>` plus
  matching flags on `rfc index list` and `rfc fetch`. MCP: extended
  `list_rfcs` schema and a new `list_facets` tool. No schema change.

## 0.1.0 2025-05-04

Initial release
