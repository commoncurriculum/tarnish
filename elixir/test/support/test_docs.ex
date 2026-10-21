defmodule Tarnish.TestDocs do
  @moduledoc false
  # The JSON of the documents and steps the tests make, and the card stack TARNISH_BENCH_DOC
  # names, which is read here and never kept in the repo.

  def text(text, marks \\ []) do
    case marks do
      [] -> %{"type" => "text", "text" => text}
      marks -> %{"type" => "text", "text" => text, "marks" => marks}
    end
  end

  def paragraph(content), do: %{"type" => "paragraph", "content" => content}
  def item(content), do: %{"type" => "list_item", "content" => content}
  def list(items), do: %{"type" => "bullet_list", "content" => items}

  def insert(at, text),
    do: %{
      "stepType" => "replace",
      "from" => at,
      "to" => at,
      "slice" => %{"content" => [text(text)]}
    }

  def replace(from, to, content) do
    %{"stepType" => "replace", "from" => from, "to" => to, "slice" => %{"content" => content}}
  end

  def doc(schema, json) do
    {:ok, doc} = Tarnish.Node.from_json(schema, json)
    doc
  end

  def ordered(path), do: path |> File.read!() |> Jason.decode!(objects: :ordered_objects)

  def schema(spec) do
    {:ok, schema} = Tarnish.Schema.new(spec)
    schema
  end

  @doc """
  The card stack TARNISH_BENCH_DOC names, `%{schema:, doc:, changes:}`, its schema the file's or
  else TARNISH_BENCH_SCHEMA's; `nil` when it names none.
  """
  def bench_doc do
    with path when is_binary(path) <- System.get_env("TARNISH_BENCH_DOC"),
         true <- File.exists?(path) do
      file = Jason.decode!(File.read!(path))
      spec = ordered(path)["schema"] || ordered(System.fetch_env!("TARNISH_BENCH_SCHEMA"))
      schema = schema(spec)
      %{schema: schema, doc: doc(schema, file["doc"]), changes: file["changes"]}
    else
      _ -> nil
    end
  end
end
