//! Days passing, for a test: the times the learner record holds move back
//! by whole local calendar days, at the same wall-clock time.
//!
//! The runtime counts days on the local calendar. A day is not always 24
//! hours: the day the clocks go back has 25, the day they go forward 23. A
//! time moved back by multiples of 24 hours across such a day lands an hour
//! off on the wall clock, and at 23:00 or later on the next local day. Two
//! answers meant for separate days then share one, and a pass is lost. So
//! times move by the calendar here, never by a fixed count of milliseconds.

use std::path::Path;

use chrono::{Days, Local, LocalResult, NaiveDateTime, TimeDelta, TimeZone};
use rusqlite::{Connection, params};

/// `ms` moved back `days` local calendar days, at the same wall-clock time.
/// A wall time the clocks skip on the earlier day becomes the first time
/// after the gap; one they repeat becomes the first of the two.
pub fn days_earlier(ms: i64, days: u64) -> i64 {
    let local = Local
        .timestamp_millis_opt(ms)
        .single()
        .expect("a stored time is one instant");
    let wall = local
        .naive_local()
        .checked_sub_days(Days::new(days))
        .expect("a date in range");
    at_wall(wall).timestamp_millis()
}

fn at_wall(wall: NaiveDateTime) -> chrono::DateTime<Local> {
    match Local.from_local_datetime(&wall) {
        LocalResult::Single(time) => time,
        LocalResult::Ambiguous(first, _) => first,
        LocalResult::None => {
            // In a gap: the first minute after it, at most an hour on.
            let mut later = wall;
            loop {
                later += TimeDelta::minutes(1);
                if let Some(time) = Local.from_local_datetime(&later).earliest() {
                    break time;
                }
                assert!(later - wall <= TimeDelta::hours(2), "no gap is this long");
            }
        }
    }
}

/// Move every column `column` of `table` back `days` local calendar days.
pub fn move_column(conn: &Connection, table: &str, column: &str, days: u64) {
    let rows: Vec<(i64, i64)> = conn
        .prepare(&format!("SELECT rowid, {column} FROM {table}"))
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for (rowid, ms) in rows {
        conn.execute(
            &format!("UPDATE {table} SET {column}=?1 WHERE rowid=?2"),
            params![days_earlier(ms, days), rowid],
        )
        .unwrap();
    }
}

/// What `days` later looks like to the runtime's learner store: every
/// answer and every change to a practice item moves that many local days
/// back.
pub fn days_pass(workspace: &Path, days: u64) {
    let conn = Connection::open(workspace.join(".magnesium/state.sqlite")).unwrap();
    move_column(&conn, "attempts", "answered_at_ms", days);
    move_column(&conn, "practice_events", "at_ms", days);
}
