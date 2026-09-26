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
| `test/`                 | tarnish's own tests, in the upstream suites' style, of inputs those suites don't give                                                           |
| `fixtures/`             | What the real packages do: the transforms `prosemirror-transform`'s tests check, and `prosemirror-model` on inputs its tests don't give it     |

## How it's proven

The proof doesn't depend on anyone reading the Rust. CI checks it:

1. **ProseMirror's own test suites run unedited.** `npm run test:js` runs them against the real
   packages, which shows the suites and the harness are sound. `npm run test:rust` runs the same
   files against tarnish: a Node resolve hook swaps `prosemirror-model` and
   `prosemirror-transform` for the bridge. Both must pass every test. Skipped or focused tests
   fail the run. The tests in `test/` run the same way against both, so each is proven against
   JavaScript before it checks tarnish.
2. **The bindings are checked against JavaScript's output.** `npm run test:js` also records each
   transform the upstream suite checks with steps, using the upstream test file's own
   `EMIT_JSON` option: the schema, the starting document, the steps, the resulting document, and
   mapped positions. It also records what `prosemirror-model` does with inputs its tests don't
   give it, such as attributes that aren't objects, and types that no content fills. CI fails if
   what it records differs from the committed files.
   - The Elixir tests apply every recorded transform and must get the recorded document. They
     also invert the steps and map the positions.
   - The C test does the same, and the JSON the library returns must equal `JSON.stringify`'s
     output byte for byte.
   - The Rust and Elixir tests read the recorded nodes and must get the same nodes or errors.

## Where it differs

Where ProseMirror takes input it can't make sense of, tarnish refuses it or gives what it can
hold:

- A step's position must be a whole number from zero up, its ranges must run forwards, and a
  slice can't be open deeper than its content. ProseMirror takes any number, and gives a
  backwards range or such a slice a negative size. tarnish raises the `RangeError` its
  `fromJSON` raises for a position that isn't a number.
- A JSON value holds strings as Rust does, which can't hold a lone surrogate, so one read or
  written as a value is U+FFFD. A document written as JSON text keeps it.
- An attribute whose spec's default is `undefined` is left out. ProseMirror keeps its name with
  no value, so `toJSON` writes `"attrs": {}` and `hasMarkup` sees the name.

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
`Jason.OrderedObject`s. Rust reads the terms it's given in place, and makes the terms it answers
from Erlang's external term format, which the VM makes a whole term from in one call. Errors come
back as `{:error, {kind, message}}`, where the kind names the class ProseMirror throws. A part of
a term that ProseMirror reads and Jason couldn't encode, such as a tuple, raises
`ArgumentError`; a part it ignores, such as a key a node doesn't have, isn't read. Give a
schema's `"nodes"` and `"marks"` as lists of `{name, spec}` pairs, or as `Jason.OrderedObject`s,
because their order matters and a map doesn't keep it.

A call runs on a normal scheduler when it has no more than about a millisecond's work, and on a
dirty one otherwise. A dirty scheduler takes a few microseconds to hand a call to, which is most
of the time a small document takes.

Documents nest as deeply as memory allows: a recursion that runs low on a dirty scheduler's
small stack carries on in a new stack segment, so no document takes the VM down.

A step that splits a surrogate pair, as a browser's can, leaves a lone surrogate in the text.
A binary holds UTF-8, which can't, so `to_json` gives U+FFFD for it. That keeps every position
where it was: both are one UTF-16 unit.

## C, and other languages through it

`cargo build --release -p tarnish-c` builds `libtarnish_c` (`.so`/`.dylib` and `.a`).
[`include/tarnish.h`](crates/tarnish-c/include/tarnish.h) declares the same operations as the
Elixir package, with schemas and nodes as handles, and JSON strings in and out, read as
`JSON.parse` reads them. Errors are `"Class: message"` strings. A document's JSON is
`JSON.stringify`'s text, a lone surrogate included. Other JSON is read and written as Rust
strings, which can't hold one, so a lone surrogate in a step's slice becomes U+FFFD.

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
