//! Read-only, order-independent schema and content snapshots for test-selected tables.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use sha2::{Digest, Sha256};

#[derive(Debug, Eq, PartialEq)]
pub struct DatabaseSnapshot {
    tables: BTreeMap<String, Option<TableSnapshot>>,
}

#[derive(Debug, Eq, PartialEq)]
struct TableSnapshot {
    row_count: usize,
    digest: [u8; 32],
}

impl DatabaseSnapshot {
    pub fn capture(database: &Path, tables: &[&str]) -> rusqlite::Result<Self> {
        let mut snapshots = tables
            .iter()
            .map(|table| ((*table).to_owned(), None))
            .collect::<BTreeMap<_, _>>();
        if !database.exists() {
            return Ok(Self { tables: snapshots });
        }

        let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        for table in tables {
            assert!(
                table
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_'),
                "test table names must be SQL identifiers"
            );
            let schema = connection
                .query_row(
                    "SELECT coalesce(sql, '') FROM sqlite_schema WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if let Some(schema) = schema {
                let mut statement = connection.prepare(&format!("SELECT * FROM \"{table}\""))?;
                let column_count = statement.column_count();
                let mut row_digests: Vec<[u8; 32]> = Vec::new();
                let mut query = statement.query([])?;
                while let Some(row) = query.next()? {
                    let mut row_hasher = Sha256::new();
                    row_hasher.update((column_count as u64).to_le_bytes());
                    for column in 0..column_count {
                        hash_sql_value(&mut row_hasher, row.get_ref(column)?);
                    }
                    row_digests.push(row_hasher.finalize().into());
                }
                row_digests.sort_unstable();

                let mut table_hasher = Sha256::new();
                hash_bytes(&mut table_hasher, schema.as_bytes());
                table_hasher.update((row_digests.len() as u64).to_le_bytes());
                for row_digest in &row_digests {
                    table_hasher.update(row_digest);
                }
                snapshots.insert(
                    (*table).to_owned(),
                    Some(TableSnapshot {
                        row_count: row_digests.len(),
                        digest: table_hasher.finalize().into(),
                    }),
                );
            }
        }
        Ok(Self { tables: snapshots })
    }
}

fn hash_sql_value(hasher: &mut Sha256, value: ValueRef<'_>) {
    match value {
        ValueRef::Null => hasher.update([0]),
        ValueRef::Integer(value) => {
            hasher.update([1]);
            hasher.update(value.to_le_bytes());
        }
        ValueRef::Real(value) => {
            hasher.update([2]);
            hasher.update(value.to_bits().to_le_bytes());
        }
        ValueRef::Text(value) => {
            hasher.update([3]);
            hash_bytes(hasher, value);
        }
        ValueRef::Blob(value) => {
            hasher.update([4]);
            hash_bytes(hasher, value);
        }
    }
}

fn hash_bytes(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}
