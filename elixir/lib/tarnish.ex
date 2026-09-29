defmodule Tarnish do
  @moduledoc """
  ProseMirror, run in Rust, in modules named as ProseMirror's JavaScript names them:

  | JavaScript | Elixir |
  | --- | --- |
  | `new Schema(spec)` | `Tarnish.Schema.new/1` |
  | `Node.fromJSON`, `node.toJSON()`, `node.check()`, `node.textBetween`, `node.textContent` | `Tarnish.Node` |
  | `step.apply(doc)`, `step.invert(doc)` | `Tarnish.Step` |
  | `new Mapping(maps).map(pos, assoc)` | `Tarnish.Mapping.map/4` |
  | `new Transform(doc)`, its methods, `tr.doc` and `tr.steps` | `Tarnish.Transform.new/2` |

  A schema is built once from its spec, and kept in Rust. A node is read once from its JSON,
  into binaries that Rust reads in place, so applying steps to a document doesn't convert it
  again, and a changed document is a binary more, holding only what the steps changed. Specs,
  steps and JSON are ProseMirror's JSON, as Jason decodes it: maps with string keys, or
  `Jason.OrderedObject`s where order matters.

  A node keeps the JSON it was read from. Its own JSON shares every part of that which is what
  ProseMirror writes for a node it still has, so after steps only the nodes they changed are
  made anew.

  Errors come back as `{:error, {kind, message}}`. The kind names the class ProseMirror throws:

    * `:range_error`
    * `:syntax_error`
    * `:replace_error`
    * `:transform_error`
    * `:type_error`
    * `:js_error`, for a plain `Error`

  A part of a term that ProseMirror reads and Jason couldn't encode, such as a tuple, raises
  `ArgumentError`. A part it ignores, such as a key a node doesn't have, isn't read.

  `Tarnish.Bridge` converts documents between Markdown, HTML and JSON with an application's own
  conversions, in its NIF or in Node workers.
  """

  @type json :: map() | list() | String.t() | number() | boolean() | nil
  @type error :: {:error, {atom(), String.t()}}
end
