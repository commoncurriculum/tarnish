# tarnish

ProseMirror's document model and transforms in Rust: `prosemirror-model` 1.25.11 and
`prosemirror-transform` 1.12.0. A server can read, check, build and change ProseMirror documents,
and apply the steps editors send, without running JavaScript.

- **The whole API.** Schemas, nodes, fragments, marks, slices, resolved positions, content
  expressions, the eight step types, step maps and mappings, every `Transform` operation, and
  `DOMParser` and `DOMSerializer` over a DOM you plug in.
- **Proven by ProseMirror's own tests.** Both packages' test suites run unedited against tarnish:
  309 of 309 model tests and 238 of 238 transform tests pass.
- **Bindings.** Elixir, taking and returning Erlang terms, and C, taking and returning JSON, for
  any language that can call a C library.
- **Safe.** No `unsafe` code, and no depth limit: documents nest as deeply as memory allows.

## Use it

### Rust

```toml
[dependencies]
tarnish = { git = "https://github.com/commoncurriculum/tarnish" }
```

```rust
use tarnish::transform::Transform;
use tarnish::{Node, api, json};

let spec = r#"{"nodes": {"doc": {"content": "paragraph+"}, "paragraph": {"content": "text*"},
                         "text": {}},
               "marks": {"em": {}}}"#;
let schema = api::schema(&json::from_str(spec).expect("JSON"))?;
let doc = r#"{"type": "doc", "content": [
    {"type": "paragraph", "content": [{"type": "text", "text": "Hello"}]}]}"#;
let doc = Node::from_json(&schema, &json::from_str(doc).expect("JSON"))?;

// Apply the steps an editor sent.
let steps = r#"[{"stepType": "replace", "from": 6, "to": 6,
                 "slice": {"content": [{"type": "text", "text": ", world"}]}}]"#;
let doc = api::apply_steps(&doc, &json::from_str(steps).expect("JSON"))?;
assert_eq!(doc.text_content()?.as_str(), Some("Hello, world"));

// Or change it on the server, as a transform.
let em = schema.mark(&schema.mark_type("em").expect("em"), None)?;
let mut tr = Transform::new(doc);
tr.add_mark(1, 6, &em)?;
println!("{}", tr.doc().to_json_string());
```

This example is the crate's doctest, so it compiles and runs in CI.

### HTML, in Rust

`tarnish-html` is a DOM for `DomParser` and `DomSerializer` to read and write HTML with. It
parses HTML with html5ever, as jsdom's parse5 does, and writes it as jsdom's `innerHTML` does.

```toml
[dependencies]
tarnish = { git = "https://github.com/commoncurriculum/tarnish" }
tarnish-html = { git = "https://github.com/commoncurriculum/tarnish" }
```

```rust
use std::collections::HashMap;
use std::sync::Arc;

use tarnish::dom::{
    DomParser, DomSerializer, DomSpec, GetAttrsResult, MarkToDom, NodeToDom, ParseOptions,
    ParseRule, Rule, TagRule,
};
use tarnish::{api, json};
use tarnish_html::{HtmlNode, parse_html, to_html};

let spec = r#"{"nodes": {"doc": {"content": "paragraph+"},
                         "paragraph": {"content": "text*"}, "text": {}},
               "marks": {"link": {"attrs": {"href": {}}}}}"#;
let schema = api::schema(&json::from_str(spec).expect("JSON"))?;

// Each type's parse rules, in schema order: the marks', then the nodes'.
let mut link = TagRule::new("a[href]");
link.get_attrs = Some(Arc::new(|element: &HtmlNode| {
    let href = element.attribute("href").unwrap_or_default();
    Ok(GetAttrsResult::Attrs(json::object!({"href": href})))
}));
let marks = vec![vec![ParseRule::Tag(Rule::new(link))]];
let nodes = vec![vec![], vec![ParseRule::Tag(Rule::new(TagRule::new("p")))], vec![]];
let parser = DomParser::from_schema(schema, marks, nodes)?;

let html = "<p>Read <a href='/docs'>the docs</a>.</p>";
let doc = parse_html(&parser, html, ParseOptions::default())?;

let paragraph: NodeToDom<HtmlNode> = Arc::new(|_| Ok(json::json!(["p", 0]).into()));
let link: MarkToDom<HtmlNode> = Arc::new(|mark, _| {
    let href = mark.attrs().to_map().remove("href").unwrap_or_default();
    Ok(DomSpec::from(json::json!(["a", {"href": href}, 0])))
});
let serializer = DomSerializer::new(
    HashMap::from([("paragraph".to_owned(), paragraph)]),
    HashMap::from([("link".to_owned(), link)]),
);
let html = to_html(&serializer, doc.content())?;
assert_eq!(html, r#"<p>Read <a href="/docs">the docs</a>.</p>"#);
```

This example is `tarnish-html`'s doctest.

- **Rules are closures.** A rule's `getAttrs` gets an `HtmlNode`, which reads the element's
  attributes and inline style. `parseDOM` and `toDOM` aren't read from a spec's JSON.
- **Fragments and documents.** `parse_html` parses HTML as a `<template>`'s content holds it.
  `HtmlDom::parse_document` parses a whole document, whose `body()` a parser can read.
- **Proven against jsdom.** `npm run test:js` records to `fixtures/dom.json` what ProseMirror
  does in jsdom with prosemirror-schema-basic and prosemirror-schema-list: 295 parses of 285
  HTML inputs, as a template's content and as a document, 448 documents written as HTML, 85
  inline styles and 40 DOM output specs. The crate's tests write those schemas' rules and
  `toDOM`s in Rust, and must build the same trees, documents and HTML.
- **Where the trees differ.** The tests print both trees for four inputs, where html5ever
  follows the HTML standard and jsdom doesn't, but for `<isindex>`:
  - text moved out of a table, which jsdom puts after the table instead of before it;
  - elements in a `<select>`, which html5ever keeps, as the standard now does, and jsdom's
    parse5 drops;
  - a CDATA section in MathML's `<mi>`, text to html5ever and a comment to parse5;
  - an end tag past an `<isindex>`, which html5ever still treats as special, as the standard
    did before it dropped `<isindex>`.

### Elixir

```elixir
{:tarnish, git: "https://github.com/commoncurriculum/tarnish", subdir: "elixir"}
```

```elixir
{:ok, schema} = Tarnish.schema(%{"nodes" => [{"doc", %{"content" => "paragraph+"}}, ...], "marks" => [...]})
{:ok, doc} = Tarnish.node_from_json(schema, json)
:ok = Tarnish.check(doc)
{:ok, doc} = Tarnish.apply_steps(doc, steps)
{:ok, inverted} = Tarnish.invert_steps(doc, steps)
json = Tarnish.to_json(doc)
{:ok, pos} = Tarnish.map_position(schema, steps, 5)
```

- **Input.** Specs, documents and steps are ProseMirror's JSON as Jason decodes it. Give a
  schema's `"nodes"` and `"marks"` as lists of `{name, spec}` pairs, or as
  `Jason.OrderedObject`s, since their order matters and a map doesn't keep it.
- **Errors.** Errors are `{:error, {kind, message}}`, where the kind names the class ProseMirror
  throws (`:range_error`, `:replace_error`, …). A term ProseMirror would read but Jason couldn't
  encode, such as a tuple, raises `ArgumentError`.
- **Output.** A document keeps the map it was read from, and `to_json` shares every part of that
  map that is still what ProseMirror writes. After steps, only the nodes they changed are new
  maps.

### C, and other languages through it

`cargo build --release -p tarnish-c` builds `libtarnish_c` as a shared and a static library.
[`include/tarnish.h`](crates/tarnish-c/include/tarnish.h) declares the same operations as the
Elixir package:

- schemas and nodes are handles;
- JSON goes in and out as strings;
- errors are `"Class: message"`;
- a document's JSON is exactly the text `JSON.stringify` writes.

Everything behind the header is Rust; C is only the calling convention, which PHP, Python, Ruby,
Go and Node can all load.

## What maps to what

Names are Rust's: `nodeSize` is `node_size`, `Transform.addMark` is `Transform::add_mark`.

| ProseMirror | tarnish |
| --- | --- |
| `Schema`, `NodeType`, `MarkType`, `NodeSpec`, `MarkSpec`, `AttributeSpec` | `Schema`, `NodeType`, `MarkType`, `SchemaSpec`, `NodeSpec`, `MarkSpec`, `AttributeSpec`; `SchemaSpec::from_json` reads a spec from JSON |
| `Node`, `Fragment`, `Mark`, `Slice`, `ResolvedPos`, `NodeRange`, `ContentMatch` | The same names, in `tarnish` |
| `Node.fromJSON`, `node.toJSON()` | `Node::from_json`, `Node::to_json`, `Node::to_json_string` |
| `Transform` and every operation on it | `transform::Transform` |
| `ReplaceStep`, `ReplaceAroundStep`, `AddMarkStep`, `RemoveMarkStep`, `AddNodeMarkStep`, `RemoveNodeMarkStep`, `AttrStep`, `DocAttrStep` | `transform::Step`, one variant each |
| `StepMap`, `Mapping`, `MapResult` | `transform::StepMap`, `Mapping`, `MapResult` |
| `liftTarget`, `findWrapping`, `canSplit`, `canJoin`, `joinPoint`, `insertPoint`, `dropPoint`, `replaceStep` | The same functions in `transform` |
| `DOMParser`, `DOMSerializer`, `DOMParser.schemaRules` | `dom::DomParser` (`from_schema`), `dom::DomSerializer`, `dom::schema_rules`, over the `dom::Dom` trait |

Positions count UTF-16 units, as they do in the browser, so a step lands where it did there, even
one that splits a surrogate pair.

## What isn't here yet

- **Custom step types.** `Step.jsonID` registers a new kind of step in JavaScript. tarnish's steps
  are ProseMirror's own eight.
- **An HTML parser.** `DomParser` and `DomSerializer` run over the `Dom` trait. Implement it for
  your DOM (html5ever's, for example) to parse or write HTML. The test bridge implements it for
  jsdom.
- **Functions in specs from data.** A spec read from JSON, Elixir or C has no `toDOM`,
  `getAttrs` or `leafText`: those are JavaScript functions. In Rust you give them as closures.
- **Building transforms from Elixir and C.** The bindings apply, invert and map steps. Making new
  steps (`addMark`, `setBlockType`, …) is Rust-only for now.

## How it's proven

The proof doesn't depend on anyone reading the Rust. CI checks it:

1. **ProseMirror's suites.** They run unedited, from `upstream/`, pinned submodules at the tags of
   the npm versions (CI checks the two match).
   - `npm run test:js` runs them against the real packages, which proves the suites and the
     harness.
   - `npm run test:rust` runs the same files against tarnish, through a Node bridge
     (`crates/tarnish-node`, internal) that stands in for the two packages.
   - A skipped or focused test fails the run, so a pass means every test ran.
2. **tarnish's own tests.** The tests in `test/` cover inputs the upstream suites don't, such as
   text that joins across a replaced range. They run the same way, against JavaScript first.
3. **Recorded answers for the bindings.** `npm run test:js` also records fixtures from the real
   packages, and CI fails if they change:
   - every transform the upstream transform suite checks: its schema, starting document, steps,
     resulting document and mapped positions;
   - what `prosemirror-model` does with inputs its tests don't give.

   The Elixir tests apply every recorded transform, invert it and map its positions. The C test
   does the same, and its JSON must equal `JSON.stringify`'s byte for byte.

| Check | Result |
| --- | --- |
| prosemirror-model's suite, against tarnish | 309 passing |
| prosemirror-transform's suite, against tarnish | 238 passing |
| tarnish's own suite, against JavaScript and tarnish | 34 passing |
| Elixir (`mix test`) | 203 tests |
| C (`npm run test:c`) | 148 recorded transforms, the error cases and a 200,000-deep attribute |
| Rust (`cargo test`) | the recorded cases, and a 20,000-deep document through every operation on a 256 KB stack |

## Speed

The bench document has 200 paragraphs with bold text and links: 1,201 nodes, 78 KB of JSON.
`npm run bench` writes it, together with ten steps that each type one character. The table shows
µs per call on one 4-core VM: ProseMirror in Node 22 after its JIT warms up, tarnish from Rust,
and tarnish from Elixir.

| | ProseMirror (Node) | tarnish (Rust) | tarnish (Elixir) |
| --- | --- | --- | --- |
| Read a document from JSON | 282 | 73–82 | 335 |
| Check it | 189 | 24 | 27 |
| Apply 10 steps | 23 | 11 | 24 |
| Write the changed document's JSON | 71 | 225 | 6 |

- **Elixir reading.** Most of the Elixir read time is spent reading Erlang terms. Keeping the
  document between calls, as `%Tarnish.Doc{}` does, skips the read.
- **Elixir writing.** The Elixir `to_json` shares the maps it read, so it writes only what the
  steps changed.
- **Rust writing.** Rust's `to_json` builds every value anew, and is slower than V8 at that.

To reproduce:

```sh
npm run bench
cargo run --release -p tarnish --example bench
(cd elixir && MIX_ENV=test mix run bench/apply_steps.exs)
```

## Design

The contract is ProseMirror's public API. Behind it, tarnish takes whatever shape is fastest.

- **Documents are chunks.** A chunk is an immutable array of little-endian bytes: a header, then
  one section per kind of thing, each an array of fixed-size records linked by `u32` index.
  - The sections hold nodes (24 bytes), each node's list of children (12 bytes), mark sets,
    marks, attribute values and text.
  - A chunk refers into the chunks it imports by slot, never by address, so its bytes are the
    whole of it. Elixir keeps a document as binaries and reads them in place.
  - A step writes the nodes it makes into a new chunk that imports the old one. A chunk that
    nothing else holds is patched in place.
  - Every read is checked, so a bad chunk panics instead of misreading.
  - The format is in [`crates/tarnish/src/chunk/mod.rs`](crates/tarnish/src/chunk/mod.rs).
- **Depth.** Every recursion over nesting goes through `stack::grow`, which continues on a new
  stack segment when the thread's stack runs low. A scheduler's small stack can't overflow.

## Why not oxc, Biome or Yuku

They are fast parsers, but for other languages, and nothing tarnish does is parsing one of those.
tarnish reads two things:

- JSON, with serde_json (and json-event-parser for what serde_json refuses);
- ProseMirror's content expressions, such as `paragraph+ (heading | list)*`, a grammar only
  ProseMirror has.

HTML comes in through the `Dom` trait, from whatever parser the caller uses.

- **Yuku** is a JavaScript and TypeScript compiler written in Zig. Its idea of a tree as flat
  arrays of fixed-size nodes linked by index, instead of a tree of heap objects, is the idea
  behind tarnish's chunks. The code itself parses JavaScript, and calling Zig from Rust would
  take unsafe code.
- **oxc** is a JavaScript and TypeScript toolchain in Rust. Its regular-expression crate parses
  patterns without running them. Its Markdown parser follows micromark.
- **Biome** parses JavaScript, TypeScript, JSON, CSS and GraphQL (and HTML and Markdown for its
  formatter) into lossless syntax trees, where tarnish needs ProseMirror's nodes.

## Where it differs

Where ProseMirror takes input it can't make sense of, tarnish refuses it or keeps what it can
hold:

- **Positions.** A step's position must be a whole number from zero up, its ranges must run
  forwards, and a slice can't be open deeper than its content. ProseMirror takes any number, and
  gives such a range or slice a negative size. tarnish raises the `RangeError` that `fromJSON`
  raises for a position that isn't a number.
- **Depths.** Depth arguments are unsigned. Where ProseMirror takes a negative depth counting
  back from the position's own, pass that depth.
- **Lone surrogates.** A JSON value holds strings as Rust does, which can't hold a lone
  surrogate, so one read or written as a value is U+FFFD.
  - A document written as JSON text keeps it, as `JSON.stringify` does.
  - In Elixir, `to_json` gives U+FFFD, one UTF-16 unit, so positions stay where they were.
- **`default: undefined`.** An attribute whose spec's default is `undefined` is left out.
  ProseMirror keeps its name with no value.
- **Spec properties.** A spec's properties that ProseMirror doesn't read are kept in
  `NodeSpec::extra` and `MarkSpec::extra`.

## Layout

| Path | What it is |
| --- | --- |
| `crates/tarnish` | The library: `model/`, `transform/`, `dom/`, `chunk/` (the document format), `json/` and `js/` (JSON and JavaScript's semantics for it), `api` (what the bindings call) |
| `crates/tarnish-html` | An HTML DOM for `DomParser` and `DomSerializer`: html5ever's parser, jsdom's serialization |
| `elixir/`, `crates/tarnish_elixir` | The Elixir package and the Rustler NIF behind it |
| `crates/tarnish-c` | The C library and its generated header |
| `crates/tarnish-node` | The Node bridge that runs ProseMirror's suites against tarnish. Internal, not published |
| `upstream/` | ProseMirror's repositories, pinned as submodules, for their test suites |
| `harness/` | The suite runner, the fixture recorders, and the bridge and C test builds |
| `test/` | tarnish's own tests, in the upstream suites' style |
| `fixtures/` | What the real packages did, recorded by `npm run test:js` |
| `bench/` | The benchmark document and ProseMirror's timing |

## Running the tests

```sh
git submodule update --init
npm ci
npm run test:js      # ProseMirror's suites against ProseMirror, recording the fixtures
npm run test:rust    # ProseMirror's suites against tarnish
npm run test:c       # the C library against the fixtures
(cd elixir && mix deps.get && mix test)
cargo test
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
```

## License

MIT. tarnish ports ProseMirror, whose MIT notice [`LICENSE`](LICENSE) keeps.
