use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Instant;

use pdf_extract::{Document, Object, Stream, content::Content, content::Operation, dictionary};
use serde_json::Value;
use support::test_binary;
use tree_sitter::Parser;

#[test]
fn selected_native_content_dependencies_work_without_sidecars() {
    assert_parses(
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "function twice(value: number): number { return value * 2; }",
        "function_declaration",
    );
    assert_parses(
        tree_sitter_javascript::LANGUAGE.into(),
        "export const answer = () => 42;",
        "export_statement",
    );
    assert_parses(
        tree_sitter_python::LANGUAGE.into(),
        "def twice(value):\n    return value * 2\n",
        "function_definition",
    );

    let pdf = readable_pdf_fixture();
    let started = Instant::now();
    let mut pages = Vec::new();
    for _ in 0..100 {
        pages = pdf_extract::extract_text_from_mem_by_pages(&pdf)
            .expect("extract readable PDF text by page");
    }
    eprintln!(
        "pdf-extract 0.12.0: 100 iterations, {} input bytes, {:?}",
        pdf.len(),
        started.elapsed()
    );
    assert_eq!(pages.len(), 2);
    assert!(pages[0].contains("Alpha page anchor"));
    assert!(pages[1].contains("Beta page anchor"));

    let runtime_pages = runtime_pdf_pages(&pdf);
    assert!(runtime_pages[0].contains("Alpha page anchor"));
    assert!(runtime_pages[1].contains("Beta page anchor"));
}

fn runtime_pdf_pages(pdf: &[u8]) -> Vec<String> {
    let mut child = Command::new(test_binary())
        .args(["_internal", "extract-pdf"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start release PDF extractor mode");
    child
        .stdin
        .take()
        .expect("piped PDF input")
        .write_all(pdf)
        .expect("write PDF input");
    let output = child.wait_with_output().expect("wait for PDF extractor");
    assert!(
        output.status.success(),
        "PDF extractor failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let marker = b"MAGNESIUM_PDF_V2 ";
    assert!(output.stdout.starts_with(marker));
    let framed = &output.stdout[marker.len()..];
    let newline = framed
        .iter()
        .position(|byte| *byte == b'\n')
        .expect("PDF frame length terminator");
    let length: usize = std::str::from_utf8(&framed[..newline])
        .expect("PDF frame length UTF-8")
        .parse()
        .expect("PDF frame length integer");
    let payload = &framed[newline + 1..];
    assert_eq!(payload.len(), length, "complete PDF frame");
    let reply: Value = serde_json::from_slice(payload).expect("PDF frame JSON");
    assert_eq!(reply["outcome"], "pages");
    assert_eq!(reply.as_object().expect("PDF reply").len(), 2);
    reply["pages"]
        .as_array()
        .expect("PDF pages")
        .iter()
        .map(|page| page.as_str().expect("PDF page text").to_owned())
        .collect()
}

fn assert_parses(language: tree_sitter::Language, source: &str, expected_kind: &str) {
    let mut parser = Parser::new();
    parser.set_language(&language).expect("load grammar");
    let tree = parser.parse(source, None).expect("parse source");
    assert!(!tree.root_node().has_error());
    assert!(
        tree.root_node()
            .named_children(&mut tree.walk())
            .any(|node| node.kind() == expected_kind),
        "parse tree omitted {expected_kind}: {}",
        tree.root_node().to_sexp()
    );
}

fn readable_pdf_fixture() -> Vec<u8> {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = document.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });

    let page_ids = ["Alpha page anchor", "Beta page anchor"].map(|text| {
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 18.into()]),
                Operation::new("Td", vec![72.into(), 720.into()]),
                Operation::new("Tj", vec![Object::string_literal(text)]),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id = document.add_object(Stream::new(
            dictionary! {},
            content.encode().expect("encode PDF text stream"),
        ));
        document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        })
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids.map(Object::Reference).to_vec(),
            "Count" => 2,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);

    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("serialize PDF fixture");
    bytes
}
