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
        {:ok, schema} = Tarnish.Schema.new(spec)
        schema
      end

    %{schemas: schemas}
  end

  # A section with no records would make no tests, so each must have some.
  %{
    "transforms" => [_ | _] = transforms,
    "textBetween" => [_ | _] = text_between,
    "textsBetween" => [_ | _] = texts_between,
    "textContent" => [_ | _] = text_content
  } = @fixtures |> File.read!() |> Jason.decode!()

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
  defp expected(text) when is_binary(text), do: {:ok, text}

  for {fixture, index} <- Enum.with_index(transforms) do
    @fixture fixture
    test "op list #{index} gives what ProseMirror gives", %{schemas: schemas} do
      doc = doc(schemas, @fixture)

      transformed =
        with {:ok, %Tarnish.Transform{doc: changed, steps: steps}} <-
               Tarnish.Transform.new(doc, @fixture["ops"]),
             do: {:ok, Tarnish.Node.to_json(changed), steps}

      assert transformed === expected(@fixture)

      # Where each position maps through the steps, which are mapped where they read back from
      # their JSON.
      schema = Enum.at(schemas, @fixture["schema"])

      for %{"assoc" => assoc, "pos" => [_ | _] = positions} <- Map.get(@fixture, "mapped", []) do
        mapped =
          for pos <- 0..(length(positions) - 1),
              do: Tarnish.Mapping.map(schema, @fixture["steps"], pos, assoc)

        assert mapped === Enum.map(positions, &{:ok, &1}), "assoc #{assoc}"
      end
    end
  end

  for {fixture, index} <- Enum.with_index(text_between) do
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

  for {fixture, index} <- Enum.with_index(texts_between) do
    @fixture fixture
    test "textBetween every two positions #{index} is ProseMirror's", %{schemas: schemas} do
      doc = doc(schemas, @fixture)
      %{"texts" => [_ | _] = texts} = @fixture
      separator = @fixture["blockSeparator"]
      leaf = @fixture["leafText"]

      for {from_here, from} <- Enum.with_index(texts) do
        given =
          for to <- from..(from + length(from_here) - 1),
              do: Tarnish.Node.text_between(doc, from, to, separator, leaf)

        assert given === Enum.map(from_here, &expected/1), "from #{from}"
      end
    end
  end

  for {fixture, index} <- Enum.with_index(text_content) do
    @fixture fixture
    test "textContent #{index} is ProseMirror's", %{schemas: schemas} do
      assert Tarnish.Node.text_content(doc(schemas, @fixture)) === @fixture["result"]
    end
  end

  test "a transform's document is the document given when there are no ops", %{
    schemas: [schema | _]
  } do
    paragraph = %{"type" => "paragraph", "content" => [%{"type" => "text", "text" => "same"}]}
    {:ok, doc} = Tarnish.Node.from_json(schema, %{"type" => "doc", "content" => [paragraph]})
    assert Tarnish.Transform.new(doc, []) === {:ok, %Tarnish.Transform{doc: doc, steps: []}}
  end
end
