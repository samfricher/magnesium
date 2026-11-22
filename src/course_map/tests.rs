use std::path::PathBuf;

use chrono::NaiveDate;
use serde_json::json;

use super::facts::{Evidence, Map, Record, reach_of};
use super::request::CourseMapRequest;
use super::store::MapStore;
use super::write::{Source, apply, insert_sections};
use crate::retrieval::SectionFact;

struct Folder {
    root: PathBuf,
}

impl Folder {
    fn new(files: &[&str]) -> Self {
        let root = std::env::temp_dir().join(crate::learner::store::new_id("map-test").unwrap());
        for file in files {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "x").unwrap();
        }
        Self { root }
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn request(value: serde_json::Value) -> CourseMapRequest {
    serde_json::from_value(value).unwrap()
}

fn write(
    store: &mut MapStore,
    folder: &Folder,
    value: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let index = folder.root.join(".magnesium/index/none.sqlite");
    let source = Source {
        root: &folder.root,
        index: &index,
        now_ms: 1,
    };
    let tx = store.write()?;
    let (report, revision) = apply(&tx, &source, &request(value))?;
    tx.commit().unwrap();
    Ok(report.to_json(revision))
}

fn section(heading: &str, start: u32, end: u32) -> SectionFact {
    SectionFact {
        heading: heading.into(),
        start,
        end,
    }
}

fn day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 18).unwrap()
}

fn course() -> (MapStore, Folder) {
    let folder = Folder::new(&[
        "Materials/lecture-08.html",
        "Materials/week8-list.png",
        "Materials/old-test.pdf",
    ]);
    let mut store = MapStore::in_memory().unwrap();
    {
        let tx = store.write().unwrap();
        tx.execute(
            "INSERT INTO files(code, path, readable, added_ms) VALUES ('lec08', 'Materials/lecture-08.html', 1, 1)",
            [],
        )
        .unwrap();
        insert_sections(
            &tx,
            "lec08",
            None,
            &[
                section(
                    "Lecture 8 > Language models > RNN for language modelling",
                    10,
                    40,
                ),
                section("Lecture 8 > Language models > Teacher forcing", 41, 60),
                section("Lecture 8 > LSTM > LSTM Math", 61, 90),
                section("Lecture 8 > Text generation", 91, 120),
            ],
            1,
        )
        .unwrap();
        tx.commit().unwrap();
    }
    (store, folder)
}

#[test]
fn sections_from_heading_paths_nest_under_their_parents_and_drop_a_shared_title() {
    let (store, _folder) = course();
    let map = Map::load(&store.conn).unwrap();
    let rows = map
        .items
        .iter()
        .map(|i| {
            (
                i.id.as_str(),
                i.parent.as_deref(),
                i.title.as_str(),
                i.start,
                i.end,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        vec![
            ("lec08.1", None, "Language models", Some(10), Some(60)),
            (
                "lec08.2",
                Some("lec08.1"),
                "RNN for language modelling",
                Some(10),
                Some(40)
            ),
            (
                "lec08.3",
                Some("lec08.1"),
                "Teacher forcing",
                Some(41),
                Some(60)
            ),
            ("lec08.4", None, "LSTM", Some(61), Some(90)),
            ("lec08.5", Some("lec08.4"), "LSTM Math", Some(61), Some(90)),
            ("lec08.6", None, "Text generation", Some(91), Some(120)),
        ]
    );
}

#[test]
fn items_read_by_eye_take_the_file_code_and_their_ref() {
    let (mut store, folder) = course();
    let report = write(
        &mut store,
        &folder,
        json!({"action": "write", "items": [
            {"file": "Materials/week8-list.png", "kind": "exercise", "title": "the maximum of two uniforms", "ref": "6.63"},
            {"file": "Materials/week8-list.png", "kind": "exercise", "title": "Exercise 6.72"},
            {"parent": "lec08.5", "kind": "idea", "title": "Forget gate keeps or clears the cell"}
        ]}),
    )
    .unwrap();
    assert_eq!(
        report["items_added"],
        json!([
            {"id": "week8-list.6.63", "title": "the maximum of two uniforms"},
            {"id": "week8-list.1", "title": "Exercise 6.72"},
            {"id": "lec08.5.1", "title": "Forget gate keeps or clears the cell"},
        ])
    );
    assert_eq!(report["files_added"][0]["code"], "week8-list");
    let map = Map::load(&store.conn).unwrap();
    assert_eq!(
        map.title("week8-list.6.63"),
        "Exercise 6.63: the maximum of two uniforms"
    );
    assert_eq!(map.title("week8-list.1"), "Exercise 6.72");
    let after_math = map.items.iter().position(|i| i.id == "lec08.5.1").unwrap();
    assert_eq!(map.items[after_math - 1].id, "lec08.5");
    assert_eq!(map.items[after_math + 1].id, "lec08.6");
    let again = write(
        &mut store,
        &folder,
        json!({"action": "write", "items": [{"file": "week8-list", "kind": "exercise", "title": "Again", "ref": "6.63"}]}),
    );
    assert!(again.unwrap_err().contains("already on the map"));
}

#[test]
fn scope_keeps_the_instructors_order_and_a_cut_needs_its_reason() {
    let (mut store, folder) = course();
    write(
        &mut store,
        &folder,
        json!({"action": "write", "target": "midterm-2",
               "items": [{"file": "Materials/week8-list.png", "kind": "exercise", "title": "Exercise 1", "ref": "1"}],
               "facts": {"label": "Midterm 2", "at": "2026-11-20 16:00"},
               "scope": [{"ref": "week8-list", "state": "in"}, {"ref": "lec08", "state": "in"}]}),
    )
    .unwrap();
    let missing = write(
        &mut store,
        &folder,
        json!({"action": "write", "target": "midterm-2", "scope": [{"ref": "lec08.4", "state": "cut"}]}),
    );
    assert!(missing.unwrap_err().contains("reason"));
    write(
        &mut store,
        &folder,
        json!({"action": "write", "target": "midterm-2", "scope": [{"ref": "lec08.4", "state": "cut", "reason": "the LSTM gates, not on this test"}]}),
    )
    .unwrap();
    let map = Map::load(&store.conn).unwrap();
    let scoped = map.in_scope("midterm-2");
    assert_eq!(
        scoped.items,
        vec!["week8-list.1", "lec08.1", "lec08.2", "lec08.3", "lec08.6"]
    );
    assert_eq!(scoped.cut, vec!["lec08.4", "lec08.5"]);
    assert!(!scoped.cuts[0].confirmed);
    write(
        &mut store,
        &folder,
        json!({"action": "write", "target": "midterm-2", "scope": [{"ref": "lec08.4", "state": "confirm"}]}),
    )
    .unwrap();
    assert!(Map::load(&store.conn).unwrap().in_scope("midterm-2").cuts[0].confirmed);
}

#[test]
fn an_item_is_asked_by_the_plan_only_through_a_task_or_a_carrier() {
    let (mut store, folder) = course();
    write(
        &mut store,
        &folder,
        json!({"action": "write", "target": "midterm-2",
               "items": [{"kind": "task", "title": "Write a sampling loop", "ref": "d1"}],
               "links": [{"item": "task.d1", "uses": ["lec08.6", "lec08.2"]}],
               "scope": [{"ref": "lec08", "state": "in"}],
               "blocks": [{"title": "Language models", "sections": ["lec08.1"], "tasks": ["task.d1"], "minutes": 30, "opens_with": "test"}],
               "carried": [{"idea": "lec08.3", "by": "task.d1"}]}),
    )
    .unwrap();
    let map = Map::load(&store.conn).unwrap();
    let plan = super::facts::planned(&map, "midterm-2");
    let unasked = map
        .in_scope("midterm-2")
        .items
        .into_iter()
        .filter(|id| map.is_leaf(id) && !plan.asked(id))
        .collect::<Vec<_>>();
    assert_eq!(unasked, vec!["lec08.5"]);
    let bad = write(
        &mut store,
        &folder,
        json!({"action": "write", "target": "midterm-2", "blocks": [{"title": "x", "tasks": ["task.d9"]}]}),
    );
    assert!(bad.unwrap_err().contains("task.d9"));
    write(
        &mut store,
        &folder,
        json!({"action": "write", "remove_items": ["task.d1"]}),
    )
    .unwrap();
    let map = Map::load(&store.conn).unwrap();
    assert!(map.blocks["midterm-2"][0].tasks.is_empty());
    assert!(!map.carried.contains_key("midterm-2"));
}

#[test]
fn reach_counts_used_items_by_their_record_and_passes_nothing_along_a_link() {
    let (mut store, folder) = course();
    write(
        &mut store,
        &folder,
        json!({"action": "write", "links": [
            {"item": "lec08.6", "uses": ["lec08.2", "lec08.3", "lec08.5"]},
            {"item": "lec08.3", "uses": ["lec08.2"]}
        ]}),
    )
    .unwrap();
    let map = Map::load(&store.conn).unwrap();
    let mut evidence = Evidence::default();
    evidence.by_label.insert(
        "lec08.2".into(),
        Record {
            asked: 2,
            passes: 1,
            helped: 0,
            last_ms: Some(1),
        },
    );
    evidence.by_label.insert(
        "lec08.6".into(),
        Record {
            asked: 1,
            passes: 1,
            helped: 0,
            last_ms: Some(1),
        },
    );
    evidence.due.insert("lec08.3".into());
    let reach = reach_of(&map, &evidence, "lec08.6");
    assert_eq!(reach["uses"], 3);
    assert_eq!(reach["never_asked"], 2);
    assert_eq!(reach["no_blank_page_pass"], 2);
    assert_eq!(reach["due"], 1);
    assert_eq!(reach_of(&map, &evidence, "lec08.3")["never_asked"], 0);
    assert_eq!(evidence.phrase("lec08.5"), "never asked");
    assert_eq!(evidence.phrase("lec08.3"), "never asked, due");
}

#[test]
fn the_plan_page_keeps_the_rows_the_digest_and_the_window_read() {
    let (mut store, folder) = course();
    write(
        &mut store,
        &folder,
        json!({"action": "write", "target": "midterm-2",
               "facts": {"label": "Midterm 2", "at": "2026-11-20 16:00", "length_minutes": 50, "hours": 6,
                         "sessions": ["If it is Monday to Friday at 08:00, then I work the next block at my desk.", "If I miss a morning, then I work it after dinner."]},
               "scope": [{"ref": "lec08", "state": "in"}],
               "blocks": [{"title": "Language models", "sections": ["lec08.1"], "minutes": 30}]}),
    )
    .unwrap();
    let map = Map::load(&store.conn).unwrap();
    let target = map.target("midterm-2").unwrap();
    let page = super::render::plan_page(&map, &Evidence::default(), target, day());
    let plan_dir = folder.root.join("Study/midterm-2");
    std::fs::create_dir_all(&plan_dir).unwrap();
    std::fs::write(plan_dir.join("plan.md"), &page).unwrap();
    assert_eq!(
        crate::digest::plan_test_date(&folder.root, "midterm-2"),
        NaiveDate::from_ymd_opt(2026, 11, 20)
    );
    assert!(page.contains("| When | 2026-11-20 (Fri) 16:00 |"));
    assert!(page.contains("| 1 | Language models |"));
    assert!(
        page.contains(
            "- If it is Monday to Friday at 08:00, then I work the next block at my desk."
        )
    );
    assert!(
        page.contains("| `lec08.5` | LSTM Math | `lec08.4` LSTM | no task yet | never asked |")
    );
}

#[test]
fn a_hand_edit_of_a_page_is_kept_as_a_backup_before_it_is_replaced() {
    let folder = Folder::new(&[]);
    std::fs::create_dir_all(folder.root.join(".magnesium")).unwrap();
    let workspace = crate::workspace::Workspace::discover(&folder.root)
        .unwrap()
        .unwrap();
    let store = MapStore::open(&workspace).unwrap();
    let first = super::write_page(&workspace, &store, "t", "plan.md", "one\n").unwrap();
    assert_eq!(first, "Study/t/plan.md");
    std::fs::write(folder.root.join("Study/t/plan.md"), "edited by hand\n").unwrap();
    super::write_page(&workspace, &store, "t", "plan.md", "two\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(folder.root.join("Study/t/plan.md")).unwrap(),
        "two\n"
    );
    let backups = std::fs::read_dir(folder.root.join(".magnesium/backups/map"))
        .unwrap()
        .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(backups, vec!["edited by hand\n"]);
}

#[test]
fn file_codes_keep_the_first_and_last_words_of_a_long_name() {
    for (path, code) in [
        (
            "Materials/Week09/Exercises/optional-exercises-w08.html",
            "optional-w08",
        ),
        ("Materials/lecture-08.html", "lecture-08"),
        (
            "Materials/Week09/Readings/exploding-vanishing-gradients-L07b.pdf",
            "exploding-l07b",
        ),
        (
            "Materials/Lecture Slides/week1-suggested.png",
            "week1-suggested",
        ),
        (
            "Materials/course101w18-test2-lec0101-V2.pdf",
            "course101w18-v2",
        ),
    ] {
        assert_eq!(super::write::code_from_path(path), code, "{path}");
    }
}

#[test]
fn a_symlinked_store_is_refused() {
    let folder = Folder::new(&[]);
    std::fs::create_dir_all(folder.root.join(".magnesium")).unwrap();
    let elsewhere = folder.root.join("elsewhere.sqlite");
    std::fs::write(&elsewhere, b"").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&elsewhere, folder.root.join(".magnesium/map.sqlite")).unwrap();
    let workspace = crate::workspace::Workspace::discover(&folder.root)
        .unwrap()
        .unwrap();
    #[cfg(unix)]
    assert!(MapStore::open(&workspace).is_err());
}

fn workspace_folder(files: &[&str]) -> Folder {
    let folder = Folder::new(files);
    std::fs::create_dir_all(folder.root.join(".magnesium")).unwrap();
    folder
}

#[test]
fn a_write_that_commits_reports_a_page_it_could_not_write_and_a_retry_adds_nothing_twice() {
    let folder = workspace_folder(&[]);
    std::fs::create_dir_all(folder.root.join("Study/t/plan.md")).unwrap();
    let first = super::handle(
        &folder.root,
        request(json!({"action": "write", "target": "t", "facts": {"label": "T"}, "items": [{"kind": "task", "title": "one", "ref": "a"}]})),
    )
    .unwrap();
    assert!(
        first["page_warnings"][0]
            .as_str()
            .unwrap()
            .contains("Study/t/plan.md"),
        "{first}"
    );
    let again = super::handle(
        &folder.root,
        request(
            json!({"action": "write", "target": "t", "items": [{"kind": "task", "title": "one", "ref": "a"}]}),
        ),
    );
    assert!(again.unwrap_err().contains("already on the map"));
}

#[test]
fn a_removed_id_is_never_given_to_a_new_item() {
    let (mut store, folder) = course();
    write(
        &mut store,
        &folder,
        json!({"action": "write", "items": [
        {"kind": "task", "title": "one"}, {"kind": "task", "title": "two"}]}),
    )
    .unwrap();
    write(
        &mut store,
        &folder,
        json!({"action": "write", "remove_items": ["task.2"]}),
    )
    .unwrap();
    let added = write(
        &mut store,
        &folder,
        json!({"action": "write", "items": [{"kind": "task", "title": "three"}]}),
    )
    .unwrap();
    assert_eq!(added["items_added"][0]["id"], "task.3");
    let refused = write(
        &mut store,
        &folder,
        json!({"action": "write", "items": [{"kind": "task", "title": "x", "ref": "2"}]}),
    );
    assert!(refused.unwrap_err().contains("removed"));
    write(
        &mut store,
        &folder,
        json!({"action": "write", "remove_items": ["lec08.4"]}),
    )
    .unwrap();
    let section = write(
        &mut store,
        &folder,
        json!({"action": "write", "items": [{"file": "lec08", "kind": "section", "title": "new"}]}),
    )
    .unwrap();
    assert_eq!(section["items_added"][0]["id"], "lec08.7");
}

#[test]
fn removing_an_item_takes_the_cut_rows_of_the_items_inside_it() {
    let (mut store, folder) = course();
    write(&mut store, &folder, json!({"action": "write", "target": "m",
        "scope": [{"ref": "lec08", "state": "in"}, {"ref": "lec08.3", "state": "cut", "reason": "teacher forcing, not on this test"}]}))
    .unwrap();
    write(
        &mut store,
        &folder,
        json!({"action": "write", "remove_items": ["lec08.1"]}),
    )
    .unwrap();
    let map = Map::load(&store.conn).unwrap();
    assert!(map.in_scope("m").cuts.is_empty());
}

#[test]
fn a_task_inside_a_section_leaves_the_section_counted() {
    let (mut store, folder) = course();
    write(
        &mut store,
        &folder,
        json!({"action": "write", "target": "m", "scope": [{"ref": "lec08", "state": "in"}],
        "items": [{"parent": "lec08.6", "kind": "task", "title": "sample a sequence"}]}),
    )
    .unwrap();
    let map = Map::load(&store.conn).unwrap();
    let leaves = map
        .in_scope("m")
        .items
        .into_iter()
        .filter(|id| map.is_leaf(id))
        .collect::<Vec<_>>();
    assert_eq!(leaves, vec!["lec08.2", "lec08.3", "lec08.5", "lec08.6"]);
}

#[test]
fn an_empty_schedule_writes_no_block_row() {
    let (mut store, folder) = course();
    write(
        &mut store,
        &folder,
        json!({"action": "write", "target": "m", "facts": {"label": "M"}}),
    )
    .unwrap();
    let map = Map::load(&store.conn).unwrap();
    let page =
        super::render::plan_page(&map, &Evidence::default(), map.target("m").unwrap(), day());
    let blocks = page
        .split("## Blocks")
        .nth(1)
        .unwrap()
        .split("\n## ")
        .next()
        .unwrap();
    assert_eq!(
        blocks
            .lines()
            .filter(|l| l.starts_with("| ") && !l.starts_with("| #"))
            .count(),
        0,
        "{blocks}"
    );
}

#[test]
fn an_idea_counts_as_carried_only_when_its_carrier_is_in_the_schedule() {
    let (mut store, folder) = course();
    write(
        &mut store,
        &folder,
        json!({"action": "write", "target": "m", "scope": [{"ref": "lec08", "state": "in"}],
        "items": [{"kind": "task", "title": "a", "ref": "a"}],
        "carried": [{"idea": "lec08.2", "by": "task.a"}]}),
    )
    .unwrap();
    let map = Map::load(&store.conn).unwrap();
    assert!(!super::facts::planned(&map, "m").asked("lec08.2"));
    write(
        &mut store,
        &folder,
        json!({"action": "write", "target": "m", "blocks": [{"title": "b", "tasks": ["task.a"]}]}),
    )
    .unwrap();
    let map = Map::load(&store.conn).unwrap();
    assert!(super::facts::planned(&map, "m").asked("lec08.2"));
}

#[test]
fn a_heading_repeated_later_in_a_file_is_a_new_item_with_its_own_span() {
    let (store, _folder) = course();
    let mut store = store;
    let tx = store.write().unwrap();
    tx.execute(
        "INSERT INTO files(code, path, readable, added_ms) VALUES ('s', 'Materials/s.pdf', 1, 1)",
        [],
    )
    .unwrap();
    insert_sections(
        &tx,
        "s",
        None,
        &[
            section("Intro", 1, 2),
            section("Example", 3, 3),
            section("Bayes", 4, 10),
            section("Example", 11, 11),
            section("MLE", 12, 30),
            section("Example", 31, 31),
        ],
        1,
    )
    .unwrap();
    tx.commit().unwrap();
    let map = Map::load(&store.conn).unwrap();
    let examples = map
        .items
        .iter()
        .filter(|i| i.title == "Example")
        .map(|i| (i.start, i.end))
        .collect::<Vec<_>>();
    assert_eq!(
        examples,
        vec![
            (Some(3), Some(3)),
            (Some(11), Some(11)),
            (Some(31), Some(31))
        ]
    );
}

#[test]
fn a_child_of_an_item_with_no_file_comes_right_after_its_parent() {
    let (mut store, folder) = course();
    let items = (1..=10)
        .map(|n| json!({"kind": "idea", "title": format!("idea {n}")}))
        .collect::<Vec<_>>();
    write(
        &mut store,
        &folder,
        json!({"action": "write", "items": items}),
    )
    .unwrap();
    write(&mut store, &folder, json!({"action": "write", "items": [{"parent": "outline.9", "kind": "idea", "title": "inside 9"}]})).unwrap();
    let map = Map::load(&store.conn).unwrap();
    let order = map
        .items
        .iter()
        .filter(|i| i.file.is_none())
        .map(|i| i.id.as_str())
        .collect::<Vec<_>>();
    let at = order.iter().position(|id| *id == "outline.9").unwrap();
    assert_eq!(
        &order[at..at + 3],
        &["outline.9", "outline.9.1", "outline.10"]
    );
}

#[test]
fn a_whole_file_cut_counts_as_cut() {
    let (mut store, folder) = course();
    write(
        &mut store,
        &folder,
        json!({"action": "write", "target": "m", "scope": [
        {"ref": "Materials/week8-list.png", "state": "in"},
        {"ref": "lec08", "state": "cut", "reason": "lecture 8, after this test"}]}),
    )
    .unwrap();
    let map = Map::load(&store.conn).unwrap();
    let cut = map
        .in_scope("m")
        .cut
        .into_iter()
        .filter(|id| map.is_leaf(id))
        .count();
    assert_eq!(cut, 4);
}

#[test]
fn reach_counts_what_a_section_and_the_items_inside_it_use_from_elsewhere() {
    let (mut store, folder) = course();
    write(
        &mut store,
        &folder,
        json!({"action": "write", "links": [
        {"item": "lec08.6", "uses": ["lec08.5"]},
        {"item": "lec08.3", "uses": ["lec08.2", "lec08.5"]}]}),
    )
    .unwrap();
    let map = Map::load(&store.conn).unwrap();
    let reach = reach_of(&map, &Evidence::default(), "lec08.1");
    assert_eq!(
        (reach["inside"].as_u64(), reach["uses"].as_u64()),
        (Some(2), Some(1))
    );
}

#[cfg(unix)]
#[test]
fn a_backup_is_never_written_through_a_symlinked_backups_folder() {
    let folder = workspace_folder(&[]);
    let outside = std::env::temp_dir().join(crate::learner::store::new_id("map-outside").unwrap());
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, folder.root.join(".magnesium/backups")).unwrap();
    let workspace = crate::workspace::Workspace::discover(&folder.root)
        .unwrap()
        .unwrap();
    let store = MapStore::open(&workspace).unwrap();
    super::write_page(&workspace, &store, "t", "plan.md", "one\n").unwrap();
    std::fs::write(folder.root.join("Study/t/plan.md"), "edited\n").unwrap();
    assert!(super::write_page(&workspace, &store, "t", "plan.md", "two\n").is_err());
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(outside);
}
