defmodule Tarnish.Bridge do
  @moduledoc """
  Converts documents between Markdown, HTML and ProseMirror JSON with the application's own
  conversions, in its NIF or in Node workers. Both give the same answers.

      config :tarnish, Tarnish.Bridge, backend: :node, conversions: "/path/to/conversions.mjs"
      config :tarnish, Tarnish.Bridge, backend: :nif

  An application makes its four conversions twice, in a JavaScript module and in its NIF (see
  `Tarnish.NIF`), and each refuses a request as the other does. tarnish refuses the rest on
  both: a request whose operation isn't one of the four, or whose input isn't text for a parse
  and an object for a serialization, gets `"Unknown operation or invalid input"`.

  `:node`, the default, sends the requests to a pool of workers, `Tarnish.Bridge.Pool`, which
  run the module that `conversions:` names. It exports `parseMarkdown`, `serializeMarkdown`,
  `parseHTML` and `serializeHTML`, each taking a request's input and its options. The pool also
  takes `node:`, the executable (`"node"` by default), `size:`, its count of workers (2 by
  default), and `lazy: true` to start a worker only once a call needs it. `:nif` calls the NIF
  that `config :tarnish, native:` names.

  Put `Tarnish.Bridge` in your supervision tree. It loads the NIF, so that an application whose
  NIF doesn't load fails to start, and it starts the pool for `:node`, and for `:nif` when the
  pool is lazy, which costs nothing until a call picks `:node`. `start_link/1` takes `backend:`
  and the pool's options, over the configured ones, and `name:`.

  Each conversion gives `{:ok, value}` or `{:error, message}`. Its `opts` take `backend:`, over
  the configured one, so that a test can hold both backends to the same answers. On `:node`, a
  worker that exits or passes the timeout raises, and `opts` take the pool's `timeout:` in
  milliseconds (30,000 by default) and `pool:`, the name of a pool other than the one in your
  supervision tree.
  """

  alias Tarnish.Bridge.Pool

  @native Application.compile_env(:tarnish, :native, Tarnish.Native)
  @compile {:no_warn_undefined, @native}

  @typedoc """
  A conversion: its operation, its input, and its options when it takes them. The input is read
  as Jason would encode it.
  """
  @type request :: {String.t(), term()} | {String.t(), term(), map() | nil}
  @type result :: {:ok, Tarnish.json()} | {:error, String.t()}

  def child_spec(opts),
    do: %{id: Keyword.get(opts, :name, __MODULE__), start: {__MODULE__, :start_link, [opts]}}

  def start_link(opts) do
    Code.ensure_loaded!(@native)

    if backend(opts) == :node or Pool.lazy?(opts), do: Pool.start_link(opts), else: :ignore
  end

  @doc "Markdown to a document's JSON: `{:ok, doc_json}` or `{:error, message}`."
  @spec parse_markdown(String.t(), map(), keyword()) :: result()
  def parse_markdown(markdown_string, markdown_options \\ %{}, opts \\ []),
    do: one({"parseMarkdown", markdown_string, markdown_options}, opts)

  @doc "A document's JSON to Markdown: `{:ok, markdown_string}` or `{:error, message}`."
  @spec serialize_markdown(Tarnish.json(), map(), keyword()) :: result()
  def serialize_markdown(doc_json, markdown_options \\ %{}, opts \\ []),
    do: one({"serializeMarkdown", doc_json, markdown_options}, opts)

  @doc "HTML to a document's JSON: `{:ok, doc_json}` or `{:error, message}`."
  @spec parse_html(String.t(), map(), keyword()) :: result()
  def parse_html(html_string, options \\ %{}, opts \\ []),
    do: one({"parseHTML", html_string, options}, opts)

  @doc "A document's JSON to HTML: `{:ok, html_string}` or `{:error, message}`."
  @spec serialize_html(Tarnish.json(), map(), keyword()) :: result()
  def serialize_html(doc_json, options \\ %{}, opts \\ []),
    do: one({"serializeHTML", doc_json, options}, opts)

  @doc """
  Makes each conversion, in order, and gives one result for each. The operation of a request is
  `"parseMarkdown"`, `"serializeMarkdown"`, `"parseHTML"` or `"serializeHTML"`.

  The NIF spreads a batch over its threads, as the pool spreads one over its workers. It reads
  maps, lists, strings, numbers and atoms itself. A request holding any other term, such as a
  struct, is encoded with Jason and decoded back first, as a worker would read it, so a term
  Jason can't encode raises as Jason raises.
  """
  @spec each([request()], keyword()) :: [result()]
  def each(requests, opts \\ []) do
    requests = Enum.map(requests, &without_empty_options/1)

    case backend(opts) do
      :node -> Pool.run(requests, opts)
      :nif -> run_in_nif(requests)
    end
  end

  defp backend(opts) do
    configured = Keyword.get(Application.get_env(:tarnish, __MODULE__, []), :backend, :node)

    case Keyword.get(opts, :backend, configured) do
      backend when backend in [:node, :nif] ->
        backend

      other ->
        raise ArgumentError,
              "Tarnish.Bridge's backend must be :node or :nif, not #{inspect(other)}"
    end
  end

  defp one(request, opts) do
    [result] = each([request], opts)
    result
  end

  # Empty options are no options, which a conversion that takes none accepts.
  defp without_empty_options({operation, input, options}) when options in [nil, %{}],
    do: {operation, input}

  defp without_empty_options(request), do: request

  defp run_in_nif(requests) do
    answers = convert_in_nif(requests)

    if :not_json in answers do
      encoded = for {request, :not_json} <- Enum.zip(requests, answers), do: as_encoded(request)
      fill(answers, convert_in_nif(encoded))
    else
      answers
    end
  end

  # A single request converts on the caller's own scheduler when the NIF finds it light, which
  # spares handing the process to a dirty scheduler and back.
  defp convert_in_nif([request]) do
    case @native.convert_light(request) do
      :dirty -> @native.convert([request])
      answer -> [answer]
    end
  end

  defp convert_in_nif(requests), do: @native.convert(requests)

  defp fill([:not_json | answers], [answer | rest]) when answer != :not_json,
    do: [answer | fill(answers, rest)]

  defp fill([answer | answers], rest) when answer != :not_json, do: [answer | fill(answers, rest)]
  defp fill([], []), do: []

  # The request as a worker reads the JSON Jason writes for it, each object's keys in the order
  # written.
  defp as_encoded({operation, input}), do: {decoded(operation), decoded(input)}

  defp as_encoded({operation, input, options}),
    do: {decoded(operation), decoded(input), decoded(options)}

  defp decoded(term), do: term |> Jason.encode!() |> Jason.decode!(objects: :ordered_objects)
end
