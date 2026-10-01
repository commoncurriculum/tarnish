defmodule Tarnish.OpsTest do
  use ExUnit.Case, async: true

  @native Application.compile_env(:tarnish, :native, Tarnish.Native)

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
        {:ok, schema} = Tarnish.Schema.new(spec)
        schema
      end

    %{schemas: schemas}
  end

  fixtures = @fixtures |> File.read!() |> Jason.decode!()

  defp doc(schemas, %{"schema" => schema, "doc" => json}) do
    {:ok, doc} = Tarnish.Node.from_json(Enum.at(schemas, schema), json)
    doc
  end

  defp expected(%{"error" => %{"class" => class, "message" => message}}),
    do: {:error, {Map.fetch!(@kinds, class), message}}

  defp expected(%{"result" => result, "steps" => steps}), do: {:ok, result, steps}
  defp expected(%{"result" => result}), do: {:ok, result}

  # Text with a lone surrogate is recorded as its JSON, and comes back with U+FFFD for each lone
  # surrogate.
  defp expected(%{"resultJSON" => json}), do: {:ok, Tarnish.JSON.decode!(json)}

  for {fixture, index} <- Enum.with_index(fixtures["transforms"]) do
    @fixture fixture
    test "op list #{index} gives what ProseMirror gives", %{schemas: schemas} do
      doc = doc(schemas, @fixture)

      transformed =
        with {:ok, %Tarnish.Transform{doc: changed, steps: steps}} <-
               Tarnish.Transform.new(doc, @fixture["ops"]),
             do: {:ok, Tarnish.Node.to_json(changed), steps}

      assert transformed === expected(@fixture)
    end
  end

  for {fixture, index} <- Enum.with_index(fixtures["textBetween"]) do
    @fixture fixture
    test "textBetween #{index} is ProseMirror's", %{schemas: schemas} do
      %{"from" => from, "to" => to} = @fixture
      separator = @fixture["blockSeparator"]

      text =
        Tarnish.Node.text_between(
          doc(schemas, @fixture),
          from,
          to,
          separator,
          @fixture["leafText"]
        )

      assert text === expected(@fixture)
    end
  end

  for {fixture, index} <- Enum.with_index(fixtures["textContent"]) do
    @fixture fixture
    test "textContent #{index} is ProseMirror's", %{schemas: schemas} do
      assert Tarnish.Node.text_content(doc(schemas, @fixture)) === @fixture["result"]
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
      {:ok, doc} = Tarnish.Node.from_json(schema, %{"type" => "doc", "content" => content})
      mark = %{"type" => "em"}
      ops = [%{"op" => "addMark", "from" => 6, "to" => 9, "mark" => mark}]
      {:ok, %Tarnish.Transform{doc: changed, steps: steps}} = Tarnish.Transform.new(doc, ops)
      assert steps === [%{"stepType" => "addMark", "mark" => mark, "from" => 6, "to" => 9}]
      %{"type" => "doc", "content" => [one, two, three]} = json = Tarnish.Node.to_json(changed)
      assert map_size(json) === 2
      assert :erts_debug.same(one, first) and :erts_debug.same(three, third)

      assert two === %{
               second
               | "content" => [%{"type" => "text", "text" => "two", "marks" => [mark]}]
             }

      refute :erts_debug.same(two, second)
    end

    test "is the document given when there are no ops", %{schema: schema} do
      {:ok, doc} = Tarnish.Node.from_json(schema, paragraphs(2, "same"))
      assert Tarnish.Transform.new(doc, []) === {:ok, %Tarnish.Transform{doc: doc, steps: []}}
    end

    test "comes from a dirty scheduler past a normal one's limits", %{schema: schema} do
      {:ok, doc} = Tarnish.Node.from_json(schema, paragraphs(200, "text"))
      x = [%{"type" => "text", "text" => "x"}]
      ops = List.duplicate(%{"op" => "insert", "pos" => 2, "content" => x}, 200)
      assert @native.transform(doc.ref, ops) === :dirty
      {:ok, %Tarnish.Transform{doc: changed, steps: steps}} = Tarnish.Transform.new(doc, ops)
      step = %{"stepType" => "replace", "from" => 2, "to" => 2, "slice" => %{"content" => x}}
      assert steps === List.duplicate(step, 200)
      %{"content" => [_ | rest]} = paragraphs(200, "text")
      first = paragraph("t" <> String.duplicate("x", 200) <> "ext")
      assert Tarnish.Node.to_json(changed) === %{"type" => "doc", "content" => [first | rest]}

      mark = %{"type" => "strong"}
      whole = [%{"op" => "addMark", "from" => 0, "to" => 1200, "mark" => mark}]
      assert @native.transform(doc.ref, whole) === :dirty
      {:ok, %Tarnish.Transform{doc: marked, steps: steps}} = Tarnish.Transform.new(doc, whole)

      assert steps ===
               for(
                 start <- 1..1195//6,
                 do: %{
                   "stepType" => "addMark",
                   "mark" => mark,
                   "from" => start,
                   "to" => start + 4
                 }
               )

      strong = %{"type" => "text", "text" => "text", "marks" => [mark]}
      strong_paragraph = %{"type" => "paragraph", "content" => [strong]}

      assert Tarnish.Node.to_json(marked) === %{
               paragraphs(200, "text")
               | "content" => List.duplicate(strong_paragraph, 200)
             }
    end
  end

  describe "text" do
    setup %{schemas: [schema | _]}, do: %{schema: schema}

    test "comes from a dirty scheduler past a normal one's limits", %{schema: schema} do
      long = String.duplicate("long ", 200_000)
      json = %{"type" => "doc", "content" => [paragraph(long)]}
      {:ok, doc} = Tarnish.Node.from_json(schema, json)
      assert @native.text_content(doc.ref) === :dirty
      assert Tarnish.Node.text_content(doc) === long
      assert @native.text_between(doc.ref, 0, 1_000_002, "\n", nil) === :dirty
      assert Tarnish.Node.text_between(doc, 0, 1_000_002, "\n") === {:ok, long}
    end

    test "has U+FFFD for each half of a surrogate pair a position splits", %{schema: schema} do
      {:ok, doc} = Tarnish.Node.from_json(schema, paragraphs(1, "a😀b"))
      assert Tarnish.Node.text_between(doc, 1, 3) === {:ok, "a�"}
      assert Tarnish.Node.text_between(doc, 3, 5) === {:ok, "�b"}
    end
  end
end
