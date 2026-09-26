# tarnish

ProseMirror's document model and transforms (`prosemirror-model` and `prosemirror-transform`)
in Rust, for servers that need to read and change ProseMirror documents without running
JavaScript. For example, an Elixir backend can apply the steps an editor sends.

## What's here

| Path                    | What it is                                                                                                                                      |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/tarnish`        | The library: schemas, nodes, marks, slices, resolved positions, steps, mapping, transforms, and DOM parsing and serializing over a `Dom` trait |
| `elixir/`               | The Elixir package. It takes and returns Erlang terms, with no JSON in between                                                                  |
| `crates/tarnish_elixir` | The Rustler NIF behind the Elixir package                                                                                                       |
| `crates/tarnish-c`      | A shared and static library for any other language: JSON strings in and out, declared in `include/tarnish.h`                                   |
| `crates/tarnish-node`   | A Node bridge, used only to run ProseMirror's own tests against the Rust code. It isn't published                                              |
| `upstream/`             | ProseMirror's repositories, pinned as submodules, for their test suites                                                                         |
| `fixtures/`             | Every transform `prosemirror-transform`'s tests make, recorded from the real package                                                            |

## How it's proven

The proof doesn't depend on anyone reading the Rust. CI checks it:

1. **ProseMirror's own test suites run unedited.** `npm run test:js` runs them against the real
   packages, which shows the suites and the harness are sound. `npm run test:rust` runs the same
   files against tarnish: a Node resolve hook swaps `prosemirror-model` and
   `prosemirror-transform` for the bridge. Both must pass every test. Skipped or focused tests
   fail the run.
2. **The bindings are checked against JavaScript's output.** `npm run fixtures` records every
   transform the upstream suite makes, using the upstream test file's own `EMIT_JSON` option:
   the schema, the starting document, the steps, the resulting document, and mapped positions.
   CI re-records the fixtures and fails if they differ from the committed file.
   - The Elixir tests apply every recorded transform and must get the recorded document. They
     also invert the steps and map the positions.
   - The C test does the same, and the JSON the library returns must equal `JSON.stringify`'s
     output byte for byte.

## Elixir

```elixir
{:ok, schema} = Tarnish.schema(%{"nodes" => [{"doc", %{"content" => "paragraph+"}}, ...], "marks" => [...]})
{:ok, doc} = Tarnish.apply_steps(schema, doc, steps)
{:ok, inverted} = Tarnish.invert_steps(schema, doc, steps)
{:ok, pos} = Tarnish.map_position(schema, steps, 5)
:ok = Tarnish.check(schema, doc)
```

Documents and steps are maps as Jason decodes ProseMirror's JSON. Errors come back as
`{:error, {kind, message}}`, where the kind names the class ProseMirror throws. Give a schema's
`"nodes"` and `"marks"` as lists of `{name, spec}` pairs, because their order matters and a map
doesn't keep it.

## C, and other languages through it

`cargo build --release -p tarnish-c` builds `libtarnish_c` (`.so`/`.dylib`/`.dll` and `.a`).
[`include/tarnish.h`](crates/tarnish-c/include/tarnish.h) declares the same five operations,
taking and returning JSON strings. Errors are `"Class: message"` strings. Free every string
the library returns with `tarnish_free`.

## Running the tests

```sh
git submodule update --init
npm ci
npm run test:js      # ProseMirror's suites against ProseMirror
npm run test:rust    # ProseMirror's suites against tarnish
npm run test:c       # the C library against the fixtures
(cd elixir && mix deps.get && mix test)
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
```
