//! `magnesium searchable` tells a study window which of the files a learner
//! added the tutor's source search will read, and why not for the others. It
//! runs the same rules and the same PDF reader that search does.
use crate::support::{TestEnvironment, v2};
use pdf_extract::{Document, Object, Stream, content::Content, content::Operation, dictionary};
use serde_json::Value;
use std::fs;

/// A one-page PDF. With text it has a text layer; with `None` the page is
/// blank, which is what a scan looks like to a text reader.
fn pdf(text: Option<&str>) -> Vec<u8> {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let resources_id =
        document.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });
    let operations = match text {
        Some(text) => vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 18.into()]),
            Operation::new("Td", vec![72.into(), 720.into()]),
            Operation::new("Tj", vec![Object::string_literal(text)]),
            Operation::new("ET", vec![]),
        ],
        None => vec![],
    };
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        Content { operations }.encode().expect("encode PDF content"),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::Reference(page_id)], "Count" => 1,
        }),
    );
    let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("serialize PDF");
    bytes
}

fn searchable(e: &TestEnvironment, paths: &[&str]) -> Vec<Value> {
    let output = e
        .command()
        .arg("searchable")
        .arg("--json")
        .args(paths)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    value["files"].as_array().unwrap().clone()
}

fn add(e: &TestEnvironment, name: &str, bytes: &[u8]) {
    let path = e.workspace.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

#[test]
fn a_pdf_with_text_is_read_a_scan_is_not_and_a_word_in_a_name_is_named() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    add(&e, "Study/stats/week4.pdf", &pdf(Some("Bayes rule prior")));
    add(&e, "Study/stats/scan.pdf", &pdf(None));
    add(
        &e,
        "Study/stats/answer-key.pdf",
        &pdf(Some("Bayes rule prior")),
    );
    add(&e, "Study/stats/notes.txt", b"posterior");
    add(&e, "Study/stats/slides.pptx", b"x");

    let files = searchable(
        &e,
        &[
            "Study/stats/week4.pdf",
            "Study/stats/scan.pdf",
            "Study/stats/answer-key.pdf",
            "Study/stats/notes.txt",
            "Study/stats/slides.pptx",
            "Study/stats/missing.pdf",
            "../outside.pdf",
        ],
    );
    let reasons: Vec<(bool, Option<&str>)> = files
        .iter()
        .map(|f| (f["searchable"].as_bool().unwrap(), f["reason"].as_str()))
        .collect();
    assert_eq!(
        reasons,
        [
            (true, None),
            (false, Some("scanned")),
            (false, Some("name_word")),
            (true, None),
            (false, Some("unsupported_type")),
            (false, Some("missing")),
            (false, Some("outside")),
        ]
    );
    assert_eq!(files[0]["pages"], 1);
    assert_eq!(files[1]["pages_without_text"], 1);
    assert_eq!(files[2]["word"], "key");
    assert!(
        files[1]["detail"]
            .as_str()
            .unwrap()
            .contains("cannot be searched"),
        "{}",
        files[1]
    );
}

#[test]
fn a_word_in_the_name_leaves_a_text_file_out() {
    // The command reports the rules search runs on, not rules of its own.
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    add(
        &e,
        "Study/stats/notes.txt",
        b"the posterior is proportional to prior times likelihood",
    );
    add(
        &e,
        "Study/stats/tokens.txt",
        b"the posterior is proportional to prior times likelihood",
    );
    let files = searchable(&e, &["Study/stats/notes.txt", "Study/stats/tokens.txt"]);
    assert_eq!(files[0]["searchable"], true);
    assert_eq!(files[1]["searchable"], false);
    assert_eq!(files[1]["word"], "tokens");
}

#[test]
fn it_changes_nothing_in_the_study_folder() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    add(&e, "Study/stats/week4.pdf", &pdf(Some("Bayes rule prior")));
    let listing = |e: &TestEnvironment| {
        let mut names = Vec::new();
        let mut stack = vec![e.workspace.clone()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let modified = fs::metadata(&path).unwrap().modified().unwrap();
                    names.push((path, modified));
                }
            }
        }
        names.sort();
        names
    };
    let before = listing(&e);
    let _ = searchable(&e, &["Study/stats/week4.pdf"]);
    assert_eq!(before, listing(&e));
}

#[test]
fn text_form_says_what_search_cannot_read() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    add(&e, "Study/stats/scan.pdf", &pdf(None));
    let output = e
        .command()
        .args(["searchable", "Study/stats/scan.pdf"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Study/stats/scan.pdf: left out of search. It is a scan with no text, so its text cannot be searched.\n"
    );
}

#[test]
fn a_folder_not_set_up_yet_is_judged_by_the_rules_it_will_have() {
    // A new folder's first step is adding material, before any lesson sets it up.
    let e = TestEnvironment::from_fixture("basic").unwrap();
    let bare = e.root.join("bare");
    fs::create_dir_all(bare.join("Study/materials")).unwrap();
    fs::write(bare.join("Study/materials/scan.pdf"), pdf(None)).unwrap();
    fs::write(bare.join("Study/materials/notes.md"), "Bayes").unwrap();
    fs::write(bare.join("Study/materials/api key.md"), "Bayes").unwrap();
    let output = e
        .command_in(&bare)
        .args([
            "searchable",
            "--json",
            "--",
            "Study/materials/scan.pdf",
            "Study/materials/notes.md",
            "Study/materials/api key.md",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let reasons: Vec<Option<&str>> = value["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["reason"].as_str())
        .collect();
    assert_eq!(reasons, [Some("scanned"), None, Some("name_word")]);
    assert!(!bare.join(".magnesium").exists(), "it set nothing up");
}
