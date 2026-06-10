use std::path::Path;

//
// Drift detection between the canonical log-query schema and the consumers
// that can't import it directly (currently the documentation page). The
// test doesn't validate full structure — it asserts every canonical table
// and column name appears in the artifact, so a schema change that forgets
// a manual sync point fails CI with a pointer to the file needing updates.
//

fn check_artifact_contains_schema(relative_path: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(relative_path);

    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));

    let mut missing = Vec::new();
    for table in common::log_query_schema::TABLES {
        if !content.contains(table.name) {
            missing.push(format!("table {}", table.name));
        }
        for column in table.columns {
            if !content.contains(column.name) {
                missing.push(format!("column {}.{}", table.name, column.name));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "{} is out of sync with common::log_query_schema — missing: {}",
        relative_path,
        missing.join(", ")
    );
}

#[test]
fn log_query_docs_in_sync() {
    check_artifact_contains_schema("docs/src/usage/log-query.md");
}
