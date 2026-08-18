//! Store behavior tests.

use super::migrations::{migration_v1, migration_v2, migration_v3, migration_v4, migration_v5};
use super::*;
use crate::graph;
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture_root() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("flopeek-store-{suffix}"));
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(root.join("src/main.ts"), "export const main = 1;").expect("write");
    root
}

fn flow_fixture_root() -> PathBuf {
    let root = fixture_root();
    fs::write(
        root.join("package.json"),
        r#"{
            "scripts": {
                "start": "tsx src/main.ts",
                "unsupported": "tsx src/main.ts && echo credential-sentinel"
            },
            "bin": {"checkout": "src/main.ts"},
            "main": "src/main",
            "module": "src/main.ts"
        }"#,
    )
    .expect("package manifest");
    fs::write(
        root.join("src/main.ts"),
        "export function main() { helper(); }\nfunction helper() { return 'source-body-sentinel'; }\nmain();\n",
    )
    .expect("main source");
    fs::create_dir_all(root.join("tests")).expect("tests");
    fs::write(
        root.join("tests/main.test.ts"),
        "import { main } from '../src/main'; main();\n",
    )
    .expect("test source");
    root
}

fn schema_snapshot(connection: &rusqlite::Connection) -> Vec<(String, String, String)> {
    connection
        .prepare(
            "SELECT type, name, COALESCE(sql, '') FROM sqlite_master
             WHERE name NOT LIKE 'sqlite_%' AND type IN ('table', 'index')
             ORDER BY type, name",
        )
        .expect("schema query")
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .expect("schema rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("schema snapshot")
}

fn initialize_v5_database(root: &Path) {
    fs::create_dir_all(root.join(STORE_DIRECTORY)).expect("store directory");
    let mut connection = rusqlite::Connection::open(database_path(root)).expect("sqlite");
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .expect("foreign keys");
    for target in 1..=5 {
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .expect("migration transaction");
        match target {
            1 => migration_v1(&transaction).expect("v1"),
            2 => migration_v2(&transaction).expect("v2"),
            3 => migration_v3(&transaction).expect("v3"),
            4 => migration_v4(&transaction).expect("v4"),
            5 => migration_v5(&transaction).expect("v5"),
            _ => unreachable!(),
        }
        transaction
            .execute_batch(&format!("PRAGMA user_version = {target};"))
            .expect("version");
        transaction.commit().expect("migration commit");
    }
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

mod flow;
mod graph_behavior;
mod legacy;
mod migration;
