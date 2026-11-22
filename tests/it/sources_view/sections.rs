use std::fs;

use pdf_extract::{Document, Object, Stream, content::Content, content::Operation, dictionary};
use serde_json::{Value, json};

use crate::support::TestEnvironment;
use crate::support::mcp::McpClient;

use super::initialize_product;

fn headings(listed: &Value) -> Vec<(String, u64, u64)> {
    listed["sections"]
        .as_array()
        .expect("sections")
        .iter()
        .map(|section| {
            (
                section["heading"].as_str().unwrap().to_owned(),
                section["anchor_start"].as_u64().unwrap(),
                section["anchor_end"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn a_slide_source_lists_every_heading_with_its_lines_without_any_search() {
    let _one_source_test = crate::support::sources::source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").expect("env");
    initialize_product(&environment);
    fs::create_dir_all(environment.workspace.join("slides")).unwrap();
    fs::write(
        environment.workspace.join("slides/lec08.qmd"),
        "---\ntitle: \"Lecture 8\"\n---\n\n# Recurrent Networks\n\n## Parity\n\nbody\n\n## What can RNNs compute?\n\none\n\n## What can RNNs compute?\n\ntwo\n\n# Wrap Up\n\n## Summary\n",
    )
    .unwrap();
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();
    let listed = client.call_value(
        2,
        "sources",
        json!({"action": "sections", "path": "slides/lec08.qmd"}),
    );
    assert_eq!(listed["anchor_kind"], "line", "{listed}");
    assert_eq!(
        headings(&listed),
        [
            ("Recurrent Networks".to_owned(), 5, 6),
            ("Recurrent Networks > Parity".to_owned(), 7, 10),
            (
                "Recurrent Networks > What can RNNs compute?".to_owned(),
                11,
                14
            ),
            (
                "Recurrent Networks > What can RNNs compute?".to_owned(),
                15,
                18
            ),
            ("Wrap Up".to_owned(), 19, 20),
            ("Wrap Up > Summary".to_owned(), 21, 21),
        ],
        "{listed}"
    );
    assert_eq!(listed["untitled_anchors"], 1, "{listed}");
    client.close_cleanly();
}

#[test]
fn a_slide_deck_pdf_names_each_page_by_its_title_and_not_its_running_header() {
    let _one_source_test = crate::support::sources::source_test_guard();
    let environment = TestEnvironment::from_fixture("basic").expect("env");
    initialize_product(&environment);
    fs::write(
        environment.workspace.join("deck.pdf"),
        deck(&[
            Some("Toy Problem"),
            Some("Pivotal quantity"),
            Some("Exercise 8.59"),
            None,
        ]),
    )
    .unwrap();
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();
    let listed = client.call_value(
        2,
        "sources",
        json!({"action": "sections", "path": "deck.pdf"}),
    );
    assert_eq!(listed["anchor_kind"], "page", "{listed}");
    assert_eq!(
        headings(&listed),
        [
            ("Toy Problem".to_owned(), 1, 1),
            ("Pivotal quantity".to_owned(), 2, 2),
            ("Exercise 8.59".to_owned(), 3, 3),
        ],
        "{listed}"
    );
    assert_eq!(listed["untitled_anchors"], 1, "{listed}");
    client.close_cleanly();
}

fn text(size: i64, y: i64, words: &str) -> Vec<Operation> {
    vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), size.into()]),
        Operation::new("Td", vec![72.into(), y.into()]),
        Operation::new("Tj", vec![Object::string_literal(words)]),
        Operation::new("ET", vec![]),
    ]
}

fn deck(titles: &[Option<&str>]) -> Vec<u8> {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document
        .add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica"});
    let resources_id = document.add_object(dictionary! {"Font"=>dictionary! {"F1"=>font_id}});
    let mut pages = Vec::new();
    for (index, title) in titles.iter().enumerate() {
        let mut operations = text(9, 770, "Statistics 101 running header");
        if let Some(title) = title {
            operations.extend(text(22, 700, title));
        }
        operations.extend(text(
            12,
            640,
            &format!("The body of slide {index} starts here"),
        ));
        operations.extend(text(12, 620, "and carries on in the same size below"));
        let content = Content { operations };
        let content_id =
            document.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        pages.push(Object::Reference(document.add_object(dictionary! {
            "Type"=>"Page", "Parent"=>pages_id, "Contents"=>content_id,
            "Resources"=>resources_id, "MediaBox"=>vec![0.into(),0.into(),612.into(),792.into()],
        })));
    }
    document.objects.insert(
        pages_id,
        Object::Dictionary(
            dictionary! {"Type"=>"Pages", "Kids"=>pages, "Count"=>titles.len() as i64},
        ),
    );
    let catalog_id = document.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>pages_id});
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    bytes
}
