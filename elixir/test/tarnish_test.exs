defmodule TarnishTest do
  use ExUnit.Case, async: true

  # Every transform prosemirror-transform's own tests make, recorded from the real package by
  # `npm run fixtures`: a schema, a starting document, the steps, the document they give, and
  # positions mapped through them.
  @fixtures Path.expand("../../fixtures/transform.json", __DIR__)
  @external_resource @fixtures

  # A schema's node and mark types are in order, so its spec is read keeping the order of
  # "nodes" and "marks", as lists of {name, spec} pairs.
  defp schema_spec(%Jason.OrderedObject{values: values}) do
    Map.new(values, fn
      {key, %Jason.OrderedObject{values: types}} when key in ["nodes", "marks"] ->
        {key, Enum.map(types, fn {name, spec} -> {name, plain(spec)} end)}

      {key, value} ->
        {key, plain(value)}
    end)
  end

  defp plain(%Jason.OrderedObject{values: values}),
    do: Map.new(values, fn {key, value} -> {key, plain(value)} end)

  defp plain(list) when is_list(list), do: Enum.map(list, &plain/1)
  defp plain(other), do: other

  setup_all do
    specs = @fixtures |> File.read!() |> Jason.decode!(objects: :ordered_objects)

    schemas =
      for spec <- specs["schemas"] do
        {:ok, schema} = Tarnish.schema(schema_spec(spec))
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

  # A dirty scheduler's stack is far too small for these; each call runs on a stack of its own.
  describe "deeply nested documents" do
    setup %{schemas: [schema | _]}, do: %{schema: schema}

    defp nested(depth) do
      quote =
        Enum.reduce(1..depth, %{"type" => "paragraph"}, fn _, inner ->
          %{"type" => "blockquote", "content" => [inner]}
        end)

      %{"type" => "doc", "content" => [quote]}
    end

    test "convert far deeper than JavaScript's stack takes them", %{schema: schema} do
      doc = nested(100_000)
      assert Tarnish.apply_steps(schema, doc, []) == {:ok, doc}
    end

    test "throw, as JavaScript does, past the stack", %{schema: schema} do
      assert Tarnish.apply_steps(schema, nested(1_000_000), []) ==
               {:error, {:range_error, "Maximum call stack size exceeded"}}
    end
  end
end
