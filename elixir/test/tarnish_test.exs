defmodule TarnishTest do
  use ExUnit.Case, async: true

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
      {:ok, schema} = Tarnish.schema(spec)
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
      {:ok, doc} = Tarnish.node_from_json(schema, start)

      {:ok, changed} = Tarnish.apply_steps(doc, steps)
      assert Tarnish.to_json(changed) == result

      {:ok, inverted} = Tarnish.invert_steps(doc, steps)
      {:ok, undone} = Tarnish.apply_steps(changed, inverted)
      assert Tarnish.to_json(undone) == start

      for [from, to] <- @fixture["mapping"] do
        assert Tarnish.map_position(schema, steps, from, 1) == {:ok, to}
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

      case Tarnish.node_from_json(schema, @fixture["json"]) do
        {:ok, doc} ->
          assert Tarnish.to_json(doc) == @fixture["result"]

        {:error, {kind, message}} ->
          %{"class" => class, "message" => expected} = @fixture["error"]
          assert {kind, message} == {@kinds[class], expected}
      end
    end
  end

  for {fixture, index} <- Enum.with_index(model["transforms"]) do
    @fixture fixture
    test "a step splitting a surrogate pair leaves U+FFFD for each half, #{index}",
         %{model_schemas: schemas} do
      %{"schema" => schema, "start" => start, "steps" => steps, "result" => result} = @fixture
      {:ok, doc} = Tarnish.node_from_json(Enum.at(schemas, schema), start)
      {:ok, changed} = Tarnish.apply_steps(doc, steps)
      # JSON.stringify escapes a surrogate only when it's alone.
      replaced = Regex.replace(~r/\\ud[89a-f][0-9a-f]{2}/i, result, "\\ufffd")
      assert Tarnish.to_json(changed) == Jason.decode!(replaced)

      {:ok, inverted} = Tarnish.invert_steps(doc, steps)
      {:ok, undone} = Tarnish.apply_steps(changed, inverted)
      assert Tarnish.to_json(undone) == start
    end
  end

  defp doc(schema, json) do
    {:ok, doc} = Tarnish.node_from_json(schema, json)
    doc
  end

  describe "schemas" do
    test "take their types as {name, spec} pairs, in order", _ do
      spec = %{nodes: [doc: %{content: "text*"}, text: %{}], marks: [strong: %{}, em: %{}]}
      {:ok, schema} = Tarnish.schema(spec)

      text = %{
        "type" => "text",
        "text" => "a",
        "marks" => [%{"type" => "em"}, %{"type" => "strong"}]
      }

      doc = doc(schema, %{"type" => "doc", "content" => [text]})

      assert %{"content" => [%{"marks" => [%{"type" => "strong"}, %{"type" => "em"}]}]} =
               Tarnish.to_json(doc)
    end

    test "refuse types in a map, which loses their order", _ do
      assert_raise ArgumentError, fn -> Tarnish.schema(%{"nodes" => %{"doc" => %{}}}) end
    end
  end

  describe "errors" do
    setup %{schemas: [schema | _]}, do: %{schema: schema}

    test "a schema without its top node", _ do
      assert Tarnish.schema(%{"nodes" => [{"text", %{}}]}) ==
               {:error, {:range_error, "Schema is missing its top node type ('doc')"}}
    end

    test "a content expression that doesn't parse", _ do
      spec = %{"nodes" => [{"doc", %{"content" => "paragraph+"}}, {"text", %{}}]}

      assert Tarnish.schema(spec) ==
               {:error,
                {:syntax_error,
                 "No node type or group 'paragraph' found (in content expression 'paragraph+')"}}
    end

    test "a document that doesn't fit the schema", %{schema: schema} do
      doc = doc(schema, %{"type" => "doc", "content" => [%{"type" => "text", "text" => "loose"}]})
      assert {:error, {:range_error, "Invalid content for node doc" <> _}} = Tarnish.check(doc)
    end

    test "a step that doesn't apply", %{schema: schema} do
      doc = doc(schema, %{"type" => "doc", "content" => [%{"type" => "paragraph"}]})
      step = %{"stepType" => "replace", "from" => 0, "to" => 1}
      assert {:error, {:transform_error, _}} = Tarnish.apply_steps(doc, [step])
    end

    test "a term that isn't JSON", %{schema: schema} do
      assert_raise ArgumentError, fn -> Tarnish.node_from_json(schema, {:not, :json}) end
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
      assert Tarnish.check(doc) == :ok
      {:ok, same} = Tarnish.apply_steps(doc, [])
      assert Tarnish.to_json(same) == json
    end

    test "hold attributes that nest as deeply", _ do
      doc_spec = %{content: "text*", attrs: %{data: %{default: nil}}}
      {:ok, schema} = Tarnish.schema(%{nodes: [doc: doc_spec, text: %{}]})
      data = Enum.reduce(1..@depth, [], fn _, inner -> [inner] end)
      json = %{"type" => "doc", "attrs" => %{"data" => data}}
      doc = doc(schema, json)
      assert Tarnish.check(doc) == :ok
      assert Tarnish.to_json(doc) == json
    end
  end
end
