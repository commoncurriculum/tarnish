//! A document kept as bytes and read back is the document it was.

use tarnish::json::{self, Value};
use tarnish::{Node, api};
use tarnish_fixtures::{read, records, schemas};

#[test]
fn documents_read_back_from_their_bytes() {
    let fixture = read("transform");
    let schemas = schemas(&fixture);
    for case in records(&fixture, "tests") {
        let schema = &schemas[case["schema"].as_u64().unwrap_or(0) as usize];
        let Ok(start) = Node::from_json(schema, &case["start"]) else {
            continue;
        };
        let mut docs = vec![start.clone()];
        if let Ok(changed) = api::apply_steps(&start, &case["steps"]) {
            docs.push(changed);
        }
        for doc in docs {
            let bytes = doc.to_bytes();
            let again = Node::from_bytes(schema, bytes.as_slice()).expect("the document again");
            assert!(again == doc);
            assert_eq!(again.to_json_string(), doc.to_json_string());
            assert_eq!(
                again.to_bytes(),
                bytes,
                "bytes read back are written the same"
            );
        }
    }
}

#[test]
fn other_bytes_are_refused() {
    let fixture = read("transform");
    let schemas = schemas(&fixture);
    let doc: Value = json::from_str(
        r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"hi"}]}]}"#,
    )
    .unwrap();
    let node = Node::from_json(&schemas[0], &doc).unwrap();
    let bytes = node.to_bytes();
    let refused = |bytes: &[u8], schema| Node::from_bytes(schema, bytes).is_err();
    assert!(refused(&[], &schemas[0]));
    assert!(refused(b"not a document", &schemas[0]));
    assert!(refused(&bytes[..bytes.len() - 1], &schemas[0]));
    let other = schemas
        .iter()
        .find(|schema| schema.fingerprint() != schemas[0].fingerprint());
    assert!(refused(&bytes, other.expect("a different schema")));
}
