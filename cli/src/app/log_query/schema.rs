//
// Schema reference for the Log Query window. The canonical definition lives
// in common::log_query_schema and is shared with the service; this module
// re-exports it under the names the CLI has always used.
//

pub use common::log_query_schema::{TableSchema as TableInfo, find_table};

pub const TABLES: &[TableInfo] = common::log_query_schema::TABLES;
