defmodule TarnishTest do
  use ExUnit.Case, async: true

  @native Application.compile_env(:tarnish, :native, Tarnish.Native)

  # Transforms prosemirror-transform's own tests make, recorded from the real package by
  # `npm run test:js`: a schema, a starting document, the steps, the document they give, and
  # positions mapped through them.
  @fixtures Path.expand("../../fixtures/transform.json", __DIR__)
  @external_resource @fixtures

  # What the real prosemirror-model makes of inputs its own tests don't give it, recorded by
  # `npm run test:js` too.
  @model Path.expand("../../fixtures/model.json", __DIR__)
  @external_resource @model

  defp schemas(path) do
    specs = path |> File.read!() |> Jason.decode!(objects: :ordered_objects)

    for spec <- specs["schemas"] do
      {:ok, schema} = Tarnish.Schema.new(spec)
      schema
    end
  end

  setup_all do
    %{schemas: schemas(@fixtures), model_schemas: schemas(@model)}
  end

  fixtures = @fixtures |> File.read!() |> Jason.decode!()

  for {fixture, index} <- Enum.with_index(fixtures["tests"]) do
    @fixture fixture
    test "transform #{index} gives the document ProseMirror gives", %{schemas: schemas} do
      %{"schema" => schema, "start" => start, "steps" => steps, "result" => result} = @fixture
      schema = Enum.at(schemas, schema)
      {:ok, doc} = Tarnish.Node.from_json(schema, start)

      {:ok, changed} = Tarnish.Step.apply(doc, steps)
      assert Tarnish.Node.to_json(changed) === result

      {:ok, inverted} = Tarnish.Step.invert(doc, steps)
      {:ok, undone} = Tarnish.Step.apply(changed, inverted)
      assert Tarnish.Node.to_json(undone) === start

      for [from, to] <- @fixture["mapping"] do
        assert Tarnish.Mapping.map(schema, steps, from, 1) === {:ok, to}
      end
    end
  end

  model = @model |> File.read!() |> Jason.decode!()
  kinds = %{"RangeError" => :range_error, "TypeError" => :type_error, "Error" => :js_error}

  for {fixture, index} <- Enum.with_index(model["fromJSON"]) do
    @fixture fixture
    @kinds kinds
    test "node #{index} from JSON is the one ProseMirror reads", %{model_schemas: schemas} do
      schema = Enum.at(schemas, @fixture["schema"])

      expected =
        case @fixture do
          %{"result" => result} ->
            {:ok, result}

          %{"error" => %{"class" => class, "message" => message}} ->
            {:error, {@kinds[class], message}}
        end

      answer =
        with {:ok, doc} <- Tarnish.Node.from_json(schema, @fixture["json"]),
             do: {:ok, Tarnish.Node.to_json(doc)}

      assert answer === expected
    end
  end

  for {fixture, index} <- Enum.with_index(model["transforms"]) do
    @fixture fixture
    test "a step splitting a surrogate pair leaves U+FFFD for each half, #{index}",
         %{model_schemas: schemas} do
      %{"schema" => schema, "start" => start, "steps" => steps, "result" => result} = @fixture
      {:ok, doc} = Tarnish.Node.from_json(Enum.at(schemas, schema), start)
      {:ok, changed} = Tarnish.Step.apply(doc, steps)
      assert Tarnish.Node.to_json(changed) === Tarnish.JSON.decode!(result)

      {:ok, inverted} = Tarnish.Step.invert(doc, steps)
      {:ok, undone} = Tarnish.Step.apply(changed, inverted)
      assert Tarnish.Node.to_json(undone) === start
    end
  end

  defp doc(schema, json) do
    {:ok, doc} = Tarnish.Node.from_json(schema, json)
    doc
  end

  describe "schemas" do
    test "take their types as {name, spec} pairs, in order", _ do
      spec = %{nodes: [doc: %{content: "text*"}, text: %{}], marks: [strong: %{}, em: %{}]}
      {:ok, schema} = Tarnish.Schema.new(spec)

      text = %{
        "type" => "text",
        "text" => "a",
        "marks" => [%{"type" => "em"}, %{"type" => "strong"}]
      }

      doc = doc(schema, %{"type" => "doc", "content" => [text]})
      in_order = %{text | "marks" => [%{"type" => "strong"}, %{"type" => "em"}]}
      assert Tarnish.Node.to_json(doc) === %{"type" => "doc", "content" => [in_order]}
    end

    test "refuse types in a map, which loses their order", _ do
      assert_raise ArgumentError, fn -> Tarnish.Schema.new(%{"nodes" => %{"doc" => %{}}}) end
    end
  end

  describe "errors" do
    setup %{schemas: [schema | _]}, do: %{schema: schema}

    test "a schema without its top node", _ do
      assert Tarnish.Schema.new(%{"nodes" => [{"text", %{}}]}) ===
               {:error, {:range_error, "Schema is missing its top node type ('doc')"}}
    end

    test "a content expression that doesn't parse", _ do
      spec = %{"nodes" => [{"doc", %{"content" => "paragraph+"}}, {"text", %{}}]}

      assert Tarnish.Schema.new(spec) ===
               {:error,
                {:syntax_error,
                 "No node type or group 'paragraph' found (in content expression 'paragraph+')"}}
    end

    test "a document that doesn't fit the schema", %{schema: schema} do
      doc = doc(schema, %{"type" => "doc", "content" => [%{"type" => "text", "text" => "loose"}]})

      assert Tarnish.Node.check(doc) ===
               {:error, {:range_error, "Invalid content for node doc: <\"loose\">"}}
    end

    test "a step that doesn't apply", %{schema: schema} do
      doc = doc(schema, %{"type" => "doc", "content" => [%{"type" => "paragraph"}]})
      step = %{"stepType" => "replace", "from" => 0, "to" => 1}

      assert Tarnish.Step.apply(doc, [step]) ===
               {:error, {:transform_error, "Inconsistent open depths"}}
    end

    test "a term that isn't JSON", %{schema: schema} do
      assert_raise ArgumentError, fn -> Tarnish.Node.from_json(schema, {:not, :json}) end
      text = %{"type" => "text", "text" => {:not, :json}}
      json = %{"type" => "doc", "content" => [%{"type" => "paragraph", "content" => [text]}]}
      assert_raise ArgumentError, fn -> Tarnish.Node.from_json(schema, json) end
    end
  end

  describe "calls with more work than a normal scheduler takes" do
    setup %{schemas: [schema | _]}, do: %{schema: schema}

    defp paragraphs(count, text) do
      paragraph = %{"type" => "paragraph", "content" => [%{"type" => "text", "text" => text}]}
      %{"type" => "doc", "content" => List.duplicate(paragraph, count)}
    end

    test "go to a dirty scheduler, with the same answers", %{schema: schema} do
      for json <- [paragraphs(10_000, "many"), paragraphs(1, String.duplicate("long ", 200_000))] do
        assert @native.node_from_json(schema, json) === :dirty
        doc = doc(schema, json)
        assert Tarnish.Node.to_json(doc) === json
        assert Tarnish.Node.check(doc) === :ok
      end
    end

    test "write JSON on a dirty scheduler", %{schema: schema} do
      # Two texts with the same marks are read as one, so every paragraph is written anew.
      split = %{"type" => "paragraph", "content" => [text("ma"), text("ny")]}
      doc = doc(schema, %{"type" => "doc", "content" => List.duplicate(split, 10_000)})
      assert @native.to_json(doc.ref, doc.json) === :dirty
      assert Tarnish.Node.to_json(doc) === paragraphs(10_000, "many")
    end

    test "apply many steps on a dirty scheduler", %{schema: schema} do
      doc = doc(schema, paragraphs(200, "text"))
      slice = %{"content" => [%{"type" => "text", "text" => "x"}]}

      steps =
        List.duplicate(%{"stepType" => "replace", "from" => 2, "to" => 2, "slice" => slice}, 200)

      assert @native.apply_steps(doc.ref, steps) === :dirty
      {:ok, applied} = Tarnish.Step.apply(doc, steps)
      %{"content" => [_ | rest]} = paragraphs(200, "text")
      first = paragraph([text("t" <> String.duplicate("x", 200) <> "ext")])
      assert Tarnish.Node.to_json(applied) === %{"type" => "doc", "content" => [first | rest]}
    end
  end

  describe "a document's JSON" do
    setup %{schemas: [schema | _]}, do: %{schema: schema}

    defp text(text, marks \\ []) do
      case marks do
        [] -> %{"type" => "text", "text" => text}
        marks -> %{"type" => "text", "text" => text, "marks" => marks}
      end
    end

    defp paragraph(content), do: %{"type" => "paragraph", "content" => content}

    defp heading(level, text),
      do: %{"type" => "heading", "attrs" => %{"level" => level}, "content" => [text(text)]}

    test "is the map it was read from, as ProseMirror writes it", %{schema: schema} do
      json = %{"type" => "doc", "content" => [heading(2, "Title"), paragraph([text("a")])]}
      assert :erts_debug.same(Tarnish.Node.to_json(doc(schema, json)), json)
    end

    test "shares the maps of the nodes steps leave", %{schema: schema} do
      [first, second, third] =
        content = for text <- ~w(one two three), do: paragraph([text(text)])

      doc = doc(schema, %{"type" => "doc", "content" => content})
      slice = %{"content" => [text("!")]}
      step = %{"stepType" => "replace", "from" => 9, "to" => 9, "slice" => slice}
      {:ok, changed} = Tarnish.Step.apply(doc, [step])
      %{"type" => "doc", "content" => [one, two, three]} = json = Tarnish.Node.to_json(changed)
      assert map_size(json) === 2
      assert :erts_debug.same(one, first) and :erts_debug.same(three, third)
      assert two === paragraph([text("two!")]) and not :erts_debug.same(two, second)
    end

    test "is what ProseMirror writes, whatever the map read", %{schema: schema} do
      em = %{"type" => "em"}
      strong = %{"type" => "strong"}

      for {read, written} <- [
            {%{"type" => :paragraph}, %{"type" => "paragraph"}},
            {%{type: "paragraph"}, %{"type" => "paragraph"}},
            {%{"type" => "paragraph", "extra" => 1}, %{"type" => "paragraph"}},
            {%{"type" => "paragraph", "attrs" => %{}}, %{"type" => "paragraph"}},
            {%{"type" => "paragraph", "content" => []}, %{"type" => "paragraph"}},
            {paragraph([%{"type" => "text", "text" => "a", "marks" => []}]),
             paragraph([text("a")])},
            {paragraph([text("a"), text("b")]), paragraph([text("ab")])},
            {paragraph([text("a", [strong, em])]), paragraph([text("a", [em, strong])])},
            {paragraph([text("a", [%{"type" => "em", "attrs" => nil}])]),
             paragraph([text("a", [em])])},
            {%{"type" => "heading", "content" => [text("a")]}, heading(1, "a")},
            {heading(1.0, "a"), heading(1, "a")},
            {heading(2, "a") |> put_in(["attrs", "extra"], 1), heading(2, "a")},
            {Jason.OrderedObject.new([{"type", "paragraph"}]), %{"type" => "paragraph"}}
          ] do
        json = %{"type" => "doc", "content" => [paragraph([text("same")]), read]}
        written = %{"type" => "doc", "content" => [paragraph([text("same")]), written]}
        assert Tarnish.Node.to_json(doc(schema, json)) === written, inspect(read)
      end
    end
  end

  # Far deeper than a dirty scheduler's stack could recurse through.
  describe "deeply nested documents" do
    setup %{schemas: [schema | _]}, do: %{schema: schema}

    @depth 100_000

    test "nest as deeply as memory allows", %{schema: schema} do
      quote =
        Enum.reduce(1..@depth, %{"type" => "paragraph"}, fn _, inner ->
          %{"type" => "blockquote", "content" => [inner]}
        end)

      json = %{"type" => "doc", "content" => [quote]}
      doc = doc(schema, json)
      assert Tarnish.Node.check(doc) === :ok
      {:ok, same} = Tarnish.Step.apply(doc, [])
      assert Tarnish.Node.to_json(same) === json
    end

    test "hold attributes that nest as deeply", _ do
      doc_spec = %{content: "text*", attrs: %{data: %{default: nil}}}
      {:ok, schema} = Tarnish.Schema.new(%{nodes: [doc: doc_spec, text: %{}]})
      data = Enum.reduce(1..@depth, [], fn _, inner -> [inner] end)
      json = %{"type" => "doc", "attrs" => %{"data" => data}}
      doc = doc(schema, json)
      assert Tarnish.Node.check(doc) === :ok
      assert Tarnish.Node.to_json(doc) === json
    end
  end
end
