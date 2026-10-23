defmodule Tarnish.KeptTest do
  use ExUnit.Case, async: true
  import Tarnish.TestDocs

  # A document keeps its JSON, which Step.apply keeps current and the JSON of the documents
  # changes make of it then shares: what Node.to_json gives must be what it gives a document that
  # keeps no JSON, term for term.

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

  # The document with no JSON whose parts its own shares: a JSON of a ref that isn't one.
  defp plain(doc), do: Map.merge(doc, %{json: nil, of: :none})

  # What the document's JSON must be: what it writes sharing nothing.
  defp fresh(doc), do: Tarnish.Node.to_json(plain(doc))

  # The document's JSON, as Node.to_json gives it, which must be what a fresh write gives.
  defp json(doc) do
    json = Tarnish.Node.to_json(doc)
    expected = fresh(doc)
    assert json === expected

    assert :erlang.term_to_binary(json, [:deterministic]) ===
             :erlang.term_to_binary(expected, [:deterministic])

    json
  end

  # Applies the steps with Step.apply, whose document must keep the JSON it wrote, which is what
  # a fresh write gives, term for term; or which must fail as the steps alone fail. Then what it
  # gave.
  defp applied(doc, steps) do
    applied = Tarnish.Step.apply(doc, steps)

    case @native.apply_steps_dirty(doc.ref, steps) do
      {:ok, _} when steps == [] ->
        assert applied === {:ok, doc}

      {:ok, _} ->
        assert {:ok, changed} = applied
        assert changed.of === changed.ref
        assert :erts_debug.same(Tarnish.Node.to_json(changed), changed.json)
        assert json(changed) === changed.json

      error ->
        assert applied === error
        assert Tarnish.Step.apply(plain(doc), steps) === error
    end

    applied
  end

  # The size of a document's JSON, as positions.
  defp size(%{"type" => "text", "text" => text}), do: length(String.to_charlist(text))
  defp size(%{"content" => content}), do: 2 + Enum.sum(Enum.map(content, &size/1))
  defp size(_leaf), do: 1

  defp content_size(json), do: size(json) - 2

  # The positions where typing "x" applies, in order.
  defp places(doc) do
    for at <- 0..content_size(Tarnish.Node.to_json(doc)),
        match?({:ok, _}, @native.apply_steps_dirty(doc.ref, [insert(at, "x")])),
        do: at
  end

  # 100 messages typing "x" in a place each, spread evenly, each into the document the message
  # before it made, and each checked; then the document the last made.
  defp chain(doc, count \\ 100) do
    places = places(doc)
    json(doc)

    {doc, _} =
      Enum.reduce(0..(count - 1), {doc, []}, fn index, {before, typed_at} ->
        place = Enum.at(places, div(index * length(places), count))
        steps = [insert(place + Enum.count(typed_at, &(&1 < place)), "x")]
        {:ok, changed} = applied(before, steps)
        {changed, [place | typed_at]}
      end)

    doc
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
        doc = doc(schema_for.(path), file["doc"])
        chain(doc)
      end
    end

    # The card stack TARNISH_BENCH_DOC names, read here and never kept in the repo. Its changes
    # each apply to the document as read, so each is one message from the document read, and one
    # more from where a chain of messages left it, where those that fail must fail as they do on
    # a document that keeps no JSON.
    @tag :bench_doc
    test "is what Node.to_json gives on TARNISH_BENCH_DOC's card stack" do
      with %{doc: read, changes: changes} <- bench_doc() do
        json(read)
        chained = chain(read)

        for %{"steps" => steps} <- changes, from <- [read, chained] do
          applied(from, steps)
        end
      end
    end

    test "is the JSON written last, when no step has changed the document", %{schema: schema} do
      doc = doc(schema, %{"type" => "doc", "content" => [paragraph([text("a")])]})
      {:ok, changed} = applied(doc, [insert(1, "b")])
      json = json(changed)
      assert :erts_debug.same(Tarnish.Node.to_json(changed), json)
      {:ok, unchanged} = applied(changed, [])
      assert :erts_debug.same(Tarnish.Node.to_json(unchanged), json)
    end

    test "shares the terms the message before wrote for the nodes this one left", %{
      schema: schema
    } do
      content = for text <- ~w(one two three), do: paragraph([text(text)])
      [_, read_two, _] = content
      doc = doc(schema, %{"type" => "doc", "content" => content})

      # A long text after "one", then "!" after "three".
      long = String.duplicate("long ", 1000)
      {:ok, first} = applied(doc, [insert(4, long)])
      %{"content" => [one, two, three]} = json(first)
      assert :erts_debug.same(two, read_two)
      {:ok, second} = applied(first, [insert(5016, "!")])
      %{"content" => [one_again, two_again, three_again]} = json(second)

      assert :erts_debug.same(one_again, one)
      assert :erts_debug.same(two_again, read_two)
      refute :erts_debug.same(three_again, three)
    end

    # On a flat document every change rewrites the top-level list, whose chunk then takes in the
    # one before; the nodes the change left in it are shared still.
    test "shares a node changed in a chunk the next change takes in", %{schema: schema} do
      for count <- [3, 200] do
        content = for at <- 1..count, do: paragraph([text("p#{at}")])
        doc = doc(schema, %{"type" => "doc", "content" => content})
        {:ok, first} = applied(doc, [insert(1, "a")])
        %{"content" => [typed | first_rest]} = json(first)
        {:ok, second} = applied(first, [insert(6, "b")])
        %{"content" => [typed_again, other | rest]} = json(second)
        assert typed_again === paragraph([text("ap1")])
        assert :erts_debug.same(typed_again, typed)
        assert other === paragraph([text("bp2")])
        [_ | first_rest] = first_rest
        assert Enum.all?(Enum.zip(rest, first_rest), fn {a, b} -> :erts_debug.same(a, b) end)
      end
    end

    # A node whose reading flagged it is written anew, its map not being what toJSON writes for
    # it; what was written for it then is what toJSON writes.
    test "shares the term written anew for a node whose reading flagged it", %{schema: schema} do
      # A heading without its level, which toJSON writes.
      heading = %{"type" => "heading", "content" => [text("Title")]}
      doc = doc(schema, %{"type" => "doc", "content" => [heading, paragraph([text("a")])]})
      # A long text keeps the first change's chunk, which the JSON kept is shared through.
      {:ok, first} = applied(doc, [insert(8, String.duplicate("long ", 1000))])
      %{"content" => [written, _]} = json(first)
      assert written === Map.put(heading, "attrs", %{"level" => 1})
      refute :erts_debug.same(written, heading)
      {:ok, second} = applied(first, [insert(8, "c")])
      %{"content" => [again, _]} = json(second)
      assert :erts_debug.same(again, written)
    end

    # Steps of every kind, made by Transform ops at random places, in random messages of one to
    # five ops each, applied as Tarnish.Transform.new makes them, or as Step.apply applies the
    # steps it made, in one call or one at a time, in turn.
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

        Enum.reduce(1..200, doc(schema, json), fn message, doc ->
          size = content_size(Tarnish.Node.to_json(doc))
          ops = for _ <- 1..:rand.uniform(5), do: op(size)

          case Tarnish.Transform.new(doc, ops) do
            {:ok, %{doc: transformed, steps: steps}} ->
              case rem(message, 3) do
                0 ->
                  json(transformed)
                  transformed

                1 ->
                  {:ok, applied} = applied(doc, steps)
                  applied

                2 ->
                  Enum.reduce(steps, doc, fn step, before ->
                    {:ok, applied} = applied(before, [step])
                    applied
                  end)
              end

            error ->
              assert error === Tarnish.Transform.new(plain(doc), ops)
              steps = [%{"stepType" => "replace", "from" => at(size), "to" => at(size)}]

              assert written(Tarnish.Step.apply(doc, steps)) ===
                       written(Tarnish.Step.apply(plain(doc), steps))

              applied(doc, steps)

              doc
          end
        end)
      end
    end
  end

  defp written({:ok, doc}), do: {:ok, json(doc)}
  defp written(error), do: error

  # The steps alone, which leave the document the JSON the one given kept.
  defp stepped(doc, steps) do
    {:ok, ref} = @native.apply_steps_dirty(doc.ref, steps)
    %{doc | ref: ref}
  end

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

  describe "Step.apply" do
    # prosemirror-transform's tests, each of whose steps applies as one call to the document
    # read, whose JSON is the map read; one at a time, each to the document the one before made;
    # and all but the first in one call, after the first.
    test "keeps the JSON a fresh write gives, for every step of transform.json" do
      fixtures = ordered(@fixtures)
      schemas = Enum.map(fixtures["schemas"], &schema/1)
      tests = Jason.decode!(File.read!(@fixtures))["tests"]

      kinds =
        for test <- tests,
            {:ok, read} =
              Tarnish.Node.from_json(Enum.at(schemas, test["schema"] || 0), test["start"]),
            steps = test["steps"] || [],
            reduce: %{} do
          kinds ->
            applied(read, steps)
            json(read)

            Enum.reduce(steps, read, fn step, before ->
              case applied(before, [step]) do
                {:ok, changed} -> changed
                _error -> before
              end
            end)

            with [first | rest] when rest != [] <- steps,
                 {:ok, changed} <- applied(read, [first]),
                 do: applied(changed, rest)

            Enum.reduce(steps, kinds, &Map.update(&2, &1["stepType"], 1, fn n -> n + 1 end))
        end

      assert Map.keys(kinds) |> Enum.sort() ==
               ~w(addMark addNodeMark attr docAttr removeMark removeNodeMark replace replaceAround)
    end

    test "gives the document itself for no steps", %{schema: schema} do
      doc = doc(schema, %{"type" => "doc", "content" => [paragraph([text("a")])]})
      assert {:ok, ^doc} = applied(doc, [])
      {:ok, changed} = applied(doc, [insert(1, "b")])
      assert {:ok, ^changed} = applied(changed, [])
      assert :erts_debug.same(Tarnish.Node.to_json(changed), changed.json)
    end

    # A step deep down writes only the nodes along the path to the node it changes, and shares
    # the terms of the siblings off it.
    test "writes the nodes along the path a step takes, at every depth", %{schema: schema} do
      for depth <- [0, 1, 9] do
        bottom = [paragraph([text("s")]), paragraph([text("ab")]), paragraph([text("t")])]

        content =
          Enum.reduce(1..depth//1, bottom, fn _, inner ->
            [
              paragraph([text("s")]),
              %{"type" => "blockquote", "content" => inner},
              paragraph([text("t")])
            ]
          end)

        doc = doc(schema, %{"type" => "doc", "content" => content})
        before = json(doc)
        {:ok, changed} = applied(doc, [insert(4 * depth + 5, "x")])

        {changed, shared} =
          Enum.reduce(1..depth//1, {changed.json, before}, fn _,
                                                              {%{"content" => [s, q, t]}, old} ->
            %{"content" => [old_s, old_q, old_t]} = old
            assert :erts_debug.same(s, old_s) and :erts_debug.same(t, old_t)
            {q, old_q}
          end)

        %{"content" => [s, typed, t]} = changed
        %{"content" => [old_s, _, old_t]} = shared
        assert :erts_debug.same(s, old_s) and :erts_debug.same(t, old_t)
        assert typed === paragraph([text("axb")])
      end
    end

    test "keeps the same JSON on a document another process holds", %{schema: schema} do
      # A chunk's binary is bigger than 64 bytes, which the processes share rather than copy, so
      # the ref its JSON is of is still the document's own there.
      for count <- [1, 200] do
        content = for at <- 1..count, do: paragraph([text("p#{at}")])
        doc = doc(schema, %{"type" => "doc", "content" => content})
        {:ok, kept} = applied(doc, [insert(1, "a")])
        parent = self()

        child =
          spawn_link(fn ->
            receive do
              kept -> send(parent, {:applied, kept, Tarnish.Step.apply(kept, [insert(2, "b")])})
            end
          end)

        send(child, kept)
        assert_receive {:applied, there, {:ok, %{json: json}}}
        assert {:ok, %{json: ^json}} = applied(there, [insert(2, "b")])
        {:ok, %{json: ^json}} = applied(kept, [insert(2, "b")])
      end
    end

    test "keeps the JSON a fresh write gives with a JSON it wasn't written for", %{
      schema: schema
    } do
      content = for text <- ~w(one two three), do: paragraph([text(text)])
      doc = doc(schema, %{"type" => "doc", "content" => content})
      {:ok, first} = applied(doc, [insert(4, "!")])
      {:ok, second} = applied(first, [insert(10, "!")])
      other = doc(schema, %{"type" => "doc", "content" => [paragraph([text("other")])]})
      {:ok, other} = applied(other, [insert(1, "o")])

      for {of, json} <- [
            {first.ref, first.json},
            {second.ref, second.json},
            {other.ref, other.json},
            {first.ref, :not_a_map},
            {second.ref, %{"content" => :not_a_list}},
            {:not, :a_ref},
            {:not_a_ref, first.json},
            {nil, doc.json}
          ],
          from <- [first, second] do
        applied(%{from | of: of, json: json}, [insert(16, "?")])
      end

      applied(Map.delete(first, :of), [insert(16, "?")])
    end

    test "fails as the steps fail", %{schema: schema} do
      doc = doc(schema, %{"type" => "doc", "content" => [paragraph([text("a")])]})
      {:ok, kept} = applied(doc, [insert(1, "b")])

      for from <- [doc, kept] do
        assert {:error, _} = applied(from, [insert(100, "x")])
        assert {:error, _} = applied(from, [insert(1, "x"), %{"stepType" => "nope"}])
      end

      assert_raise ArgumentError, fn -> Tarnish.Step.apply(kept, [self()]) end

      assert_raise ArgumentError, fn ->
        Tarnish.Step.apply(%{kept | ref: :nope}, [insert(1, "x")])
      end
    end

    # A message whose steps alone and whose write alone each a normal scheduler takes on in one
    # call, but not both: the write has what the steps leave of a light call.
    test "goes to a dirty scheduler past what a normal one takes on in one call", %{
      schema: schema
    } do
      content = for at <- 1..1500, do: paragraph([text("p#{at}")])
      doc = doc(schema, %{"type" => "doc", "content" => content})
      {:ok, kept} = applied(doc, [insert(1, "a")])
      steps = for _ <- 1..4, do: insert(1, "b")
      assert {:ok, changed} = @native.apply_steps(kept.ref, steps)
      assert @native.to_json_from(changed, kept.json, kept.of) !== :dirty
      assert @native.apply_write(kept.ref, steps, kept.json, kept.of) === :dirty
      {:ok, {_, json}} = @native.apply_write_dirty(kept.ref, steps, kept.json, kept.of)
      assert {:ok, %{json: ^json}} = applied(kept, steps)
    end
  end

  describe "a document's JSON kept for another" do
    setup %{schema: schema} do
      content = for text <- ~w(one two three), do: paragraph([text(text)])
      %{doc: doc(schema, %{"type" => "doc", "content" => content})}
    end

    test "is what Node.to_json gives on a document older than the one it was written for", %{
      doc: doc
    } do
      {:ok, first} = applied(doc, [insert(4, "!")])
      {:ok, _second} = applied(first, [insert(10, "!")])
      json(stepped(first, [insert(16, "?")]))
      applied(first, [insert(16, "?")])
    end

    test "is what Node.to_json gives on a document it wasn't written for", %{
      doc: doc,
      schema: schema
    } do
      one_way = stepped(doc, [insert(4, "!")])
      {:ok, other_way} = applied(doc, [insert(16, "?")])
      other = doc(schema, %{"type" => "doc", "content" => [paragraph([text("other")])]})
      {:ok, other} = applied(other, [insert(1, "o")])

      # The same spec, another schema.
      {:ok, again} =
        @fixtures |> ordered() |> Access.get("schemas") |> hd() |> Tarnish.Schema.new()

      {:ok, elsewhere} = applied(doc(again, Tarnish.Node.to_json(doc)), [insert(1, "e")])
      ancestor = stepped(doc, [insert(9, "-")])

      for {of, json} <- [
            {other_way.ref, other_way.json},
            {other.ref, other.json},
            {elsewhere.ref, elsewhere.json},
            {ancestor.ref, :not_a_map},
            {ancestor.ref, %{"content" => :not_a_list}},
            {{elem(doc.ref, 0), [<<1, 2, 3>>]}, %{}},
            {{elem(doc.ref, 0), [<<1, 2, 3>> | elem(doc.ref, 1)]}, %{}},
            {:not, :a_ref},
            {{1, 2, 3}, one_way.json},
            {:not_a_ref, one_way.json}
          ] do
        assert Tarnish.Node.to_json(%{one_way | of: of, json: json}) === fresh(one_way)
      end
    end

    test "is none on a document made before documents kept one", %{doc: doc} do
      old = Map.delete(doc, :of)
      assert Tarnish.Node.to_json(old) === fresh(doc)
      assert {:ok, ^old} = Tarnish.Step.apply(old, [])
      {:ok, changed} = applied(old, [insert(4, "!")])
      applied(changed, [insert(5, "!")])
    end
  end

  test "a kept document's JSON goes to a dirty scheduler past a normal one's limits", %{
    schema: schema
  } do
    long = String.duplicate("long ", 200_000)
    doc = doc(schema, %{"type" => "doc", "content" => [paragraph([text(long)])]})
    {:ok, changed} = applied(doc, [insert(1, "x")])
    {:ok, ref} = @native.apply_steps_dirty(changed.ref, [insert(1, "y")])
    again = %{changed | ref: ref}
    assert @native.to_json_from(again.ref, again.json, again.of) === :dirty
    expected = fresh(again)
    assert @native.to_json_from_dirty(again.ref, again.json, again.of) === expected
    assert json(again) === expected
    assert {:ok, %{json: ^expected}} = applied(changed, [insert(1, "y")])
  end

  # The heap's size counts a chunk binary as its header only: the chunks of the ref the JSON was
  # written for are bounded apart.
  test "a kept document's heap holds no more than its JSON written", %{schema: schema} do
    content = for index <- 1..50, do: paragraph([text("paragraph #{index}")])
    json = %{"type" => "doc", "content" => content}
    doc = json |> then(&doc(schema, &1)) |> chain()
    written = Tarnish.Node.to_json(doc)
    assert :erts_debug.size(doc) <= :erts_debug.flat_size(written) + 256
    # The document the JSON was written for shares its oldest chunk, the biggest by far.
    bytes = fn {_schema, chunks} -> chunks |> Enum.map(&byte_size/1) |> Enum.sum() end

    for at <- [3, 40, 200] do
      {:ok, ref} = @native.apply_steps_dirty(doc.ref, [insert(at, "y")])
      assert bytes.(doc.ref) <= 2 * bytes.(ref)
      {:ok, changed} = applied(doc, [insert(at, "y")])
      assert :erts_debug.size(changed) <= :erts_debug.flat_size(changed.json) + 256
    end
  end
end
