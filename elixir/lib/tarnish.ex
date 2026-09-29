defmodule Tarnish do
  @moduledoc """
  ProseMirror's document model and transforms, run in Rust.

  A schema is built once from its spec, and kept in Rust. A document is read once from its
  JSON, into binaries that Rust reads in place, so applying steps to a document doesn't
  convert it again, and a changed document is a binary more, holding only what the steps
  changed. Specs, steps and JSON are ProseMirror's JSON, as Jason decodes it: maps with string
  keys, or `Jason.OrderedObject`s where order matters.

  A document keeps the JSON it was read from. Its own JSON shares every part of that which is
  what ProseMirror writes for a node it still has, so after steps only the nodes they changed
  are made anew.

  Errors come back as `{:error, {kind, message}}`. The kind names the class ProseMirror throws:

    * `:range_error`
    * `:syntax_error`
    * `:replace_error`
    * `:transform_error`
    * `:type_error`
    * `:js_error`, for a plain `Error`

  A part of a term that ProseMirror reads and Jason couldn't encode, such as a tuple, raises
  `ArgumentError`. A part it ignores, such as a key a node doesn't have, isn't read.

  ## Conversions

  `parse_markdown/3`, `serialize_markdown/3`, `parse_html/2`, `serialize_html/2` and `convert/2`
  convert between a document's JSON and Markdown or HTML, as the application's own conversions
  do. They run in the application's NIF, which implements `convert/1` and `convert_light/1`
  (`config :tarnish, conversions: :nif`, the default), or in worker processes through
  `Tarnish.Bridge` (`conversions: :bridge`). Each gives `{:ok, value}` or `{:error, message}`.
  """

  alias Tarnish.Doc

  # The module that loaded the NIF: Tarnish.Native, or an application's own (Tarnish.NIF), which
  # is compiled after this package.
  @native Application.compile_env(:tarnish, :native, Tarnish.Native)
  @compile {:no_warn_undefined, @native}

  @opaque schema :: reference()
  @opaque doc :: %Doc{}
  @type json :: map() | list() | String.t() | number() | boolean() | nil
  @type error :: {:error, {atom(), String.t()}}

  @typedoc "A conversion: its operation, its input, and the options when it takes them."
  @type request :: {String.t(), json()} | {String.t(), json(), map()}
  @type converted :: {:ok, json()} | {:error, String.t()}

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
    |> @native.schema()
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

  # Each call below runs on a normal scheduler, which answers `:dirty` when the call has more
  # work than it should take on, and is then made on a dirty scheduler.

  @doc "Reads a document, or any node, from its JSON, as `Node.fromJSON` does."
  @spec node_from_json(schema(), json()) :: {:ok, doc()} | error()
  def node_from_json(schema, json) do
    read =
      with :dirty <- @native.node_from_json(schema, json),
           do: @native.node_from_json_dirty(schema, json)

    with {:ok, ref} <- read, do: {:ok, %Doc{ref: ref, json: json}}
  end

  @doc "The document's JSON."
  @spec to_json(doc()) :: json()
  def to_json(%Doc{ref: ref, json: json}) do
    with :dirty <- @native.to_json(ref, json), do: @native.to_json_dirty(ref, json)
  end

  @doc "Checks that the document conforms to its schema."
  @spec check(doc()) :: :ok | error()
  def check(%Doc{ref: ref}) do
    with :dirty <- @native.check(ref), do: @native.check_dirty(ref)
  end

  @doc "Applies steps to the document, in order, and gives the changed document."
  @spec apply_steps(doc(), [json()]) :: {:ok, doc()} | error()
  def apply_steps(%Doc{ref: ref} = doc, steps) do
    applied =
      with :dirty <- @native.apply_steps(ref, steps), do: @native.apply_steps_dirty(ref, steps)

    with {:ok, ref} <- applied, do: {:ok, %{doc | ref: ref}}
  end

  @doc "The steps that undo `steps`, applied to `doc`, last first."
  @spec invert_steps(doc(), [json()]) :: {:ok, [json()]} | error()
  def invert_steps(%Doc{ref: ref}, steps) do
    with :dirty <- @native.invert_steps(ref, steps), do: @native.invert_steps_dirty(ref, steps)
  end

  @doc """
  Maps a position through the changes the steps make.

  With `assoc` below zero, a position where content is inserted stays before it; otherwise it
  moves after it.
  """
  @spec map_position(schema(), [json()], non_neg_integer(), integer()) ::
          {:ok, non_neg_integer()} | error()
  def map_position(schema, steps, pos, assoc \\ 1) do
    with :dirty <- @native.map_position(schema, steps, pos, assoc),
         do: @native.map_position_dirty(schema, steps, pos, assoc)
  end

  @doc """
  Makes changes on the server: applies `ops` in order to one ProseMirror `Transform` of `doc`,
  and gives the changed document and the steps the transform made, which editors can apply.

  An op is a map naming a `Transform` method in `"op"`, with the method's arguments by the
  names ProseMirror gives them. Nodes, fragments, slices, marks and steps are their JSON, and
  node and mark types their names:

      Tarnish.transform(doc, [
        %{"op" => "addMark", "from" => 1, "to" => 6, "mark" => %{"type" => "em"}},
        %{"op" => "setBlockType", "from" => 1, "type" => "heading", "attrs" => %{"level" => 2}}
      ])

  The methods are `replace`, `replaceWith`, `delete`, `insert`, `replaceRange`,
  `replaceRangeWith`, `deleteRange`, `addMark`, `removeMark`, `addNodeMark`, `removeNodeMark`,
  `setNodeMarkup`, `setNodeAttribute`, `setDocAttribute`, `setBlockType`, `lift`, `wrap`,
  `join`, `split`, `clearIncompatible`, `step` and `maybeStep`.

    * `removeMark` and `removeNodeMark` take a mark's JSON or a mark type's name.
    * `lift` and `wrap` take `"from"`, `"to"` and an optional `"depth"` for their range, which
      is `$from.blockRange($to)` without a depth. Without a `"target"`, `lift` lifts to
      `liftTarget`'s. Without `"wrappers"`, `wrap` wraps in `"nodeType"` with `"attrs"`, as
      `findWrapping` finds.
    * Positions are whole numbers, and a range's `"to"` can't come before its `"from"`.

  An op that fails fails the whole call, with the error ProseMirror throws.
  """
  @spec transform(doc(), [json()]) :: {:ok, doc(), [json()]} | error()
  def transform(%Doc{ref: ref} = doc, ops) do
    transformed =
      with :dirty <- @native.transform(ref, ops), do: @native.transform_dirty(ref, ops)

    with {:ok, ref, steps} <- transformed, do: {:ok, %{doc | ref: ref}, steps}
  end

  @doc """
  The text between two positions, as `textBetween` gives it: `block_separator` goes between
  blocks, and `leaf_text` stands for each leaf node that isn't text.

  A lone surrogate, where a position splits a pair, is U+FFFD.
  """
  @spec text_between(
          doc(),
          non_neg_integer(),
          non_neg_integer(),
          String.t() | nil,
          String.t() | nil
        ) :: {:ok, String.t()} | error()
  def text_between(%Doc{ref: ref}, from, to, block_separator \\ nil, leaf_text \\ nil) do
    with :dirty <- @native.text_between(ref, from, to, block_separator, leaf_text),
         do: @native.text_between_dirty(ref, from, to, block_separator, leaf_text)
  end

  @doc "All the text in the document, as `textContent` gives it."
  @spec text_content(doc()) :: String.t()
  def text_content(%Doc{ref: ref}) do
    {:ok, text} = with :dirty <- @native.text_content(ref), do: @native.text_content_dirty(ref)
    text
  end

  @doc "Parses Markdown to a document's JSON."
  @spec parse_markdown(String.t(), map(), keyword()) :: converted()
  def parse_markdown(markdown_string, markdown_options \\ %{}, opts \\ []),
    do: convert_one({"parseMarkdown", markdown_string, markdown_options}, opts)

  @doc "Serializes a document's JSON to Markdown."
  @spec serialize_markdown(json(), map(), keyword()) :: converted()
  def serialize_markdown(doc_json, markdown_options \\ %{}, opts \\ []),
    do: convert_one({"serializeMarkdown", doc_json, markdown_options}, opts)

  @doc "Parses HTML to a document's JSON."
  @spec parse_html(String.t(), keyword()) :: converted()
  def parse_html(html_string, opts \\ []), do: convert_one({"parseHTML", html_string}, opts)

  @doc "Serializes a document's JSON to HTML."
  @spec serialize_html(json(), keyword()) :: converted()
  def serialize_html(doc_json, opts \\ []), do: convert_one({"serializeHTML", doc_json}, opts)

  @doc """
  Makes each conversion, in order, and gives one result for each. A request is
  `{operation, input}` or `{operation, input, options}`, where the operation is
  `"parseMarkdown"`, `"serializeMarkdown"`, `"parseHTML"` or `"serializeHTML"`.

  The NIF spreads a batch over its threads, as `Tarnish.Bridge` spreads one over its workers,
  and reads maps, lists, strings, numbers and atoms itself. A request holding any other term,
  such as a struct, is encoded with Jason and decoded back first, as a worker would read it, so a
  term Jason can't encode raises as Jason raises. `opts` are `Tarnish.Bridge.convert/2`'s.
  """
  @spec convert([request()], keyword()) :: [converted()]
  def convert(requests, opts \\ []) do
    case conversions() do
      :nif -> convert_in_nif(requests)
      :bridge -> Tarnish.Bridge.convert(requests, opts)
    end
  end

  @doc false
  def conversions do
    case Application.get_env(:tarnish, :conversions, :nif) do
      conversions when conversions in [:nif, :bridge] ->
        conversions

      other ->
        raise ArgumentError,
              "config :tarnish, conversions: must be :nif or :bridge, not #{inspect(other)}"
    end
  end

  defp convert_one(request, opts) do
    [converted] = convert([request], opts)
    converted
  end

  defp convert_in_nif(requests) do
    answers = native_convert(requests)

    if :not_json in answers do
      encoded = for {request, :not_json} <- Enum.zip(requests, answers), do: as_encoded(request)
      fill(answers, native_convert(encoded))
    else
      answers
    end
  end

  # A single request converts on the caller's own scheduler when the NIF finds it light, which
  # spares handing the process to a dirty scheduler and back.
  defp native_convert([request]) do
    case @native.convert_light(request) do
      :dirty -> @native.convert([request])
      answer -> [answer]
    end
  end

  defp native_convert(requests), do: @native.convert(requests)

  defp fill([:not_json | answers], [answer | rest]) when answer != :not_json,
    do: [answer | fill(answers, rest)]

  defp fill([answer | answers], rest) when answer != :not_json, do: [answer | fill(answers, rest)]
  defp fill([], []), do: []

  # The request as a worker reads the JSON Jason writes for it, each object's keys in the order
  # written.
  defp as_encoded({operation, input}), do: as_encoded({operation, input, nil})

  defp as_encoded({operation, input, options}) do
    request = {decoded(operation), decoded(input)}
    if options in [nil, %{}], do: request, else: Tuple.append(request, decoded(options))
  end

  defp decoded(term), do: term |> Jason.encode!() |> Jason.decode!(objects: :ordered_objects)
end
