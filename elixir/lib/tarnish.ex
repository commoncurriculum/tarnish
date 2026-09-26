defmodule Tarnish do
  @moduledoc """
  ProseMirror's document model and transforms, run in Rust.

  Documents and steps are ProseMirror's JSON, as Jason decodes it: maps with string keys.
  A schema is built once from its spec and passed to each call.

  Errors come back as `{:error, {kind, message}}`. The kind names the class ProseMirror throws:

    * `:range_error`
    * `:syntax_error`
    * `:replace_error`
    * `:transform_error`
    * `:js_error`, for a plain `Error`
  """

  alias Tarnish.Native

  @type schema :: reference()
  @type json :: map() | list() | String.t() | number() | boolean() | nil
  @type error :: {:error, {atom(), String.t()}}

  @doc """
  Builds a schema from its spec.

  The spec is a map of `"nodes"`, `"marks"` and `"topNode"`, as ProseMirror's `SchemaSpec`.
  The order of the node and mark types matters: it decides which type comes first in a group,
  and how marks sort. So give `"nodes"` and `"marks"` as lists of `{name, spec}` pairs, which
  keep their order where a map doesn't.
  """
  @spec schema(map()) :: {:ok, schema()} | error()
  def schema(spec), do: Native.schema(spec)

  @doc "Checks that a document conforms to the schema."
  @spec check(schema(), json()) :: :ok | error()
  def check(schema, doc), do: Native.check(schema, doc)

  @doc "Applies steps to a document, in order, and gives the changed document."
  @spec apply_steps(schema(), json(), [json()]) :: {:ok, json()} | error()
  def apply_steps(schema, doc, steps), do: Native.apply_steps(schema, doc, steps)

  @doc "The steps that undo `steps`, applied to `doc`, last first."
  @spec invert_steps(schema(), json(), [json()]) :: {:ok, [json()]} | error()
  def invert_steps(schema, doc, steps), do: Native.invert_steps(schema, doc, steps)

  @doc """
  Maps a position through the changes the steps make.

  With `assoc` below zero, a position where content is inserted stays before it; otherwise it
  moves after it.
  """
  @spec map_position(schema(), [json()], non_neg_integer(), integer()) ::
          {:ok, non_neg_integer()} | error()
  def map_position(schema, steps, pos, assoc \\ 1),
    do: Native.map_position(schema, steps, pos, assoc)
end
