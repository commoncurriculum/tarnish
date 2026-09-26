defmodule TarnishTest do
  use ExUnit.Case, async: true

  # Every transform prosemirror-transform's own tests make, recorded from the real package by
  # `npm run fixtures`: a schema, a starting document, the steps, the document they give, and
  # positions mapped through them.
  @fixtures Path.expand("../../fixtures/transform.json", __DIR__)
  @external_resource @fixtures

  # A schema's node and mark types are in order, so the specs are read keeping their keys'
  # order, as lists of pairs.
  defp ordered(%Jason.OrderedObject{values: []}), do: %{}

  defp ordered(%Jason.OrderedObject{values: values}),
    do: Enum.map(values, fn {key, value} -> {key, ordered(value)} end)

  defp ordered(list) when is_list(list), do: Enum.map(list, &ordered/1)
  defp ordered(other), do: other

  setup_all do
    specs = @fixtures |> File.read!() |> Jason.decode!(objects: :ordered_objects)

    schemas =
      for spec <- specs["schemas"] do
        {:ok, schema} = Tarnish.schema(ordered(spec))
        schema
      end

    %{schemas: schemas}
  end

  fixtures = @fixtures |> File.read!() |> Jason.decode!()

  for {fixture, index} <- Enum.with_index(fixtures["tests"]) do
    @fixture fixture
    test "transform #{index} gives the document ProseMirror gives", %{schemas: schemas} do
      %{"schema" => schema, "start" => start, "steps" => steps, "result" => result} = @fixture
      schema = Enum.at(schemas, schema)

      assert Tarnish.apply_steps(schema, start, steps) == {:ok, result}

      {:ok, inverted} = Tarnish.invert_steps(schema, start, steps)
      assert Tarnish.apply_steps(schema, result, inverted) == {:ok, start}

      for [from, to] <- @fixture["mapping"] do
        assert Tarnish.map_position(schema, steps, from, 1) == {:ok, to}
      end
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
      doc = %{"type" => "doc", "content" => [%{"type" => "text", "text" => "loose"}]}

      assert {:error, {:range_error, "Invalid content for node doc" <> _}} =
               Tarnish.check(schema, doc)
    end

    test "a step that doesn't apply", %{schema: schema} do
      doc = %{"type" => "doc", "content" => [%{"type" => "paragraph"}]}
      step = %{"stepType" => "replace", "from" => 0, "to" => 1}

      assert {:error, {:transform_error, _}} = Tarnish.apply_steps(schema, doc, [step])
    end

    test "a term that isn't JSON", %{schema: schema} do
      assert_raise ArgumentError, fn -> Tarnish.check(schema, {:not, :json}) end
    end
  end
end
