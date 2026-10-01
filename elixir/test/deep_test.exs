defmodule Tarnish.DeepTest do
  use ExUnit.Case, async: true

  # Far deeper than a dirty scheduler's stack, or a thread of the NIF's batch pool, could recurse
  # through. Each deep part starts near the top of what holds it, where an unguarded level would
  # overflow.
  @depth 100_000

  @paragraph %{"type" => "paragraph"}

  setup_all do
    nodes = [
      doc: %{content: "block+"},
      paragraph: %{content: "text*", group: "block"},
      quote: %{content: "block+", group: "block", attrs: %{data: %{default: nil}}},
      text: %{}
    ]

    {:ok, schema} = Tarnish.Schema.new(%{nodes: nodes})
    %{schema: schema}
  end

  defp quotes(inside) do
    Enum.reduce(1..@depth, inside, fn _, inner ->
      %{"type" => "quote", "attrs" => %{"data" => nil}, "content" => [inner]}
    end)
  end

  # Maps and lists in turn, a map outermost.
  defp value do
    Enum.reduce(1..@depth, nil, fn level, inner ->
      if rem(level, 2) == 0, do: %{"a" => inner}, else: [inner]
    end)
  end

  defp doc(schema, json) do
    {:ok, doc} = Tarnish.Node.from_json(schema, json)
    doc
  end

  defp replace(from, to, content) do
    %{"stepType" => "replace", "from" => from, "to" => to, "slice" => %{"content" => content}}
  end

  test "a document changed deep down has its JSON made anew down to the change", %{
    schema: schema
  } do
    doc = doc(schema, %{"type" => "doc", "content" => [quotes(@paragraph)]})
    text = %{"type" => "text", "text" => "x"}
    {:ok, changed} = Tarnish.Step.apply(doc, [replace(@depth + 1, @depth + 1, [text])])
    paragraph = %{"type" => "paragraph", "content" => [text]}
    assert Tarnish.Node.to_json(changed) == %{"type" => "doc", "content" => [quotes(paragraph)]}
  end

  test "steps read deep, and the content they insert is written whole", %{schema: schema} do
    doc = doc(schema, %{"type" => "doc", "content" => [@paragraph]})
    holding = %{"type" => "quote", "attrs" => %{"data" => value()}, "content" => [@paragraph]}
    inserted = [holding, quotes(@paragraph)]
    {:ok, changed} = Tarnish.Step.apply(doc, [replace(0, 0, inserted)])

    assert Tarnish.Node.to_json(changed) == %{
             "type" => "doc",
             "content" => inserted ++ [@paragraph]
           }
  end

  test "the step undoing a deletion holds the deep content deleted", %{schema: schema} do
    deleted = quotes(@paragraph)
    doc = doc(schema, %{"type" => "doc", "content" => [deleted, @paragraph]})
    delete = %{"stepType" => "replace", "from" => 0, "to" => 2 * @depth + 2}
    assert Tarnish.Step.invert(doc, [delete]) == {:ok, [replace(0, 0, [deleted])]}
  end

  test "a batch of the NIF's reads deep requests on its threads" do
    start_supervised!({Tarnish.Bridge, backend: :nif})
    value = value()
    # The test NIF's serializeMarkdown writes the document's JSON.
    written = {:ok, Jason.encode!(value)}
    requests = [{"serializeMarkdown", value}, {"serializeMarkdown", value}]
    assert Tarnish.Bridge.each(requests, backend: :nif) == [written, written]
  end
end
