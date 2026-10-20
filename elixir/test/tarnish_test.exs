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
      for {spec, key} <- [
            {%{"nodes" => %{"doc" => %{}, "text" => %{}}}, "nodes"},
            {%{"nodes" => [{"doc", %{}}, {"text", %{}}], "marks" => %{"em" => %{}}}, "marks"}
          ] do
        error = assert_raise ArgumentError, fn -> Tarnish.Schema.new(spec) end

        assert error.message ===
                 "#{key} must be a list of {name, spec} pairs, since a map doesn't keep their order"
      end
    end
  end

  test "a term that isn't JSON is refused", %{schemas: [schema | _]} do
    assert_raise ArgumentError, fn -> Tarnish.Node.from_json(schema, {:not, :json}) end
    text = %{"type" => "text", "text" => {:not, :json}}
    json = %{"type" => "doc", "content" => [%{"type" => "paragraph", "content" => [text]}]}
    assert_raise ArgumentError, fn -> Tarnish.Node.from_json(schema, json) end
  end

  defp text(text, marks \\ []) do
    case marks do
      [] -> %{"type" => "text", "text" => text}
      marks -> %{"type" => "text", "text" => text, "marks" => marks}
    end
  end

  defp paragraph(content), do: %{"type" => "paragraph", "content" => content}

  defp paragraphs(count, text),
    do: %{"type" => "doc", "content" => List.duplicate(paragraph([text(text)]), count)}

  # Each call is made on a normal scheduler first, which answers `:dirty` past what it takes on,
  # for the call to be made again on a dirty scheduler.
  test "each call goes to a dirty scheduler past a normal one's limits, with the same answers",
       %{schemas: [schema | _]} do
    long = String.duplicate("long ", 200_000)
    small = doc(schema, paragraphs(2, "few"))
    many = doc(schema, paragraphs(200, "text"))
    lengthy = doc(schema, paragraphs(1, long))
    # Two texts with the same marks are read as one, so every paragraph is written anew.
    split = paragraph([text("ma"), text("ny")])
    split = doc(schema, %{"type" => "doc", "content" => List.duplicate(split, 10_000)})
    loose = doc(schema, %{"type" => "doc", "content" => [text(long)]})

    x = [text("x")]
    insert = %{"stepType" => "replace", "from" => 2, "to" => 2, "slice" => %{"content" => x}}
    inserts = List.duplicate(insert, 200)
    %{"content" => [_ | rest]} = paragraphs(200, "text")
    first = paragraph([text("t" <> String.duplicate("x", 200) <> "ext")])
    inserted = %{"type" => "doc", "content" => [first | rest]}
    deletes = List.duplicate(%{"stepType" => "replace", "from" => 2, "to" => 3}, 200)
    ops = List.duplicate(%{"op" => "insert", "pos" => 2, "content" => x}, 200)

    strong = %{"type" => "strong"}
    whole = [%{"op" => "addMark", "from" => 0, "to" => 1200, "mark" => strong}]

    marks =
      for from <- 1..1195//6,
          do: %{"stepType" => "addMark", "mark" => strong, "from" => from, "to" => from + 4}

    marked = List.duplicate(paragraph([text("text", [strong])]), 200)
    marked = %{"type" => "doc", "content" => marked}

    transformed = fn ops ->
      {:ok, %Tarnish.Transform{doc: changed, steps: steps}} = Tarnish.Transform.new(many, ops)
      {Tarnish.Node.to_json(changed), steps}
    end

    applied = fn steps ->
      {:ok, changed} = Tarnish.Step.apply(many, steps)
      Tarnish.Node.to_json(changed)
    end

    # The light call, a big input and a small one, and the public call and its answer for the
    # big input.
    for {name, light, big, little, public, expected} <- [
          {"node_from_json of many nodes", &@native.node_from_json(schema, &1),
           paragraphs(10_000, "many"), paragraphs(2, "few"),
           &Tarnish.Node.to_json(doc(schema, &1)), paragraphs(10_000, "many")},
          {"node_from_json of a long text", &@native.node_from_json(schema, &1),
           paragraphs(1, long), paragraphs(2, "few"), &Tarnish.Node.to_json(doc(schema, &1)),
           paragraphs(1, long)},
          {"to_json", &@native.to_json(&1.ref, &1.json), split, small, &Tarnish.Node.to_json/1,
           paragraphs(10_000, "many")},
          {"check", &@native.check(&1.ref), loose, small, &Tarnish.Node.check/1,
           {:error,
            {:range_error,
             "Invalid content for node doc: <\"long long long long long long long long long lon"}}},
          {"apply_steps", &@native.apply_steps(many.ref, &1), inserts, [insert], applied,
           inserted},
          {"invert_steps", &@native.invert_steps(many.ref, &1), inserts, [insert],
           &Tarnish.Step.invert(many, &1), {:ok, deletes}},
          {"map_position", &@native.map_position(schema, &1, 5, 1),
           List.duplicate(insert, 20_000), [insert], &Tarnish.Mapping.map(schema, &1, 5, 1),
           {:ok, 20_005}},
          {"transform of many ops", &@native.transform(many.ref, &1), ops, [hd(ops)], transformed,
           {inserted, inserts}},
          {"transform over a wide range", &@native.transform(many.ref, &1), whole,
           [%{hd(whole) | "to" => 6}], transformed, {marked, marks}},
          {"text_between", &@native.text_between(&1.ref, 0, 1_000_002, "\n", nil), lengthy, small,
           &Tarnish.Node.text_between(&1, 0, 1_000_002, "\n"), {:ok, long}},
          {"text_content", &@native.text_content(&1.ref), lengthy, small,
           &Tarnish.Node.text_content/1, long}
        ] do
      assert light.(big) === :dirty, name
      refute light.(little) === :dirty, name
      assert public.(big) === expected, name
    end
  end

  describe "a document's JSON" do
    setup %{schemas: [schema | _]}, do: %{schema: schema}

    defp heading(level, text),
      do: %{"type" => "heading", "attrs" => %{"level" => level}, "content" => [text(text)]}

    test "is the map it was read from, as ProseMirror writes it", %{schema: schema} do
      json = %{"type" => "doc", "content" => [heading(2, "Title"), paragraph([text("a")])]}
      assert :erts_debug.same(Tarnish.Node.to_json(doc(schema, json)), json)
    end

    test "shares the maps of the nodes steps and ops leave", %{schema: schema} do
      [first, second, third] =
        content = for text <- ~w(one two three), do: paragraph([text(text)])

      doc = doc(schema, %{"type" => "doc", "content" => content})
      em = %{"type" => "em"}

      step = %{
        "stepType" => "replace",
        "from" => 9,
        "to" => 9,
        "slice" => %{"content" => [text("!")]}
      }

      op = %{"op" => "addMark", "from" => 6, "to" => 9, "mark" => em}

      transformed = fn doc ->
        with {:ok, transform} <- Tarnish.Transform.new(doc, [op]), do: {:ok, transform.doc}
      end

      for {change, two} <- [
            {&Tarnish.Step.apply(&1, [step]), paragraph([text("two!")])},
            {transformed, paragraph([text("two", [em])])}
          ] do
        {:ok, changed} = change.(doc)

        %{"type" => "doc", "content" => [one, changed_two, three]} =
          json = Tarnish.Node.to_json(changed)

        assert map_size(json) === 2
        assert :erts_debug.same(one, first) and :erts_debug.same(three, third)
        assert changed_two === two
        refute :erts_debug.same(changed_two, second)
      end
    end

    defp item(content), do: %{"type" => "list_item", "content" => content}
    defp list(items), do: %{"type" => "bullet_list", "content" => items}

    defp insert(at, text),
      do: %{
        "stepType" => "replace",
        "from" => at,
        "to" => at,
        "slice" => %{"content" => [text(text)]}
      }

    test "shares the map of every node steps leave, at every depth", %{schema: schema} do
      item = &item([paragraph([text(&1)])])
      [b1, b2, b3] = for name <- ~w(b1 b2 b3), do: item.(name)
      inner = list([b1, b2, b3])
      a2_text = paragraph([text("a2")])
      [a1, a3] = for name <- ~w(a1 a3), do: item.(name)
      a2 = item([a2_text, inner])
      [c1, c2, c3] = for name <- ~w(c1 c2 c3), do: item.(name)
      last = paragraph([text("end")])
      json = %{"type" => "doc", "content" => [list([a1, a2, a3]), list([c1, c2, c3]), last]}
      doc = doc(schema, json)

      # "!" after "c2", then after "b2": two lists side by side change, one of them two levels
      # down.
      {:ok, changed} = Tarnish.Step.apply(doc, [insert(51, "!"), insert(23, "!")])

      %{"content" => [%{"content" => [one, two, three]}, %{"content" => [four, five, six]}, end_]} =
        written = Tarnish.Node.to_json(changed)

      assert written ===
               %{
                 "type" => "doc",
                 "content" => [
                   list([a1, item([a2_text, list([b1, item.("b2!"), b3])]), a3]),
                   list([c1, item.("c2!"), c3]),
                   last
                 ]
               }

      %{"content" => [two_text, %{"content" => [seven, _, eight]}]} = two

      for {written, read} <- [
            {end_, last},
            {one, a1},
            {three, a3},
            {four, c1},
            {six, c3},
            {two_text, a2_text},
            {seven, b1},
            {eight, b3}
          ] do
        assert :erts_debug.same(written, read)
      end

      refute :erts_debug.same(five, c2)

      # Splitting the item after "a2" leaves its list in the second half.
      split = %{
        "stepType" => "replace",
        "from" => 11,
        "to" => 11,
        "slice" => %{
          "content" => [item([paragraph([])]), item([paragraph([])])],
          "openStart" => 2,
          "openEnd" => 2
        },
        "structure" => true
      }

      {:ok, changed} = Tarnish.Step.apply(doc, [split])

      %{
        "content" => [%{"content" => [one, _, %{"content" => [_, two_inner]}, three]}, four, end_]
      } =
        written = Tarnish.Node.to_json(changed)

      assert written ===
               put_in(
                 json,
                 ["content", Access.at(0)],
                 list([a1, item([a2_text]), item([%{"type" => "paragraph"}, inner]), a3])
               )

      %{"content" => [_, c_list, _]} = json

      for {written, read} <- [
            {one, a1},
            {three, a3},
            {two_inner, inner},
            {four, c_list},
            {end_, last}
          ] do
        assert :erts_debug.same(written, read)
      end
    end
  end
end
