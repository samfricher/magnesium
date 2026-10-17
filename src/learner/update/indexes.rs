use super::*;
pub(super) fn maintain_indexes(
    conn: &Connection,
    memory_ids: &BTreeSet<String>,
    observation_ids: &[String],
) -> Result<String> {
    let memories = !memory_ids.is_empty();
    let observations = !observation_ids.is_empty();
    let memory_current = crate::learner::search::index_current(conn, false)?;
    let observation_current = crate::learner::search::index_current(conn, true)?;
    conn.execute(
        "UPDATE workspace_meta
         SET memory_generation = memory_generation + ?1,
             observation_generation = observation_generation + ?2",
        params![memories as i64, observations as i64],
    )?;
    conn.execute_batch("SAVEPOINT fts_maintenance")?;
    let maintenance = (|| -> rusqlite::Result<()> {
        if memories && memory_current {
            crate::learner::search::sync_memories(conn, memory_ids)?;
            conn.execute_batch(
                "UPDATE workspace_meta SET memory_fts_generation=memory_generation",
            )?;
        }
        if observations && observation_current {
            for id in observation_ids {
                conn.execute("INSERT INTO observation_fts(
                     rowid, observation_id, description, conditions_text, assistance_text
                 )
                 SELECT observation_no, observation_id, description, conditions_text, assistance_text
                 FROM observations WHERE observation_id = ?1",[id])?;
            }
            conn.execute_batch(
                "UPDATE workspace_meta SET observation_fts_generation=observation_generation",
            )?;
        }
        Ok(())
    })();
    if let Err(error) = maintenance {
        if conn.is_autocommit() {
            return Err(error.into());
        }
        conn.execute_batch("ROLLBACK TO fts_maintenance; RELEASE fts_maintenance")?;
        if let rusqlite::Error::SqliteFailure(err, _) = &error
            && matches!(
                err.code,
                rusqlite::ErrorCode::SystemIoFailure
                    | rusqlite::ErrorCode::DatabaseCorrupt
                    | rusqlite::ErrorCode::DiskFull
            )
        {
            return Err(error.into());
        }
        return Ok("dirty".into());
    }
    conn.execute_batch("RELEASE fts_maintenance")?;
    Ok(if memory_current && observation_current {
        "current"
    } else {
        "dirty"
    }
    .into())
}
