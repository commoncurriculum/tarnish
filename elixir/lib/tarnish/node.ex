defmodule Tarnish.Node do
  @moduledoc """
  A node, a document among them, as ProseMirror's `Node`: read once from its JSON, and kept in
  Rust.

  Each call runs on a normal scheduler, which answers `:dirty` when the call has more work than
  it should take on, and is then made on a dirty scheduler.
  """

  @native Application.compile_env(:tarnish, :native, Tarnish.Native)
  @compile {:no_warn_undefined, @native}

  # The binaries of the chunks that hold the node, newest first, which Rust reads in place, as
  # `{schema, chunks}`. And a JSON whose parts its own JSON shares, and `of`, what that JSON is
  # of: nil for the JSON the node was read from, whose parts are what ProseMirror writes for the
  # nodes read, or the ref `to_json_kept/1` or `Tarnish.Step.apply_kept/2` last wrote it for,
  # whose parts are the JSON of that ref's nodes.
  @derive {Inspect, only: []}
  @enforce_keys [:ref, :json]
  defstruct [:ref, :json, of: nil]

  @type t :: %__MODULE__{}

  @doc "Reads a node from its JSON, as `Node.fromJSON(schema, json)` does."
  @spec from_json(Tarnish.Schema.t(), Tarnish.json()) :: {:ok, t()} | Tarnish.error()
  def from_json(schema, json) do
    read =
      with :dirty <- @native.node_from_json(schema, json),
           do: @native.node_from_json_dirty(schema, json)

    with {:ok, ref} <- read, do: {:ok, %__MODULE__{ref: ref, json: json}}
  end

  @doc "The node's JSON, as `node.toJSON()` gives it."
  @spec to_json(t()) :: Tarnish.json()
  def to_json(%__MODULE__{ref: ref, json: json} = node) do
    case Map.get(node, :of) do
      nil ->
        with :dirty <- @native.to_json(ref, json), do: @native.to_json_dirty(ref, json)

      ^ref ->
        json

      of ->
        with :dirty <- @native.to_json_from(ref, json, of),
             do: @native.to_json_from_dirty(ref, json, of)
    end
  end

  @doc """
  The node's JSON, as `to_json/1` gives it, and the node, which keeps that JSON in place of the
  one it held.

  A process that keeps a node between changes, as a socket's does between messages, writes it
  with this and keeps the node it returns. `Tarnish.Step.apply/2` and `Tarnish.Transform.new/2`
  carry that JSON to the nodes they make of it, whose JSON then shares the parts written here for
  the nodes they left. `Tarnish.Step.apply_kept/2` applies a message's steps and writes the
  document they make in one call, and shares more, as its docs say. What either gives is the
  same, term for term.

  The node returned is the same document, and is no longer `==` to the node given. It also holds
  the chunks of the document this JSON was written for, until it is written again with this.
  """
  @spec to_json_kept(t()) :: {Tarnish.json(), t()}
  def to_json_kept(%__MODULE__{ref: ref} = node) do
    json = to_json(node)
    {json, Map.merge(node, %{json: json, of: ref})}
  end

  @doc "Checks that the node conforms to its schema, as `node.check()` does."
  @spec check(t()) :: :ok | Tarnish.error()
  def check(%__MODULE__{ref: ref}) do
    with :dirty <- @native.check(ref), do: @native.check_dirty(ref)
  end

  @doc """
  The text between two positions, as `node.textBetween(from, to, blockSeparator, leafText)`
  gives it: `block_separator` goes between blocks, and `leaf_text` stands for each leaf node that
  isn't text.

  A lone surrogate, where a position splits a pair, is U+FFFD.
  """
  @spec text_between(
          t(),
          non_neg_integer(),
          non_neg_integer(),
          String.t() | nil,
          String.t() | nil
        ) :: {:ok, String.t()} | Tarnish.error()
  def text_between(%__MODULE__{ref: ref}, from, to, block_separator \\ nil, leaf_text \\ nil) do
    with :dirty <- @native.text_between(ref, from, to, block_separator, leaf_text),
         do: @native.text_between_dirty(ref, from, to, block_separator, leaf_text)
  end

  @doc "All the text in the node, as `node.textContent` gives it."
  @spec text_content(t()) :: String.t()
  def text_content(%__MODULE__{ref: ref}) do
    {:ok, text} = with :dirty <- @native.text_content(ref), do: @native.text_content_dirty(ref)
    text
  end
end
