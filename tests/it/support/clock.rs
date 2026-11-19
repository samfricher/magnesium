use std::path::Path;

use chrono::{Days, Local, LocalResult, NaiveDateTime, TimeDelta, TimeZone};
use rusqlite::{Connection, params};

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

pub fn days_pass(workspace: &Path, days: u64) {
    let conn = Connection::open(workspace.join(".magnesium/state.sqlite")).unwrap();
    move_column(&conn, "attempts", "answered_at_ms", days);
    move_column(&conn, "practice_events", "at_ms", days);
}
