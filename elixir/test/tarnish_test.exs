defmodule TarnishTest do
  use ExUnit.Case, async: true

  @native Application.compile_env(:tarnish, :native, Tarnish.Native)

  # Transforms prosemirror-transform's own tests make, recorded from the real package by
  # `npm run test:js`: a schema, a starting document, the steps, the document they give, and
  # positions mapped through them.
  @fixtures Path.expand("../../fixtures/transform.json", __DIR__)
  @external_resource @fixtures

  # What the real ProseMirror packages make of inputs their own tests don't give them, and of
  # the transforms above, recorded by `npm run test:js` too.
  @model Path.expand("../../fixtures/model.json", __DIR__)
  @external_resource @model

  @kinds %{
    "RangeError" => :range_error,
    "SyntaxError" => :syntax_error,
    "ReplaceError" => :replace_error,
    "TransformError" => :transform_error,
    "TypeError" => :type_error,
    "Error" => :js_error
  }

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

  defp expected(%{"result" => result}), do: {:ok, result}

  defp expected(%{"error" => %{"class" => class, "message" => message}}),
    do: {:error, {Map.fetch!(@kinds, class), message}}

  # Where each position maps through the steps with each assoc, as ProseMirror maps it.
  defp assert_mapped(schema, steps, mapped) do
    for %{"assoc" => assoc, "pos" => [_ | _] = positions} <- mapped do
      given =
        for pos <- 0..(length(positions) - 1), do: Tarnish.Mapping.map(schema, steps, pos, assoc)

      assert given === Enum.map(positions, &{:ok, &1}), "assoc #{assoc}"
    end
  end

  # A section with no records would make no tests, so each must have some.
  %{"tests" => [_ | _] = tests} = @fixtures |> File.read!() |> Jason.decode!()

  %{
    "fromJSON" => [_ | _] = from_json,
    "check" => [_ | _] = check,
    "transforms" => [_ | _] = transforms,
    "transformTests" => [_ | _] = transform_tests
  } = @model |> File.read!() |> Jason.decode!()

  # A spec's types are in order.
  [_ | _] = new_schema = Jason.decode!(File.read!(@model), objects: :ordered_objects)["newSchema"]

  if length(tests) != length(transform_tests), do: raise("a test without its record")

  for {{fixture, recorded}, index} <- Enum.with_index(Enum.zip(tests, transform_tests)) do
    @fixture fixture
    @recorded recorded
    @index index
    test "transform #{index} gives the document, inverse and mapping ProseMirror gives",
         %{schemas: schemas} do
      %{"schema" => schema, "start" => start, "steps" => steps, "result" => result} = @fixture
      %{"test" => @index, "inverted" => inverted, "mapped" => mapped} = @recorded
      schema = Enum.at(schemas, schema)
      {:ok, doc} = Tarnish.Node.from_json(schema, start)

      {:ok, changed} = Tarnish.Step.apply(doc, steps)
      assert Tarnish.Node.to_json(changed) === result

      assert Tarnish.Step.invert(doc, steps) === {:ok, inverted}
      {:ok, undone} = Tarnish.Step.apply(changed, inverted)
      assert Tarnish.Node.to_json(undone) === start

      assert_mapped(schema, steps, mapped)
    end
  end

  for {fixture, index} <- Enum.with_index(from_json) do
    @fixture fixture
    test "node #{index} from JSON is the one ProseMirror reads", %{model_schemas: schemas} do
      schema = Enum.at(schemas, @fixture["schema"])
      json = if text = @fixture["text"], do: Jason.decode!(text), else: @fixture["json"]

      answer =
        with {:ok, doc} <- Tarnish.Node.from_json(schema, json),
             do: {:ok, Tarnish.Node.to_json(doc)}

      assert answer === expected(@fixture)
    end
  end

  @from_json from_json
  test "terms JSON doesn't have, atoms and ordered objects, read as the JSON they stand for",
       %{model_schemas: schemas} do
    same = %{"type" => "paragraph", "content" => [%{"type" => "text", "text" => "same"}]}
    doc = &%{"type" => "doc", "content" => [same, &1]}
    twin = doc.(%{"type" => "paragraph"})

    %{"result" => written} =
      Enum.find(@from_json, &(&1["text"] && Jason.decode!(&1["text"]) === twin))

    for term <- [
          %{"type" => :paragraph},
          %{type: "paragraph"},
          Jason.OrderedObject.new([{"type", "paragraph"}])
        ] do
      {:ok, read} = Tarnish.Node.from_json(Enum.at(schemas, 5), doc.(term))
      assert Tarnish.Node.to_json(read) === written, inspect(term)
    end
  end

  for {fixture, index} <- Enum.with_index(new_schema) do
    @fixture fixture
    test "schema #{index} is ProseMirror's, or its error" do
      case {Tarnish.Schema.new(@fixture["spec"]), @fixture["error"]} do
        {answer, nil} ->
          assert {:ok, schema} = answer
          assert is_reference(schema)

        {answer, error} ->
          assert answer === {:error, {Map.fetch!(@kinds, error["class"]), error["message"]}}
      end
    end
  end

  for {fixture, index} <- Enum.with_index(check) do
    @fixture fixture
    test "check #{index} is ProseMirror's", %{model_schemas: schemas} do
      schema = Enum.at(schemas, @fixture["schema"])
      expected = with {:ok, nil} <- expected(@fixture), do: :ok

      checked =
        with {:ok, doc} <- Tarnish.Node.from_json(schema, @fixture["json"]),
             do: Tarnish.Node.check(doc)

      assert checked === expected
    end
  end

  for {fixture, index} <- Enum.with_index(transforms) do
    @fixture fixture
    test "steps #{index} give ProseMirror's text, U+FFFD for each lone surrogate, or its error",
         %{model_schemas: schemas} do
      %{"schema" => schema, "start" => start, "steps" => steps} = @fixture
      schema = Enum.at(schemas, schema)
      {:ok, doc} = Tarnish.Node.from_json(schema, start)

      case @fixture do
        %{"result" => result, "inverted" => inverted, "mapped" => mapped} ->
          {:ok, changed} = Tarnish.Step.apply(doc, steps)
          written = Tarnish.JSON.decode!(result)
          assert written === Map.get(@fixture, "wellFormed", written)
          assert Tarnish.Node.to_json(changed) === written
          assert Tarnish.Step.invert(doc, steps) === {:ok, inverted}
          assert_mapped(schema, steps, mapped)

        %{"error" => _} ->
          assert Tarnish.Step.apply(doc, steps) === expected(@fixture)
      end
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
