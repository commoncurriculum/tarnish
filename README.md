# tarnish

ProseMirror's document model and transforms (`prosemirror-model` and `prosemirror-transform`)
in Rust, for servers that need to read and change ProseMirror documents without running
JavaScript. For example, an Elixir backend can apply the steps an editor sends.

## What's here

| Path                    | What it is                                                                                                                                      |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/tarnish`        | The library: schemas, nodes, marks, slices, resolved positions, steps, mapping, transforms, and DOM parsing and serializing over a `Dom` trait |
| `elixir/`               | The Elixir package. It takes and returns Erlang terms, with no JSON text in between                                                             |
| `crates/tarnish_elixir` | The Rustler NIF behind the Elixir package                                                                                                       |
| `crates/tarnish-c`      | A shared and static library for any other language: JSON strings in and out, declared in `include/tarnish.h`                                   |
| `crates/tarnish-node`   | A Node bridge, used only to run ProseMirror's own tests against the Rust code. It isn't published                                              |
| `upstream/`             | ProseMirror's repositories, pinned as submodules, for their test suites                                                                         |
| `fixtures/`             | The transforms `prosemirror-transform`'s tests check, recorded from the real package                                                            |

## How it's proven

The proof doesn't depend on anyone reading the Rust. CI checks it:

1. **ProseMirror's own test suites run unedited.** `npm run test:js` runs them against the real
   packages, which shows the suites and the harness are sound. `npm run test:rust` runs the same
   files against tarnish: a Node resolve hook swaps `prosemirror-model` and
   `prosemirror-transform` for the bridge. Both must pass every test. Skipped or focused tests
   fail the run.
2. **The bindings are checked against JavaScript's output.** `npm run test:js` also records each
   transform the upstream suite checks with steps, using the upstream test file's own
   `EMIT_JSON` option: the schema, the starting document, the steps, the resulting document, and
   mapped positions. CI fails if what it records differs from the committed file.
   - The Elixir tests apply every recorded transform and must get the recorded document. They
     also invert the steps and map the positions.
   - The C test does the same, and the JSON the library returns must equal `JSON.stringify`'s
     output byte for byte.

## Elixir

The package builds its NIF from `crates/`, so depend on the whole repository:

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

A schema is built once, and a document is read once and kept in Rust, so applying steps to it
doesn't convert it again. Specs, steps and JSON are maps as Jason decodes ProseMirror's JSON, or
`Jason.OrderedObject`s. Terms cross in Erlang's external term format, which the VM reads and
makes in one call each. Errors come back as `{:error, {kind, message}}`, where the kind names the
class ProseMirror throws; a term Jason couldn't encode, such as a tuple, raises `ArgumentError`.
Give a schema's `"nodes"` and `"marks"` as lists of `{name, spec}` pairs, or as
`Jason.OrderedObject`s, because their order matters and a map doesn't keep it.

Documents nest as deeply as memory allows: a recursion that runs low on a dirty scheduler's
small stack carries on in a new stack segment, so no document takes the VM down.

## C, and other languages through it

`cargo build --release -p tarnish-c` builds `libtarnish_c` (`.so`/`.dylib` and `.a`).
[`include/tarnish.h`](crates/tarnish-c/include/tarnish.h) declares the same operations as the
Elixir package, with schemas and nodes as handles, and JSON strings in and out, read as
`JSON.parse` reads them. Errors are `"Class: message"` strings.

## Running the tests

```sh
git submodule update --init
npm ci
npm run test:js      # ProseMirror's suites against ProseMirror, recording the fixtures
npm run test:rust    # ProseMirror's suites against tarnish
npm run test:c       # the C library against the fixtures
(cd elixir && mix deps.get && mix test)
cargo test           # the library's own tests, among them documents nested far past a small stack
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
```
