defmodule Tarnish.OpsTest do
  use ExUnit.Case, async: true

  # Op lists, textBetween and textContent as the real ProseMirror packages ran them, recorded by
  # `npm run test:js`: the starting document, and what came of it or the error thrown.
  @fixtures Path.expand("../../fixtures/ops.json", __DIR__)
  @external_resource @fixtures

  @kinds %{
    "RangeError" => :range_error,
    "SyntaxError" => :syntax_error,
    "ReplaceError" => :replace_error,
    "TransformError" => :transform_error,
    "TypeError" => :type_error,
    "Error" => :js_error
  }

  setup_all do
    specs = @fixtures |> File.read!() |> Jason.decode!(objects: :ordered_objects)

    schemas =
      for spec <- specs["schemas"] do
        {:ok, schema} = Tarnish.schema(spec)
        schema
      end

    %{schemas: schemas}
  end

  fixtures = @fixtures |> File.read!() |> Jason.decode!()

  defp doc(schemas, %{"schema" => schema, "doc" => json}) do
    {:ok, doc} = Tarnish.node_from_json(Enum.at(schemas, schema), json)
    doc
  end

  defp expected(%{"error" => %{"class" => class, "message" => message}}),
    do: {:error, {Map.fetch!(@kinds, class), message}}

  defp expected(%{"result" => result, "steps" => steps}), do: {:ok, result, steps}
  defp expected(%{"result" => result}), do: {:ok, result}

  # Text with a lone surrogate is recorded as its JSON, and comes back with U+FFFD for each lone
  # surrogate. JSON.stringify escapes a surrogate only when it's alone.
  defp expected(%{"resultJSON" => json}),
    do: {:ok, Jason.decode!(Regex.replace(~r/\\ud[89a-f][0-9a-f]{2}/i, json, "\\ufffd"))}

  for {fixture, index} <- Enum.with_index(fixtures["transforms"]) do
    @fixture fixture
    test "op list #{index} gives what ProseMirror gives", %{schemas: schemas} do
      doc = doc(schemas, @fixture)

      transformed =
        with {:ok, changed, steps} <- Tarnish.transform(doc, @fixture["ops"]),
             do: {:ok, Tarnish.to_json(changed), steps}

      assert transformed == expected(@fixture)
    end
  end

  for {fixture, index} <- Enum.with_index(fixtures["textBetween"]) do
    @fixture fixture
    test "textBetween #{index} is ProseMirror's", %{schemas: schemas} do
      %{"from" => from, "to" => to} = @fixture
      separator = @fixture["blockSeparator"]

      text =
        Tarnish.text_between(doc(schemas, @fixture), from, to, separator, @fixture["leafText"])

      assert text == expected(@fixture)
    end
  end

  for {fixture, index} <- Enum.with_index(fixtures["textContent"]) do
    @fixture fixture
    test "textContent #{index} is ProseMirror's", %{schemas: schemas} do
      assert Tarnish.text_content(doc(schemas, @fixture)) == @fixture["result"]
    end
  end

  describe "a transform's document" do
    setup %{schemas: [schema | _]}, do: %{schema: schema}

    defp paragraph(text),
      do: %{"type" => "paragraph", "content" => [%{"type" => "text", "text" => text}]}

    defp paragraphs(count, text),
      do: %{"type" => "doc", "content" => List.duplicate(paragraph(text), count)}

    test "shares the maps of the nodes its steps leave", %{schema: schema} do
      [first, second, third] = content = for text <- ~w(one two three), do: paragraph(text)
      {:ok, doc} = Tarnish.node_from_json(schema, %{"type" => "doc", "content" => content})
      mark = %{"type" => "em"}
      ops = [%{"op" => "addMark", "from" => 6, "to" => 9, "mark" => mark}]
      {:ok, changed, [_step]} = Tarnish.transform(doc, ops)
      %{"content" => [one, two, three]} = Tarnish.to_json(changed)
      assert :erts_debug.same(one, first) and :erts_debug.same(three, third)
      refute :erts_debug.same(two, second)
    end

    test "is the document given when there are no ops", %{schema: schema} do
      {:ok, doc} = Tarnish.node_from_json(schema, paragraphs(2, "same"))
      assert Tarnish.transform(doc, []) == {:ok, doc, []}
    end

    test "comes from a dirty scheduler past a normal one's limits", %{schema: schema} do
      {:ok, doc} = Tarnish.node_from_json(schema, paragraphs(200, "text"))
      insert = %{"op" => "insert", "pos" => 2, "content" => [%{"type" => "text", "text" => "x"}]}
      ops = List.duplicate(insert, 200)
      assert Tarnish.Native.transform(doc.ref, ops) == :dirty
      {:ok, changed, steps} = Tarnish.transform(doc, ops)
      assert length(steps) == 200
      %{"content" => [first | _]} = Tarnish.to_json(changed)
      assert first == paragraph("t" <> String.duplicate("x", 200) <> "ext")

      mark = %{"type" => "strong"}
      whole = [%{"op" => "addMark", "from" => 0, "to" => 1200, "mark" => mark}]
      assert Tarnish.Native.transform(doc.ref, whole) == :dirty
      assert {:ok, _, [_ | _]} = Tarnish.transform(doc, whole)
    end
  end

  describe "text" do
    setup %{schemas: [schema | _]}, do: %{schema: schema}

    test "comes from a dirty scheduler past a normal one's limits", %{schema: schema} do
      long = String.duplicate("long ", 200_000)
      json = %{"type" => "doc", "content" => [paragraph(long)]}
      {:ok, doc} = Tarnish.node_from_json(schema, json)
      assert Tarnish.Native.text_content(doc.ref) == :dirty
      assert Tarnish.text_content(doc) == long
      assert Tarnish.Native.text_between(doc.ref, 0, 1_000_002, "\n", nil) == :dirty
      assert Tarnish.text_between(doc, 0, 1_000_002, "\n") == {:ok, long}
    end

    test "has U+FFFD for each half of a surrogate pair a position splits", %{schema: schema} do
      {:ok, doc} = Tarnish.node_from_json(schema, paragraphs(1, "a😀b"))
      assert Tarnish.text_between(doc, 1, 3) == {:ok, "a�"}
      assert Tarnish.text_between(doc, 3, 5) == {:ok, "�b"}
    end
  end
end
