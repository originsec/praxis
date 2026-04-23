//
// Truncate an id-like string to its first 8 chars for log/display use.
// Never panics on short inputs; byte-slicing is safe because id chars
// are always single-byte ASCII (uuids, hex).
//

pub fn short_id(id: &str) -> &str {
    &id[..8.min(id.len())]
}
