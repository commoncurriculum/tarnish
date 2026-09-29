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
  | `DOMParser`, `DOMSerializer` | `Tarnish.DOMParser`, `Tarnish.DOMSerializer` |
  | prosemirror-markdown's `MarkdownParser`, `MarkdownSerializer` | `Tarnish.MarkdownParser`, `Tarnish.MarkdownSerializer` |

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

  ## Conversions

  `Tarnish.DOMParser`, `Tarnish.DOMSerializer`, `Tarnish.MarkdownParser` and
  `Tarnish.MarkdownSerializer` convert between a document's JSON and HTML or Markdown, as the
  application's own conversions do, and `convert/2` makes a batch of them. They run in the
  application's NIF, which implements `convert/1` and `convert_light/1`
  (`config :tarnish, conversions: :nif`, the default), or in worker processes that
  `Tarnish.Bridge` connects (`conversions: :bridge`). Each gives `{:ok, value}` or
  `{:error, message}`.
  """

  # The module that loaded the NIF: Tarnish.Native, or an application's own (Tarnish.NIF), which
  # is compiled after this package.
  @native Application.compile_env(:tarnish, :native, Tarnish.Native)
  @compile {:no_warn_undefined, @native}

  @type json :: map() | list() | String.t() | number() | boolean() | nil
  @type error :: {:error, {atom(), String.t()}}

  @typedoc "A conversion: its operation, its input, and the options when it takes them."
  @type request :: {String.t(), json()} | {String.t(), json(), map()}
  @type converted :: {:ok, json()} | {:error, String.t()}

  @doc """
  Makes each conversion, in order, and gives one result for each. A request is
  `{operation, input}` or `{operation, input, options}`, where the operation is
  `"parseMarkdown"`, `"serializeMarkdown"`, `"parseHTML"` or `"serializeHTML"`.

  The NIF spreads a batch over its threads, as `Tarnish.Bridge` spreads one over its workers,
  and reads maps, lists, strings, numbers and atoms itself. A request holding any other term,
  such as a struct, is encoded with Jason and decoded back first, as a worker would read it, so a
  term Jason can't encode raises as Jason raises.

  `opts` are for the bridge, which the NIF doesn't need: `timeout:` in milliseconds (30,000 by
  default), and `pool:` and `size:` for a bridge other than the one in your supervision tree.
  """
  @spec convert([request()], keyword()) :: [converted()]
  def convert(requests, opts \\ []) do
    case conversions() do
      :nif -> convert_in_nif(requests)
      :bridge -> Tarnish.Bridge.call(requests, opts)
    end
  end

  @doc false
  def convert_one(request, opts) do
    [converted] = convert([request], opts)
    converted
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
