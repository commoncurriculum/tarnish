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
  # `{schema, chunks}`. And the JSON it was read from, whose parts its own JSON shares where
  # they're what ProseMirror writes, or `nil` for a node loaded from its binary.
  @derive {Inspect, only: []}
  @enforce_keys [:ref, :json]
  defstruct [:ref, :json]

  @type t :: %__MODULE__{}

  @doc "Reads a node from its JSON, as `Node.fromJSON(schema, json)` does."
  @spec from_json(Tarnish.Schema.t(), Tarnish.json()) :: {:ok, t()} | Tarnish.error()
  def from_json(schema, json) do
    read =
      with :dirty <- @native.node_from_json(schema, json),
           do: @native.node_from_json_dirty(schema, json)

    with {:ok, ref} <- read, do: {:ok, %__MODULE__{ref: ref, json: json}}
  end

  @doc """
  The node as a binary to keep, in a database or a cache, which `from_binary/2` loads back far
  faster than `from_json/2` reads its JSON. The binary is only for the schema the node is of, and
  for this version of tarnish.
  """
  @spec to_binary(t()) :: binary()
  def to_binary(%__MODULE__{ref: ref}) do
    with :dirty <- @native.node_to_binary(ref), do: @native.node_to_binary_dirty(ref)
  end

  @doc """
  Loads a node from the binary `to_binary/1` gave, which it reads in place. Loading checks that
  the binary was written for this schema and holds a node; it doesn't check every byte, so it
  should only be given binaries `to_binary/1` wrote.
  """
  @spec from_binary(Tarnish.Schema.t(), binary()) :: {:ok, t()} | Tarnish.error()
  def from_binary(schema, binary) when is_binary(binary) do
    read =
      with :dirty <- @native.node_from_binary(schema, binary),
           do: @native.node_from_binary_dirty(schema, binary)

    with {:ok, ref} <- read, do: {:ok, %__MODULE__{ref: ref, json: nil}}
  end

  @doc "The node's JSON, as `node.toJSON()` gives it."
  @spec to_json(t()) :: Tarnish.json()
  def to_json(%__MODULE__{ref: ref, json: json}) do
    with :dirty <- @native.to_json(ref, json), do: @native.to_json_dirty(ref, json)
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
