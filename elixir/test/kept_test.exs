defmodule Tarnish.KeptTest do
  use ExUnit.Case, async: true

  # A node written with Node.to_json_kept keeps the JSON written, which the JSON of the nodes
  # changes make of it then shares: what it gives must be what Node.to_json gives a node that
  # keeps no JSON but the one it was read from, term for term.

  @native Application.compile_env(:tarnish, :native, Tarnish.Native)

  @fixtures Path.expand("../../fixtures/transform.json", __DIR__)
  @external_resource @fixtures

  # The documents `npm run bench` writes, when it has.
  @bench Path.expand("../../target/bench", __DIR__)

  defp ordered(path), do: path |> File.read!() |> Jason.decode!(objects: :ordered_objects)

  setup_all do
    # prosemirror-schema-basic with prosemirror-schema-list.
    {:ok, schema} =
      @fixtures |> ordered() |> Access.get("schemas") |> hd() |> Tarnish.Schema.new()

    %{schema: schema}
  end

  defp doc(schema, json) do
    {:ok, doc} = Tarnish.Node.from_json(schema, json)
    doc
  end

  defp text(text, marks \\ []) do
    case marks do
      [] -> %{"type" => "text", "text" => text}
      marks -> %{"type" => "text", "text" => text, "marks" => marks}
    end
  end

  defp paragraph(content), do: %{"type" => "paragraph", "content" => content}
  defp item(content), do: %{"type" => "list_item", "content" => content}

  defp insert(at, text),
    do: %{
      "stepType" => "replace",
      "from" => at,
      "to" => at,
      "slice" => %{"content" => [text(text)]}
    }

  # What the node's JSON must be: what a node with no JSON kept but the one read writes.
  defp fresh(node), do: Tarnish.Node.to_json(Map.put(node, :base, nil))

  # Writes the node with Node.to_json_kept, which must give what a fresh write gives, and the
  # node that keeps it.
  defp kept(node) do
    {json, kept} = Tarnish.Node.to_json_kept(node)
    expected = fresh(node)
    assert json === expected

    assert :erlang.term_to_binary(json, [:deterministic]) ===
             :erlang.term_to_binary(expected, [:deterministic])

    assert Tarnish.Node.to_json(kept) === json
    {json, kept}
  end

  # The size of a document's JSON, as positions.
  defp size(%{"type" => "text", "text" => text}), do: length(String.to_charlist(text))
  defp size(%{"content" => content}), do: 2 + Enum.sum(Enum.map(content, &size/1))
  defp size(_leaf), do: 1

  defp content_size(json), do: size(json) - 2

  # The positions where typing "x" applies, in order.
  defp places(node) do
    for at <- 0..content_size(Tarnish.Node.to_json(node)),
        match?({:ok, _}, Tarnish.Step.apply(node, [insert(at, "x")])),
        do: at
  end

  # 100 messages typing "x" in a place each, spread evenly, each into the node the message before
  # it made and wrote, and each checked; then the node the last made.
  defp chain(node, count \\ 100) do
    places = places(node)
    {_, node} = kept(node)

    {node, _} =
      Enum.reduce(0..(count - 1), {node, []}, fn index, {before, typed_at} ->
        place = Enum.at(places, div(index * length(places), count))
        steps = [insert(place + Enum.count(typed_at, &(&1 < place)), "x")]
        {:ok, changed} = Tarnish.Step.apply(before, steps)
        {_, written} = kept(changed)
        {written, [place | typed_at]}
      end)

    node
  end

  describe "a kept node's JSON" do
    test "is what Node.to_json gives, message after message, on the bench's documents", %{
      schema: schema
    } do
      for {file, schema_for} <- [
            {"document.json", fn _ -> schema end},
            {"nested.json", &own_schema/1},
            {"cards.json", &own_schema/1}
          ],
          path = Path.join(@bench, file),
          File.exists?(path) do
        file = Jason.decode!(File.read!(path))
        node = doc(schema_for.(path), file["doc"])
        chain(node)
      end
    end

    # The card stack TARNISH_BENCH_DOC names, read here and never kept in the repo. Its changes
    # each apply to the document as read, so each is one message from the kept node, and one
    # more from where a chain of messages left it, where those that fail must fail as they do on
    # a node that keeps no JSON.
    test "is what Node.to_json gives on TARNISH_BENCH_DOC's card stack" do
      with path when is_binary(path) <- System.get_env("TARNISH_BENCH_DOC"),
           true <- File.exists?(path) do
        file = Jason.decode!(File.read!(path))

        schema =
          schema(ordered(path)["schema"] || ordered(System.fetch_env!("TARNISH_BENCH_SCHEMA")))

        node = doc(schema, file["doc"])
        {_, read} = kept(node)
        chained = chain(node)

        for %{"steps" => steps} <- file["changes"], from <- [read, chained] do
          case Tarnish.Step.apply(from, steps) do
            {:ok, changed} -> kept(changed)
            error -> assert error === Tarnish.Step.apply(Map.put(from, :base, nil), steps)
          end
        end
      end
    end

    test "is the JSON written last, when no step has changed the node", %{schema: schema} do
      node = doc(schema, %{"type" => "doc", "content" => [paragraph([text("a")])]})
      {:ok, changed} = Tarnish.Step.apply(node, [insert(1, "b")])
      {json, kept} = kept(changed)
      {again, _} = Tarnish.Node.to_json_kept(kept)
      assert :erts_debug.same(again, json)
      {:ok, unchanged} = Tarnish.Step.apply(kept, [])
      assert :erts_debug.same(Tarnish.Node.to_json(unchanged), json)
    end

    test "shares the terms the message before wrote for the nodes this one left", %{
      schema: schema
    } do
      content = for text <- ~w(one two three), do: paragraph([text(text)])
      [_, read_two, _] = content
      node = doc(schema, %{"type" => "doc", "content" => content})

      # A long text after "one", then "!" after "three". The second change's chunk takes in the
      # first's when it is no more than twice as big (doc.rs), and holds copies of its nodes then,
      # which are written anew; the long text keeps the first chunk.
      long = String.duplicate("long ", 1000)
      {:ok, first} = Tarnish.Step.apply(node, [insert(4, long)])
      {%{"content" => [one, two, three]}, first} = kept(first)
      assert :erts_debug.same(two, read_two)
      {:ok, second} = Tarnish.Step.apply(first, [insert(5016, "!")])
      {%{"content" => [one_again, two_again, three_again]}, _} = kept(second)

      assert :erts_debug.same(one_again, one)
      assert :erts_debug.same(two_again, read_two)
      refute :erts_debug.same(three_again, three)
    end

    # What the docs of Node.to_json_kept say: on a flat document every change rewrites the
    # top-level list, whose chunk then takes in the one before, and the nodes changed in it are
    # written again.
    test "writes again a node changed in a chunk the next change takes in", %{schema: schema} do
      for count <- [3, 200] do
        content = for at <- 1..count, do: paragraph([text("p#{at}")])
        node = doc(schema, %{"type" => "doc", "content" => content})
        {:ok, first} = Tarnish.Step.apply(node, [insert(1, "a")])
        {%{"content" => [typed | _]}, first} = kept(first)
        {:ok, second} = Tarnish.Step.apply(first, [insert(6, "b")])
        {%{"content" => [typed_again, other | _]}, _} = kept(second)
        assert typed_again === typed
        refute :erts_debug.same(typed_again, typed)
        assert other === paragraph([text("bp2")])
      end
    end

    # A node whose reading flagged it is written anew, its map not being what toJSON writes for
    # it; what was written for it then is what toJSON writes.
    test "shares the term written anew for a node whose reading flagged it", %{schema: schema} do
      # A heading without its level, which toJSON writes.
      heading = %{"type" => "heading", "content" => [text("Title")]}
      node = doc(schema, %{"type" => "doc", "content" => [heading, paragraph([text("a")])]})
      # A long text keeps the first change's chunk, which the JSON kept is shared through.
      {:ok, first} = Tarnish.Step.apply(node, [insert(8, String.duplicate("long ", 1000))])
      {%{"content" => [written, _]}, first} = kept(first)
      assert written === Map.put(heading, "attrs", %{"level" => 1})
      refute :erts_debug.same(written, heading)
      {:ok, second} = Tarnish.Step.apply(first, [insert(8, "c")])
      {%{"content" => [again, _]}, _} = kept(second)
      assert :erts_debug.same(again, written)
    end

    # Steps of every kind, made by Transform ops at random places, in random messages of one to
    # five ops each, applied as Tarnish.Transform.new makes them, or as Step.apply applies the
    # steps it made, in turn.
    test "is what Node.to_json gives after steps of every kind, at random", %{schema: schema} do
      json = %{
        "type" => "doc",
        "content" => [
          %{"type" => "heading", "attrs" => %{"level" => 1}, "content" => [text("Title")]},
          paragraph([text("plain "), text("strong", [%{"type" => "strong"}]), text(" end")]),
          %{
            "type" => "bullet_list",
            "content" => [
              item([paragraph([text("one")])]),
              item([
                paragraph([text("two")]),
                %{"type" => "ordered_list", "content" => [item([paragraph([text("deep")])])]}
              ])
            ]
          },
          %{"type" => "blockquote", "content" => [paragraph([text("quoted")])]},
          %{"type" => "code_block", "content" => [text("code")]},
          %{"type" => "horizontal_rule"},
          # Flagged as read: a heading without its level.
          %{"type" => "heading", "content" => [text("Flagged")]},
          paragraph([])
        ]
      }

      for seed <- 1..20 do
        :rand.seed(:exsss, {seed, seed, seed})

        Enum.reduce(1..200, kept(doc(schema, json)) |> elem(1), fn message, node ->
          size = content_size(Tarnish.Node.to_json(node))
          ops = for _ <- 1..:rand.uniform(5), do: op(size)

          case Tarnish.Transform.new(node, ops) do
            {:ok, %{doc: transformed, steps: steps}} ->
              changed =
                if rem(message, 2) == 0 do
                  transformed
                else
                  {:ok, applied} = Tarnish.Step.apply(node, steps)
                  applied
                end

              changed |> kept() |> elem(1)

            error ->
              assert error === Tarnish.Transform.new(Map.put(node, :base, nil), ops)
              steps = [%{"stepType" => "replace", "from" => at(size), "to" => at(size)}]

              assert written(Tarnish.Step.apply(node, steps)) ===
                       written(Tarnish.Step.apply(Map.put(node, :base, nil), steps))

              node
          end
        end)
      end
    end
  end

  defp written({:ok, node}), do: {:ok, kept(node) |> elem(0)}
  defp written(error), do: error

  defp at(size), do: :rand.uniform(size + 1) - 1

  defp range(size) do
    from = at(size)
    {from, min(size, from + :rand.uniform(6) - 1)}
  end

  @marks [%{"type" => "em"}, %{"type" => "strong"}, %{"type" => "code"}]

  defp op(size) do
    {from, to} = range(size)
    pos = at(size)

    case :rand.uniform(18) do
      1 -> %{"op" => "insert", "pos" => pos, "content" => [text("x")]}
      2 -> %{"op" => "insert", "pos" => pos, "content" => [paragraph([text("new")])]}
      3 -> %{"op" => "delete", "from" => from, "to" => to}
      # An open slice, which splits a block, or joins two.
      4 -> %{"op" => "replace", "from" => from, "to" => to, "slice" => split_slice()}
      5 -> %{"op" => "addMark", "from" => from, "to" => to, "mark" => Enum.random(@marks)}
      6 -> %{"op" => "removeMark", "from" => from, "to" => to, "mark" => Enum.random(@marks)}
      7 -> %{"op" => "addNodeMark", "pos" => pos, "mark" => Enum.random(@marks)}
      8 -> %{"op" => "removeNodeMark", "pos" => pos, "mark" => "em"}
      9 -> %{"op" => "setNodeMarkup", "pos" => pos, "type" => "heading", "attrs" => level()}
      10 -> %{"op" => "setNodeAttribute", "pos" => pos, "attr" => "level", "value" => 3}
      11 -> %{"op" => "setDocAttribute", "attr" => "nope", "value" => 1}
      12 -> %{"op" => "wrap", "from" => from, "to" => to, "nodeType" => "blockquote"}
      13 -> %{"op" => "lift", "from" => from, "to" => to}
      14 -> %{"op" => "join", "pos" => pos}
      15 -> %{"op" => "split", "pos" => pos}
      16 -> %{"op" => "setBlockType", "from" => from, "to" => to, "type" => "code_block"}
      17 -> %{"op" => "wrap", "from" => from, "to" => to, "nodeType" => "bullet_list"}
      18 -> %{"op" => "replaceWith", "from" => from, "to" => to, "content" => [text("yz")]}
    end
  end

  defp level, do: %{"level" => :rand.uniform(6)}

  defp split_slice,
    do: %{"content" => [paragraph([text("a")]), paragraph([])], "openStart" => 1, "openEnd" => 1}

  # The schema a bench document holds.
  defp own_schema(path), do: path |> ordered() |> Access.get("schema") |> schema()

  defp schema(spec) do
    {:ok, schema} = Tarnish.Schema.new(spec)
    schema
  end

  describe "a node's JSON kept for another" do
    setup %{schema: schema} do
      content = for text <- ~w(one two three), do: paragraph([text(text)])
      %{node: doc(schema, %{"type" => "doc", "content" => content})}
    end

    test "is what Node.to_json gives on a node older than the one it was written for", %{
      node: node
    } do
      {:ok, first} = Tarnish.Step.apply(node, [insert(4, "!")])
      {_, first} = kept(first)
      {:ok, second} = Tarnish.Step.apply(first, [insert(10, "!")])
      {_, _} = kept(second)
      {:ok, from_first} = Tarnish.Step.apply(first, [insert(16, "?")])
      kept(from_first)
    end

    test "is what Node.to_json gives on a node it wasn't written for", %{
      node: node,
      schema: schema
    } do
      {:ok, one_way} = Tarnish.Step.apply(node, [insert(4, "!")])
      {:ok, other_way} = Tarnish.Step.apply(node, [insert(16, "?")])
      {_, other_way} = kept(other_way)
      other = doc(schema, %{"type" => "doc", "content" => [paragraph([text("other")])]})
      {_, other} = kept(other)

      # The same spec, another schema.
      {:ok, again} =
        @fixtures |> ordered() |> Access.get("schemas") |> hd() |> Tarnish.Schema.new()

      {_, elsewhere} = kept(doc(again, Tarnish.Node.to_json(node)))
      {:ok, ancestor} = Tarnish.Step.apply(node, [insert(9, "-")])

      for base <- [
            other_way.base,
            other.base,
            elsewhere.base,
            {ancestor.ref, :not_a_map},
            {ancestor.ref, %{"content" => :not_a_list}},
            {{elem(node.ref, 0), [<<1, 2, 3>>]}, %{}},
            {{elem(node.ref, 0), [<<1, 2, 3>> | elem(node.ref, 1)]}, %{}},
            {:not, :a_ref},
            {1, 2, 3},
            :not_a_base
          ] do
        node = Map.put(one_way, :base, base)
        assert Tarnish.Node.to_json(node) === fresh(one_way)
        {json, _} = Tarnish.Node.to_json_kept(node)
        assert json === fresh(one_way)
      end
    end

    test "is none on a node made before nodes kept one", %{node: node} do
      old = Map.delete(node, :base)
      assert Tarnish.Node.to_json(old) === fresh(node)
      {json, kept} = Tarnish.Node.to_json_kept(old)
      assert json === fresh(node)
      {:ok, changed} = Tarnish.Step.apply(kept, [insert(4, "!")])
      kept(changed)
    end
  end

  test "a kept node's JSON goes to a dirty scheduler past a normal one's limits", %{
    schema: schema
  } do
    long = String.duplicate("long ", 200_000)
    node = doc(schema, %{"type" => "doc", "content" => [paragraph([text(long)])]})
    {:ok, changed} = Tarnish.Step.apply(node, [insert(1, "x")])
    {_, changed} = kept(changed)
    {:ok, again} = Tarnish.Step.apply(changed, [insert(1, "y")])
    assert @native.to_json_from(again.ref, again.json, again.base) === :dirty
    expected = fresh(again)
    assert @native.to_json_from_dirty(again.ref, again.json, again.base) === expected
    assert kept(again) |> elem(0) === expected
  end

  test "a kept node holds no more than its JSON read and its JSON written", %{schema: schema} do
    content = for index <- 1..50, do: paragraph([text("paragraph #{index}")])
    json = %{"type" => "doc", "content" => content}
    node = json |> then(&doc(schema, &1)) |> chain()
    written = Tarnish.Node.to_json(node)
    assert :erts_debug.size(node) <= :erts_debug.size(json) + :erts_debug.flat_size(written) + 256
  end
end
