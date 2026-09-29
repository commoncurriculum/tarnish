# tarnish

ProseMirror's document model and transforms in Rust: `prosemirror-model` 1.25.11 and
`prosemirror-transform` 1.12.0. A server can read, check, build and change ProseMirror documents,
and apply the steps editors send, without running JavaScript.

- **The whole API.** Schemas, nodes, fragments, marks, slices, resolved positions, content
  expressions, the eight step types and step types of your own, step maps and mappings, every
  `Transform` operation, and `DOMParser` and `DOMSerializer` over a DOM you plug in.
- **Proven by ProseMirror's own tests.** Both packages' test suites run unedited against tarnish:
  309 of 309 model tests and 238 of 238 transform tests pass.
- **Bindings.** Elixir, taking and returning Erlang terms, and C, taking and returning JSON, for
  any language that can call a C library.
- **Safe.** No `unsafe` code, and none of an engine's limits: documents nest as deeply, and
  strings grow as long, as memory allows.
- **JavaScript's behavior, as V8 has it.** Where ProseMirror's code leans on JavaScript, such as
  `String()`, `JSON.parse`, or the `TypeError` of reading a property of `undefined`, tarnish does
  what Node does, down to the error's message.

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
parses HTML with html5ever and writes it as the standard's `innerHTML` does. Inline styles are
`tarnish-css`: stylo, Servo's CSS engine, which parses, changes and writes declarations as
Firefox does. JavaScript gets the same DOM from
[our linkedom fork](https://github.com/commoncurriculum/linkedom), whose `element.style` is
`tarnish-css` compiled to WebAssembly.

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
- **`to_html` builds no DOM.** It writes the HTML of the DOM the serializer would build as the
  serializer renders each spec, through `HtmlWriter`, a `dom::Target`. A `toDOM` can give a
  typed spec (`DomSpec::element`, `DomSpec::wrapping`, text, the hole, a value of its node's
  attributes) that borrows from the node, which the writer writes directly; any other spec it
  renders in a DOM of its own. Its tests hold it to the DOM's HTML, and to the DOM's errors, on
  random documents whose specs take every shape.
- **Fragments and documents.** `parse_html` parses HTML as a `<template>`'s content holds it.
  `HtmlDom::parse_document` parses a whole document, whose `body()` a parser can read.
- **Styles ignore quirks mode.** Inline styles parse as in a no-quirks document, in Rust and in
  the fork, even in a document without a doctype. Selectors do follow quirks mode. So
  `width: 10` and `color: f00` are dropped where a browser, in quirks mode, would take them.
- **Proven against the linkedom fork.** `npm run test:js` records to `fixtures/dom.json` what
  ProseMirror does in the fork with prosemirror-schema-basic and prosemirror-schema-list: 297
  parses of 287 HTML inputs, as a template's content and as a document, 450 documents written
  as HTML, 85 inline styles, 42 DOM output specs, and the strings 262 nodes give an attribute
  set to them: a link its `href`, resolved against the document's `<base>`, any other node its
  interface. The crate's tests write those schemas' rules and `toDOM`s in Rust, and must build
  the same trees, documents, HTML and strings. They also check that the fork's `element.style`
  runs the `tarnish-css` they do, by the `engine()` it recorded.
- **Where the trees differ.** The tests print both trees for three inputs, where html5ever
  follows the HTML standard and parse5 8 doesn't, but for `<isindex>`:
  - elements in a `<select>`, which html5ever keeps, as the standard now does, and parse5
    drops;
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

# Change the document on the server, and send editors the steps.
ops = [%{"op" => "addMark", "from" => 1, "to" => 6, "mark" => %{"type" => "em"}}]
{:ok, doc, steps} = Tarnish.transform(doc, ops)

# Its text, to index for search.
{:ok, text} = Tarnish.text_between(doc, 0, 12, "\n")
text = Tarnish.text_content(doc)
```

- **Input.** Specs, documents, steps and ops are ProseMirror's JSON as Jason decodes it. Give a
  schema's `"nodes"` and `"marks"` as lists of `{name, spec}` pairs, or as
  `Jason.OrderedObject`s, since their order matters and a map doesn't keep it.
- **Errors.** Errors are `{:error, {kind, message}}`, where the kind names the class ProseMirror
  throws (`:range_error`, `:replace_error`, …). A term ProseMirror would read but Jason couldn't
  encode, such as a tuple, raises `ArgumentError`.
- **Output.** A document keeps the map it was read from, and `to_json` shares every part of that
  map that is still what ProseMirror writes. After steps or ops, only the nodes they changed are
  new maps. In text, a lone surrogate, where a position splits a pair, is U+FFFD.
- **In your own NIF.** `tarnish-nif` is the base of any NIF on tarnish, the package's own
  (`tarnish_elixir`) among them. It holds the terms read and written as JSON, the budgets that
  keep a call on the caller's scheduler or send it to a dirty one, a thread pool for batches, and
  tarnish's functions. An application with a NIF of its own builds it on `tarnish-nif`, so one
  library loads and every call reads and writes terms the same way. Its `rustler::init!`
  registers tarnish's functions with its own, and its load hook starts the pool: pass
  `tarnish_nif::load`, or call it from a hook of your own, with the count of threads as the load
  info. The module that loads it declares tarnish's functions with `use Tarnish.NIF`, and
  `config :tarnish, native: MyApp.Native` has `Tarnish` call it. `Tarnish.Native` is then
  neither built nor loaded.

  ```toml
  tarnish-nif = { git = "https://github.com/commoncurriculum/tarnish" }
  ```

  ```rust
  rustler::init!("Elixir.MyApp.Native", load = tarnish_nif::load);
  ```

### C, and other languages through it

`cargo build --release -p tarnish-c` builds `libtarnish_c` as a shared and a static library.
[`include/tarnish.h`](crates/tarnish-c/include/tarnish.h) declares the same operations as the
Elixir package:

- schemas and nodes are handles;
- JSON goes in and out as strings;
- errors are `"Class: message"`;
- a document's JSON is exactly the text `JSON.stringify` writes;
- `tarnish_transform` gives the changed document, and the steps' JSON through `steps_json`;
- `tarnish_text_between` and `tarnish_text_content` give text as UTF-8, with U+FFFD for a lone
  surrogate.

Everything behind the header is Rust; C is only the calling convention, which PHP, Python, Ruby,
Go and Node can all load.

### Changing documents from Elixir and C

`Tarnish.transform` and `tarnish_transform` take a list of ops and apply them in order to one
`Transform`. An op names a `Transform` method in `"op"` and gives its arguments by the names
ProseMirror gives them:

```json
[{"op": "insert", "pos": 1, "content": {"type": "text", "text": "Hello "}},
 {"op": "addMark", "from": 1, "to": 6, "mark": {"type": "strong"}},
 {"op": "setBlockType", "from": 1, "type": "heading", "attrs": {"level": 2}},
 {"op": "wrap", "from": 1, "to": 1, "nodeType": "blockquote"}]
```

- **Methods.** `replace`, `replaceWith`, `delete`, `insert`, `replaceRange`,
  `replaceRangeWith`, `deleteRange`, `addMark`, `removeMark`, `addNodeMark`, `removeNodeMark`,
  `setNodeMarkup`, `setNodeAttribute`, `setDocAttribute`, `setBlockType`, `lift`, `wrap`,
  `join`, `split`, `clearIncompatible`, and `step` and `maybeStep`, which take a step's JSON.
- **Arguments.** Nodes, slices and marks are their JSON, and a fragment is an array of nodes
  or one node. Node and mark types are names. `removeMark` and `removeNodeMark` take a mark or
  a mark type's name, and `removeMark` with neither removes every mark. What JSON can't hold,
  `setBlockType`'s function for `attrs` and `clearIncompatible`'s `match`, isn't taken.
- **Ranges.** For the `NodeRange` that `lift` and `wrap` take, an op gives `from`, `to` and an
  optional `depth`; without a depth the range is `$from.blockRange($to)`. Without a `target`,
  `lift` lifts to `liftTarget`'s. Without `wrappers`, `wrap` wraps in `nodeType`, with `attrs`,
  as `findWrapping` finds. When there's no range, target or wrapping, the op fails with a
  `RangeError` saying so.
- **Positions.** Positions and depths are whole numbers, and a range's `to` can't come before its
  `from`. An argument that is missing or of the wrong kind is a `RangeError` naming it.
- **Errors.** An op that fails fails the call with the error ProseMirror throws.

[`harness/record-ops.mjs`](harness/record-ops.mjs) reads ops the same way against the real
`Transform`, and records what a few hundred op lists give, every method among them, for the
Rust, Elixir and C tests to reproduce.

## What maps to what

Names are Rust's: `nodeSize` is `node_size`, `Transform.addMark` is `Transform::add_mark`.

| ProseMirror | tarnish |
| --- | --- |
| `Schema`, `NodeType`, `MarkType`, `NodeSpec`, `MarkSpec`, `AttributeSpec` | `Schema`, `NodeType`, `MarkType`, `SchemaSpec`, `NodeSpec`, `MarkSpec`, `AttributeSpec`; `SchemaSpec::from_json` reads a spec from JSON |
| `Node`, `Fragment`, `Mark`, `Slice`, `ResolvedPos`, `NodeRange`, `ContentMatch` | The same names, in `tarnish` |
| `Node.fromJSON`, `node.toJSON()` | `Node::from_json`, `Node::to_json`, `Node::to_json_string` |
| `node.textBetween`, `node.textContent` | `Node::text_between`, `Node::text_content`; `api::text_between`, `api::text_content` for the bindings |
| `Transform` and every operation on it | `transform::Transform`; `api::transform` applies ops, from JSON, for the bindings |
| `ReplaceStep`, `ReplaceAroundStep`, `AddMarkStep`, `RemoveMarkStep`, `AddNodeMarkStep`, `RemoveNodeMarkStep`, `AttrStep`, `DocAttrStep` | `transform::Step`, one variant each |
| A `Step` subclass, `Step.jsonID`, `step instanceof MyStep` | A `transform::CustomStep`, `transform::register_step`, `Step::custom::<MyStep>()` |
| `StepMap`, `Mapping`, `MapResult` | `transform::StepMap`, `Mapping`, `MapResult` |
| `liftTarget`, `findWrapping`, `canSplit`, `canJoin`, `joinPoint`, `insertPoint`, `dropPoint`, `replaceStep` | The same functions in `transform` |
| `DOMParser`, `DOMSerializer`, `DOMParser.schemaRules` | `dom::DomParser` (`from_schema`), `dom::DomSerializer`, `dom::schema_rules`, over the `dom::Dom` trait; `DomSerializer::write_fragment` renders to any `dom::Target` |

Positions count UTF-16 units, as they do in the browser, so a step lands where it did there, even
one that splits a surrogate pair.

## Scope

tarnish is ProseMirror's model and transform layer: what a server needs to hold documents and
change them.

- Read, check, compare and write documents in ProseMirror's JSON, and read their text.
- Apply the steps editors send, of ProseMirror's types and of an application's own.
- Invert, map, merge and rebase steps, and map positions through them, as a collaboration
  authority does.
- Change documents on the server with every `Transform` operation, from Rust, and from Elixir and
  C as ops.
- Parse and serialize with `DOMParser` and `DOMSerializer`, over the DOM you plug in, as
  ProseMirror takes the browser's or jsdom's. `tarnish-html` is one, which builds the trees our
  linkedom fork does; the test bridge plugs in the fork itself.

The editor is not part of it: `prosemirror-state` (editor state, selections, plugins),
`prosemirror-view`, commands, keymaps, input rules and history run in the browser.

A schema's functions (`toDOM`, `getAttrs`, `leafText`) and a custom step's methods are code,
which JSON can't carry. Rust gives them as closures and `CustomStep` implementations; a schema
read from JSON, Elixir or C has none, and the bindings' `text_between` takes the text for leaves
as a string.

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
   - what `prosemirror-model` does with inputs its tests don't give;
   - a step type defined with `Step.jsonID`: applied among ProseMirror's own steps, inverted,
     mapped, merged, and its errors;
   - 273 lists of `Transform` operations, 96 of them failing, and `textBetween` and
     `textContent`, as the bindings take them.

   The Elixir tests apply every recorded transform, invert it and map its positions, and run every
   recorded op list and text read. The C test does the same, and its JSON must equal
   `JSON.stringify`'s byte for byte. The Rust tests define the same custom step in Rust and expect
   what JavaScript recorded.

| Check | Result |
| --- | --- |
| prosemirror-model's suite, against tarnish | 309 passing |
| prosemirror-transform's suite, against tarnish | 238 passing |
| tarnish's own suite, against JavaScript and tarnish | 38 passing |
| Elixir (`mix test`) | 516 tests |
| C (`npm run test:c`) | 148 recorded transforms, 308 op lists and texts, the error cases and a 200,000-deep attribute |
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

HTML comes in through the `Dom` trait. `tarnish-html` parses it with html5ever, which follows the
HTML standard, and holds inline styles in stylo, Servo's CSS engine. oxc and Yuku have no HTML
parser, and Biome's builds a lossless syntax tree for its formatter and isn't published as a
crate.

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
| `crates/tarnish` | The library: `model/`, `transform/`, `dom/`, `chunk/` (the document format), `api` (what the bindings call). `tarnish::js` is `tarnish-js` |
| `crates/tarnish-html` | An HTML DOM for `DomParser` and `DomSerializer`: html5ever's parser, the standard's serialization |
| `crates/tarnish-css`, `crates/tarnish-css-wasm` | Inline styles on stylo, and the same as WebAssembly, which `harness/css-wasm.mjs` writes into the linkedom fork |
| `elixir/`, `crates/tarnish_elixir` | The Elixir package and the Rustler NIF behind it |
| `crates/tarnish-nif` | The base of a NIF on tarnish: terms as JSON, budgets, a batch pool, and tarnish's functions |
| `crates/tarnish-c` | The C library and its generated header |
| `crates/tarnish-js` | JavaScript's values and built-ins as V8 runs them, which every crate here builds on: JSON values and `JSON`, strings as UTF-16, numbers, conversions, errors and `sort`, and as features `RegExp` on regress and `localeCompare` on ICU |
| `crates/tarnish-markdown` | marked 17.0.6's lexer and marked-more-lists 1.0.1's list tokenizer, on `tarnish-js`. `tools/marked_rules.ts` writes marked's rules into `src/marked/rules.rs` |
| `vendor/regress` | regress 0.12.0, the `RegExp` engine of `tarnish-js`, with its patches marked in the source |
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

Building `tarnish-css` needs Python 3, which stylo's build script runs. Writing it into the
linkedom fork (`node harness/css-wasm.mjs <checkout>`) also needs the `wasm32-unknown-unknown`
target and the `wasm-bindgen-cli` version `tarnish-css-wasm` pins; npx fetches binaryen's
`wasm-opt`. It writes `esm/shared/css/`: the module as `engine.wasm`, its glue as `engine.js`,
and `THIRD-PARTY.md`, the crates it's compiled from, with their licences, texts and sources.

## License

MIT. tarnish ports ProseMirror, whose MIT notice [`LICENSE`](LICENSE) keeps. `tarnish-css`
builds on stylo, which is MPL-2.0.
