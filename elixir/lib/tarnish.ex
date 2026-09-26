defmodule Tarnish do
  @moduledoc """
  ProseMirror's document model and transforms, run in Rust.

  A schema is built once from its spec, and a document is read once from its JSON. Both are
  kept in Rust, so applying steps to a document doesn't convert it again. Specs, steps and
  JSON are ProseMirror's JSON, as Jason decodes it: maps with string keys, or
  `Jason.OrderedObject`s where order matters.

  Errors come back as `{:error, {kind, message}}`. The kind names the class ProseMirror throws:

    * `:range_error`
    * `:syntax_error`
    * `:replace_error`
    * `:transform_error`
    * `:js_error`, for a plain `Error`

  A term Jason couldn't encode, such as a tuple, raises `ArgumentError`.
  """

  alias Tarnish.Native

  @opaque schema :: reference()
  @opaque doc :: reference()
  @type json :: map() | list() | String.t() | number() | boolean() | nil
  @type error :: {:error, {atom(), String.t()}}

  @doc """
  Builds a schema from its spec: a map of `"nodes"`, `"marks"` and `"topNode"`, as
  ProseMirror's `SchemaSpec`.

  The order of the node and mark types matters: it decides which type comes first in a group,
  and how marks sort. So `"nodes"` and `"marks"` are lists of `{name, spec}` pairs, or
  `Jason.OrderedObject`s, which keep their order where a map doesn't.
  """
  @spec schema(map()) :: {:ok, schema()} | error()
  def schema(spec) do
    spec
    |> Map.new(fn {key, value} -> {to_string(key), value} end)
    |> in_order("nodes")
    |> in_order("marks")
    |> Native.schema()
  end

  defp in_order(spec, key) do
    case spec do
      %{^key => types} when is_list(types) ->
        %{spec | key => Enum.map(types, &pair/1)}

      %{^key => types} when is_map(types) and not is_struct(types) ->
        raise ArgumentError,
              "#{key} must be a list of {name, spec} pairs, since a map doesn't keep their order"

      _ ->
        spec
    end
  end

  defp pair({name, type}), do: [to_string(name), type]
  defp pair([name, type]), do: [to_string(name), type]

  @doc "Reads a document, or any node, from its JSON, as `Node.fromJSON` does."
  @spec node_from_json(schema(), json()) :: {:ok, doc()} | error()
  def node_from_json(schema, json), do: Native.node_from_json(schema, json)

  @doc "The document's JSON."
  @spec to_json(doc()) :: json()
  def to_json(doc), do: Native.to_json(doc)

  @doc "Checks that the document conforms to its schema."
  @spec check(doc()) :: :ok | error()
  def check(doc), do: Native.check(doc)

  @doc "Applies steps to the document, in order, and gives the changed document."
  @spec apply_steps(doc(), [json()]) :: {:ok, doc()} | error()
  def apply_steps(doc, steps), do: Native.apply_steps(doc, steps)

  @doc "The steps that undo `steps`, applied to `doc`, last first."
  @spec invert_steps(doc(), [json()]) :: {:ok, [json()]} | error()
  def invert_steps(doc, steps), do: Native.invert_steps(doc, steps)

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
