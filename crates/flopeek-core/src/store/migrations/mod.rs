//! SQLite schema initialization and migrations.

#[allow(unused_imports)]
use super::*;

mod base;
mod evidence;
mod helpers;
mod temporal;
mod versions;

pub(super) use base::{migration_v1, migration_v2, migration_v3, migration_v4};
pub(super) use evidence::migration_v8;
pub(super) use helpers::*;
#[cfg(test)]
pub(crate) use temporal::migration_v7;
pub(super) use versions::{migration_v5, migration_v6};

#[allow(unused_imports)]
use super::*;

pub(super) fn initialize_schema(connection: &mut Connection) -> Result<(), String> {
    let mut version = connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(|error| format!("Unable to read SQLite schema version: {error}"))?;
    if version > CURRENT_USER_VERSION {
        return Err(format!(
            "SQLite database schema version {version} is newer than supported version {CURRENT_USER_VERSION}."
        ));
    }

    while version < CURRENT_USER_VERSION {
        let target = version + 1;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| {
                format!("Unable to begin SQLite migration {version}->{target}: {error}")
            })?;
        match target {
            1 => migration_v1(&transaction)?,
            2 => migration_v2(&transaction)?,
            3 => migration_v3(&transaction)?,
            4 => migration_v4(&transaction)?,
            5 => migration_v5(&transaction)?,
            6 => migration_v6(&transaction)?,
            7 => temporal::migration_v7(&transaction)?,
            8 => migration_v8(&transaction)?,
            _ => unreachable!("migration target is bounded by CURRENT_USER_VERSION"),
        }
        transaction
            .execute_batch(&format!("PRAGMA user_version = {target};"))
            .map_err(|error| {
                format!("Unable to record SQLite migration version {target}: {error}")
            })?;
        transaction.commit().map_err(|error| {
            format!("Unable to commit SQLite migration {version}->{target}: {error}")
        })?;
        version = target;
    }
    Ok(())
}
