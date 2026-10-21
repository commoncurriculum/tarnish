defmodule Tarnish.KeptTest do
  use ExUnit.Case, async: true
  import Tarnish.TestDocs

  # A node written with Node.to_json_kept keeps the JSON written, which the JSON of the nodes
  # changes make of it then shares: what it gives must be what Node.to_json gives a node that
  # keeps no JSON but the one it was read from, term for term.

  @native Application.compile_env(:tarnish, :native, Tarnish.Native)

  @fixtures Path.expand("../../fixtures/transform.json", __DIR__)
  @external_resource @fixtures

  # The documents `npm run bench` writes, when it has.
  @bench Path.expand("../../target/bench", __DIR__)

  setup_all do
    # prosemirror-schema-basic with prosemirror-schema-list.
    {:ok, schema} =
      @fixtures |> ordered() |> Access.get("schemas") |> hd() |> Tarnish.Schema.new()

    %{schema: schema}
  end

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

  # Applies the steps with Step.apply_kept, which must give what Step.apply and then
  # Node.to_json_kept give, term for term, or the same error; then what it gave.
  defp apply_kept(node, steps) do
    fused = Tarnish.Step.apply_kept(node, steps)

    case Tarnish.Step.apply(node, steps) do
      {:ok, changed} ->
        {json, _} = kept(changed)
        assert {:ok, written, fused_node} = fused
        assert written === json

        assert :erlang.term_to_binary(written, [:deterministic]) ===
                 :erlang.term_to_binary(json, [:deterministic])

        assert fused_node.base === {fused_node.ref, written}
        assert fresh(fused_node) === json
        assert Tarnish.Node.to_json(fused_node) === json
        assert fused_node.json === node.json

      error ->
        assert fused === error
    end

    fused
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
        # Every other message through Step.apply_kept, whose node the next takes.
        written =
          case rem(index, 2) do
            0 ->
              {:ok, changed} = Tarnish.Step.apply(before, steps)
              changed |> kept() |> elem(1)

            1 ->
              {:ok, _, written} = apply_kept(before, steps)
              written
          end

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
    @tag :bench_doc
    test "is what Node.to_json gives on TARNISH_BENCH_DOC's card stack" do
      with %{doc: node, changes: changes} <- bench_doc() do
        {_, read} = kept(node)
        chained = chain(node)

        for %{"steps" => steps} <- changes, from <- [read, chained] do
          case Tarnish.Step.apply(from, steps) do
            {:ok, changed} -> kept(changed)
            error -> assert error === Tarnish.Step.apply(Map.put(from, :base, nil), steps)
          end

          apply_kept(from, steps)
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
              case rem(message, 3) do
                0 ->
                  transformed |> kept() |> elem(1)

                1 ->
                  {:ok, applied} = Tarnish.Step.apply(node, steps)
                  applied |> kept() |> elem(1)

                2 ->
                  {:ok, _, applied} = apply_kept(node, steps)
                  applied
              end

            error ->
              assert error === Tarnish.Transform.new(Map.put(node, :base, nil), ops)
              steps = [%{"stepType" => "replace", "from" => at(size), "to" => at(size)}]

              assert written(Tarnish.Step.apply(node, steps)) ===
                       written(Tarnish.Step.apply(Map.put(node, :base, nil), steps))

              apply_kept(node, steps)

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

  describe "Step.apply_kept" do
    # prosemirror-transform's tests, each of whose steps applies as one call to the document
    # read, whose base is the map read; one at a time, each to the node the one before made and
    # wrote; and all but the first in one call, after the first.
    test "gives what Step.apply and Node.to_json_kept give, for every step of transform.json" do
      fixtures = ordered(@fixtures)
      schemas = Enum.map(fixtures["schemas"], &schema/1)
      tests = Jason.decode!(File.read!(@fixtures))["tests"]

      kinds =
        for test <- tests,
            {:ok, node} =
              Tarnish.Node.from_json(Enum.at(schemas, test["schema"] || 0), test["start"]),
            steps = test["steps"] || [],
            reduce: %{} do
          kinds ->
            apply_kept(node, steps)
            {_, read} = kept(node)

            Enum.reduce(steps, read, fn step, before ->
              case apply_kept(before, [step]) do
                {:ok, _, written} -> written
                _error -> before
              end
            end)

            with [first | rest] when rest != [] <- steps,
                 {:ok, _, written} <- apply_kept(read, [first]),
                 do: apply_kept(written, rest)

            Enum.reduce(steps, kinds, &Map.update(&2, &1["stepType"], 1, fn n -> n + 1 end))
        end

      assert Map.keys(kinds) |> Enum.sort() ==
               ~w(addMark addNodeMark attr docAttr removeMark removeNodeMark replace replaceAround)
    end

    test "gives the node's own JSON again for no steps", %{schema: schema} do
      node = doc(schema, %{"type" => "doc", "content" => [paragraph([text("a")])]})
      apply_kept(node, [])
      {:ok, json, kept} = apply_kept(node, [insert(1, "b")])
      assert {:ok, again, ^kept} = apply_kept(kept, [])
      assert :erts_debug.same(again, json)
    end

    test "shares the terms the message before wrote for the nodes the steps left", %{
      schema: schema
    } do
      content = for at <- 1..200, do: paragraph([text("p#{at}")])
      node = doc(schema, %{"type" => "doc", "content" => content})
      {:ok, %{"content" => [_ | first]}, kept} = apply_kept(node, [insert(1, "a")])
      # The next change takes in the first's chunk: to_json_kept writes its nodes again.
      {:ok, %{"content" => [typed, second | rest]}, _} = apply_kept(kept, [insert(6, "b")])
      assert typed === paragraph([text("ap1")])
      assert second === paragraph([text("bp2")])
      [_ | first_rest] = first
      assert Enum.all?(Enum.zip(rest, first_rest), fn {a, b} -> :erts_debug.same(a, b) end)
    end

    test "is what Step.apply and Node.to_json_kept give on a node another process holds", %{
      schema: schema
    } do
      # A chunk's binary is bigger than 64 bytes, which the processes share rather than copy, so
      # the ref the base names is still the document's own there.
      for count <- [1, 200] do
        content = for at <- 1..count, do: paragraph([text("p#{at}")])
        node = doc(schema, %{"type" => "doc", "content" => content})
        {:ok, _, kept} = apply_kept(node, [insert(1, "a")])
        parent = self()

        child =
          spawn_link(fn ->
            receive do
              kept ->
                send(parent, {:applied, kept, Tarnish.Step.apply_kept(kept, [insert(2, "b")])})
            end
          end)

        send(child, kept)
        assert_receive {:applied, there, {:ok, json, _}}
        assert {:ok, ^json, _} = apply_kept(there, [insert(2, "b")])
        {:ok, ^json, _} = apply_kept(kept, [insert(2, "b")])
      end
    end

    test "is what Step.apply and Node.to_json_kept give with a base it wasn't written for", %{
      schema: schema
    } do
      content = for text <- ~w(one two three), do: paragraph([text(text)])
      node = doc(schema, %{"type" => "doc", "content" => content})
      {:ok, _, first} = apply_kept(node, [insert(4, "!")])
      {:ok, _, second} = apply_kept(first, [insert(10, "!")])
      other = doc(schema, %{"type" => "doc", "content" => [paragraph([text("other")])]})
      {_, other} = kept(other)

      for base <- [
            first.base,
            second.base,
            other.base,
            {first.ref, :not_a_map},
            {second.ref, %{"content" => :not_a_list}},
            {:not, :a_ref},
            :not_a_base,
            nil
          ],
          from <- [first, second] do
        apply_kept(Map.put(from, :base, base), [insert(16, "?")])
      end

      apply_kept(Map.delete(first, :base), [insert(16, "?")])
    end

    test "fails as Step.apply does", %{schema: schema} do
      node = doc(schema, %{"type" => "doc", "content" => [paragraph([text("a")])]})
      {:ok, _, kept} = apply_kept(node, [insert(1, "b")])

      for from <- [node, kept] do
        assert {:error, _} = apply_kept(from, [insert(100, "x")])
        assert {:error, _} = apply_kept(from, [insert(1, "x"), %{"stepType" => "nope"}])
      end

      assert_raise ArgumentError, fn -> Tarnish.Step.apply_kept(kept, [self()]) end
      assert_raise ArgumentError, fn -> Tarnish.Step.apply_kept(%{kept | ref: :nope}, []) end
    end

    # A message that each of Step.apply and to_json_kept takes on alone, on a normal scheduler,
    # but not both: the write has what the apply leaves of a light call.
    test "goes to a dirty scheduler past what a normal one takes on in one call", %{
      schema: schema
    } do
      content = for at <- 1..1500, do: paragraph([text("p#{at}")])
      node = doc(schema, %{"type" => "doc", "content" => content})
      {:ok, _, kept} = apply_kept(node, [insert(1, "a")])
      steps = for _ <- 1..4, do: insert(1, "b")
      assert {:ok, changed} = @native.apply_steps(kept.ref, steps)
      assert @native.to_json_from(changed, kept.json, kept.base) !== :dirty
      assert @native.apply_write(kept.ref, steps, kept.json, kept.base) === :dirty
      {:ok, {_, json}} = @native.apply_write_dirty(kept.ref, steps, kept.json, kept.base)
      assert {:ok, ^json, _} = apply_kept(kept, steps)
    end
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

  # The heap's size counts a chunk binary as its header only: the chunks of the ref the JSON was
  # written for are bounded apart.
  test "a kept node's heap holds no more than its JSON read and its JSON written", %{
    schema: schema
  } do
    content = for index <- 1..50, do: paragraph([text("paragraph #{index}")])
    json = %{"type" => "doc", "content" => content}
    node = json |> then(&doc(schema, &1)) |> chain()
    written = Tarnish.Node.to_json(node)
    assert :erts_debug.size(node) <= :erts_debug.size(json) + :erts_debug.flat_size(written) + 256
    # The document the JSON was written for shares its oldest chunk, the biggest by far.
    bytes = fn {_schema, chunks} -> chunks |> Enum.map(&byte_size/1) |> Enum.sum() end

    for at <- [3, 40, 200] do
      {:ok, changed} = Tarnish.Step.apply(node, [insert(at, "y")])
      assert bytes.(elem(changed.base, 0)) <= 2 * bytes.(changed.ref)
    end
  end
end
