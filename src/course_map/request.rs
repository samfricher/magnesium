use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MapAction {
    #[default]
    Inventory,
    AddFiles,
    Write,
    Read,
    Reach,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ItemKind {
    Section,
    Idea,
    Exercise,
    Question,
    Example,
    Reading,
    Foundation,
    Task,
}

impl ItemKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Section => "section",
            Self::Idea => "idea",
            Self::Exercise => "exercise",
            Self::Question => "question",
            Self::Example => "example",
            Self::Reading => "reading",
            Self::Foundation => "foundation",
            Self::Task => "task",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CourseMapRequest {
    #[schemars(
        description = "inventory lists every file in the folder and whether it can be read as text; add_files puts files on the map, with the sections the runtime reads from their headings; write adds or changes items, use links, a test's facts and scope, the schedule, carried ideas and traces; read states the map for a target, a file or an item, with coverage; reach counts, for each candidate, the items it uses that were never asked, have no blank-page pass, or are due."
    )]
    pub(crate) action: MapAction,
    #[serde(default)]
    #[schemars(
        description = "The test or goal this is for: a short slug such as test-2 or ch4-logs, the folder name under Study/."
    )]
    pub(crate) target: Option<String>,
    #[serde(default)]
    #[schemars(description = "inventory: list the files under this folder one by one.")]
    pub(crate) path_prefix: Option<String>,
    #[serde(default)]
    #[schemars(description = "add_files: the files to put on the map.")]
    pub(crate) files: Vec<FileAdd>,
    #[serde(default)]
    #[schemars(description = "write: items to add, or to change when id is given.")]
    pub(crate) items: Vec<ItemWrite>,
    #[serde(default)]
    #[schemars(description = "write: item ids to take off the map, with their links.")]
    pub(crate) remove_items: Vec<String>,
    #[serde(default)]
    #[schemars(description = "write: use links to add.")]
    pub(crate) links: Vec<LinkWrite>,
    #[serde(default)]
    #[schemars(description = "write: use links to remove.")]
    pub(crate) unlink: Vec<LinkWrite>,
    #[serde(default)]
    #[schemars(description = "write: the target's facts. Needs target.")]
    pub(crate) facts: Option<TargetFacts>,
    #[serde(default)]
    #[schemars(
        description = "write: files, folders or items in scope or cut, for target. Rows marked in are kept in the order given, which is the instructor's order."
    )]
    pub(crate) scope: Vec<ScopeWrite>,
    #[serde(default)]
    #[schemars(
        description = "write: the whole schedule for target, in order. It replaces the schedule there."
    )]
    pub(crate) blocks: Option<Vec<BlockWrite>>,
    #[serde(default)]
    #[schemars(
        description = "write: blocks of the schedule finished, by number, with the date and the minutes they took. Leaves the rest of the schedule as it is."
    )]
    pub(crate) blocks_done: Vec<BlockDone>,
    #[serde(default)]
    #[schemars(
        description = "write: for each idea not asked on its own, the larger problem that carries it and a small one in reserve. It replaces the list for target."
    )]
    pub(crate) carried: Option<Vec<CarryWrite>>,
    #[serde(default)]
    #[schemars(
        description = "write: for a question of an earlier test, the item it came from and what was changed."
    )]
    pub(crate) traces: Vec<TraceWrite>,
    #[serde(default)]
    #[schemars(description = "read: one file on the map, by path or code, with its items.")]
    pub(crate) file: Option<String>,
    #[serde(default)]
    #[schemars(description = "read: one item, with what it uses and where it is used.")]
    pub(crate) item: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "reach: the items to count for. Left out, every section in scope and every task in the schedule."
    )]
    pub(crate) candidates: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileAdd {
    #[schemars(description = "The file's path inside the study folder.")]
    pub(crate) path: String,
    #[serde(default)]
    #[schemars(
        description = "A short code for the file, such as lec08 or w1-list: lowercase letters, digits and hyphens. Item ids start with it."
    )]
    pub(crate) code: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ItemWrite {
    #[serde(default)]
    #[schemars(description = "An existing item to change. Left out, a new item.")]
    pub(crate) id: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "The file the item is in, by path or code. A file not on the map is added."
    )]
    pub(crate) file: Option<String>,
    #[serde(default)]
    #[schemars(description = "The item this one sits inside, such as a section.")]
    pub(crate) parent: Option<String>,
    #[serde(default)]
    pub(crate) kind: Option<ItemKind>,
    #[serde(default)]
    #[schemars(description = "What the item is, in the material's own words.")]
    pub(crate) title: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "The id's last part, such as 6.63 for an exercise or q2 for a question. Left out, a number."
    )]
    pub(crate) r#ref: Option<String>,
    #[serde(default)]
    #[schemars(description = "One line on what it asks or shows.")]
    pub(crate) note: Option<String>,
    #[serde(default)]
    pub(crate) anchor_start: Option<u32>,
    #[serde(default)]
    pub(crate) anchor_end: Option<u32>,
    #[serde(default)]
    #[schemars(
        description = "foundation, for a step from an earlier course; outline, for an item of your own outline when the folder has no material for it; lesson, for an item found while teaching. Left out: read, or written for a task."
    )]
    pub(crate) origin: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LinkWrite {
    pub(crate) item: String,
    #[schemars(description = "The items it cannot be done or understood without.")]
    pub(crate) uses: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TargetFacts {
    #[serde(default)]
    #[schemars(description = "The test or goal, as the learner and the instructor name it.")]
    pub(crate) label: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "When it is, starting with the date as YYYY-MM-DD, such as 2026-11-21 14:30. Left out for a goal with no date."
    )]
    pub(crate) at: Option<String>,
    #[serde(default)]
    pub(crate) length_minutes: Option<u32>,
    #[serde(default)]
    pub(crate) aids: Option<String>,
    #[serde(default)]
    #[schemars(description = "Hours the learner has before it.")]
    pub(crate) hours: Option<f64>,
    #[serde(default)]
    #[schemars(description = "For a goal with no test: what the learner wants to be able to do.")]
    pub(crate) goal: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "The instructor's own words about the test, each with where and when they were said. Replaces the list."
    )]
    pub(crate) words: Option<Vec<Quote>>,
    #[serde(default)]
    #[schemars(
        description = "When the learner practises, one line each: \"If it is <days> at <HH:MM>, then I ...\", and one \"If I miss ...\". Replaces the list."
    )]
    pub(crate) sessions: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Quote {
    pub(crate) quote: String,
    #[serde(default)]
    pub(crate) source: Option<String>,
    #[serde(default)]
    pub(crate) date: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ScopeState {
    In,
    Cut,
    Confirm,
    Clear,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScopeWrite {
    #[schemars(description = "A file path or code, a folder path ending in /, or an item id.")]
    pub(crate) r#ref: String,
    #[schemars(
        description = "in puts it in scope; cut takes it out for this test, with reason; confirm records that the learner agreed to the cut; clear removes the row."
    )]
    pub(crate) state: ScopeState,
    #[serde(default)]
    #[schemars(description = "For cut: what is cut, by its content, and why.")]
    pub(crate) reason: Option<String>,
    #[serde(default)]
    #[schemars(description = "For cut: true when the learner has already agreed to it.")]
    pub(crate) confirmed: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BlockWrite {
    pub(crate) title: String,
    #[serde(default)]
    #[schemars(description = "The sections of the material this block works, by item id.")]
    pub(crate) sections: Vec<String>,
    #[serde(default)]
    #[schemars(description = "The tasks it sets, in order, by item id.")]
    pub(crate) tasks: Vec<String>,
    #[serde(default)]
    pub(crate) minutes: Option<u32>,
    #[serde(default)]
    #[schemars(
        description = "How the block starts: test (the section's own problems first), worked (a worked solution first), question (one short concrete question), teach, or paper."
    )]
    pub(crate) opens_with: Option<String>,
    #[serde(default)]
    pub(crate) note: Option<String>,
    #[serde(default)]
    #[schemars(description = "The date it was done, as YYYY-MM-DD.")]
    pub(crate) done: Option<String>,
    #[serde(default)]
    #[schemars(description = "Minutes it took, once done.")]
    pub(crate) spent: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BlockDone {
    pub(crate) n: u32,
    #[schemars(description = "The date, as YYYY-MM-DD.")]
    pub(crate) done: String,
    #[serde(default)]
    pub(crate) spent: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CarryWrite {
    pub(crate) idea: String,
    #[schemars(description = "The larger problem that cannot be done without it.")]
    pub(crate) by: String,
    #[serde(default)]
    #[schemars(description = "A small problem on the idea alone, held in case it breaks there.")]
    pub(crate) reserve: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TraceWrite {
    #[schemars(description = "The question of the earlier test, by item id.")]
    pub(crate) question: String,
    #[schemars(description = "The item it came from, such as a listed exercise.")]
    pub(crate) source: String,
    #[schemars(description = "What was changed: the story, the numbers, a part added.")]
    pub(crate) changed: String,
}
