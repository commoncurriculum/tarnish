defmodule Tarnish.BridgeTest do
  use ExUnit.Case, async: true

  @moduletag :bridge

  @invalid "Unknown operation or invalid input"
  @no_options "The HTML conversions take no options"

  # The tree builder takes time quadratic in the count of formatting elements open.
  @long_to_parse String.duplicate("<b><i>", 600) <> "x"
  @long_parsed %{
    "type" => "text",
    "marks" => [%{"type" => "em"}, %{"type" => "strong"}],
    "text" => "x"
  }

  # Terms of each kind Jason encodes: integers at each width the external format writes one in,
  # and past a double's range; floats JavaScript writes as integers or with an exponent; atoms,
  # some past Latin-1, which the VM gives out only in the external format; charlists, which it
  # writes as bytes where it can; maps whose keys JSON.parse orders otherwise than Jason writes
  # them, or reads as one key; maps past a flatmap's 32 keys; structs; and documents of such
  # terms.
  @terms [
    0,
    255,
    256,
    -1,
    Integer.pow(2, 31) - 1,
    Integer.pow(2, 31),
    -Integer.pow(2, 31),
    -Integer.pow(2, 31) - 1,
    Integer.pow(2, 53) - 1,
    Integer.pow(2, 53) + 1,
    -Integer.pow(2, 63),
    Integer.pow(2, 63),
    Integer.pow(2, 64) - 1,
    Integer.pow(2, 64) + 1,
    Integer.pow(2, 200) + 1,
    Integer.pow(2, 1024),
    -Integer.pow(2, 1024),
    0.5,
    -0.0,
    1.0,
    1.0e20,
    1.0e21,
    5.0e-324,
    1.7976931348623157e308,
    nil,
    true,
    false,
    :foo,
    :Fïrst,
    :"✓",
    "",
    "Fïrst ✓",
    "\"\\\n\x01\u{2028}",
    ~c"abc",
    [1, 2, 255],
    ~c"✓",
    [],
    [nil, [true, [[]]]],
    %{},
    %{:a => 1, "a" => 2, 7 => 3},
    %{"10" => 1, "2" => 2, "01" => 3, "b" => 4},
    %{-1 => 1, 4_294_967_294 => 2, 4_294_967_295 => 3},
    %{:"✓" => 1, :a => 2},
    %{Integer.pow(2, 64) => 1, nil => 2, true => 3},
    %{1.5 => 1},
    Map.new(1..40, &{"k#{&1}", &1}),
    Map.put(Map.new(1..40, &{:"k#{&1}", &1}), "k7", 0),
    Jason.OrderedObject.new([{"b", 1}, {"10", 2}, {"a", 3}, {"2", 4}, {"01", 5}]),
    Jason.OrderedObject.new([{:b, 1}, {"b", 2}, {7, 3}]),
    Jason.OrderedObject.new([]),
    ~D[2024-01-02],
    %{"d" => ~D[2024-01-02], "t" => [~D[2024-01-03]]},
    %{
      type: :doc,
      content: [
        Jason.OrderedObject.new([
          {"type", "paragraph"},
          {"content", [%{type: :text, text: :"Fïrst ✓"}]}
        ])
      ]
    },
    %{
      "type" => "doc",
      "content" => [
        %{
          "type" => "paragraph",
          "content" => [
            %{:text => "Old", "text" => "First", "type" => "text"},
            %{:a => 1, :"✓" => 2, "text" => "Second", "type" => "text"}
          ]
        }
      ]
    }
  ]

  # JSON texts: each term's as Jason writes it, numbers written otherwise, past a double's range
  # among them, a lone surrogate's escape, and a key written twice.
  @texts Enum.map(@terms, &Jason.encode!/1) ++
           ["1e400", "-1e400", "-0", "1E2", "0.1", ~S("\ud800"), ~S({"a":1,"a":[2]})]

  # Terms Jason can't encode, to which a test adds a pid, which an attribute can't hold.
  @refused [{"First"}, [1 | 2], <<0xFF>>, MapSet.new([1])]

  setup %{backend: backend} do
    start_supervised!({Tarnish.Bridge, backend: backend})
    %{opts: [backend: backend]}
  end

  for backend <- [:node, :nif] do
    describe "#{backend}:" do
      @describetag backend: backend

      test "converts HTML to a document's JSON and back", %{opts: opts} do
        html = "<p>Hi <em>there</em></p>"

        doc_json = %{
          "type" => "doc",
          "content" => [
            paragraph([
              text("Hi "),
              %{"type" => "text", "marks" => [%{"type" => "em"}], "text" => "there"}
            ])
          ]
        }

        assert Tarnish.Bridge.parse_html(html, %{}, opts) === {:ok, doc_json}
        assert Tarnish.Bridge.serialize_html(doc_json, %{}, opts) === {:ok, html}
      end

      test "spreads a batch over its workers or threads, and keeps its order", %{opts: opts} do
        requests = for n <- 1..20, do: {"parseHTML", "<p>#{n}</p>"}

        assert Tarnish.Bridge.each(requests, opts) ===
                 for(n <- 1..20, do: {:ok, document([to_string(n)])})
      end

      test "refuses a request of no operation, or whose input isn't its operation's, and answers the next",
           %{opts: opts} do
        requests = [
          {"nonesuch", "x"},
          {"parseMarkdown", 5},
          {"serializeMarkdown", "x"},
          {"parseHTML", document(["x"])},
          {"parseHTML", nil},
          {"serializeHTML", []},
          {"serializeHTML", nil},
          {"serializeHTML", "<p>x</p>"}
        ]

        assert Tarnish.Bridge.each(requests, opts) === List.duplicate({:error, @invalid}, 8)
        assert Tarnish.Bridge.parse_html("<p>x</p>", %{}, opts) === {:ok, document(["x"])}
      end

      test "gives a conversion its options, which it checks before it reads the document",
           %{opts: opts} do
        nonesuch = %{"type" => "nonesuch"}
        assert Tarnish.Bridge.parse_html("<p>x</p>", %{"a" => 1}, opts) === {:error, @no_options}

        assert Tarnish.Bridge.serialize_html(nonesuch, %{"a" => 1}, opts) ===
                 {:error, @no_options}

        assert Tarnish.Bridge.serialize_html(nonesuch, %{}, opts) ===
                 {:error, "Unknown node type: nonesuch"}
      end

      test "leaves out empty options, which a conversion that takes none refuses", %{opts: opts} do
        parsed = {:ok, document(["x"])}
        assert Tarnish.Bridge.parse_html("<p>x</p>", nil, opts) === parsed
        assert Tarnish.Bridge.each([{"parseHTML", "<p>x</p>", %{}}], opts) === [parsed]
      end

      test "reads a lone surrogate in an answer as U+FFFD, and keeps a backslash before text like its escape",
           %{opts: opts} do
        assert Tarnish.Bridge.parse_markdown("x", %{}, opts) ===
                 {:error, "No Markdown: �\\😀\\ud800"}
      end

      test "converts a document of any size alone as in a batch", %{opts: opts} do
        for paragraphs <- [1, 5_000] do
          sized = document(List.duplicate("x", paragraphs))
          html = {:ok, String.duplicate("<p>x</p>", paragraphs)}
          assert Tarnish.Bridge.serialize_html(sized, %{}, opts) === html
          requests = [{"serializeHTML", sized}, {"serializeHTML", sized}]
          assert Tarnish.Bridge.each(requests, opts) === [html, html]
        end
      end

      test "parses a text that takes long for its length alone as in a batch", %{opts: opts} do
        request = {"parseHTML", @long_to_parse}
        answer = {:ok, holding(@long_parsed)}
        assert Tarnish.Bridge.each([request], opts) === [answer]
        assert Tarnish.Bridge.each([request, request], opts) === [answer, answer]
      end
    end
  end

  # The tests' conversions show how each backend read a term in the JSON serializeMarkdown and a
  # paragraph's data-json write of it, and how it wrote a value in the term parseMarkdown and
  # data-json make of JSON.
  describe "the NIF and the Node workers" do
    @describetag backend: :node

    test "read each term as Jason encodes it, alone and in a batch" do
      requests =
        for term <- @terms,
            input <- [%{"v" => term}, holding(text(term)), holding_data(term), term],
            operation <- ["serializeMarkdown", "serializeHTML"],
            do: {operation, input}

      for request <- requests, do: assert_same_answers([request])
      assert_same_answers(requests)
    end

    test "make each value the term Jason decodes from the JSON JavaScript writes, alone and in a batch" do
      requests =
        for text <- @texts,
            html = ~s(<p data-json="#{escape(text)}">x</p>),
            request <- [{"parseMarkdown", text}, {"parseHTML", html}],
            do: request

      for request <- requests, do: assert_same_answers([request])
      assert_same_answers(requests)
    end
  end

  describe "the NIF" do
    @describetag backend: :nif

    test "converts a light request on the caller's scheduler, and gives :dirty for any other" do
      light = Tarnish.TestNative.convert_light({"serializeHTML", document(["x"])})
      assert light === {:ok, "<p>x</p>"}
      heavy = document(List.duplicate("x", 5_000))
      assert Tarnish.TestNative.convert_light({"serializeHTML", heavy}) === :dirty
      assert Tarnish.TestNative.convert_light({"parseHTML", @long_to_parse}) === :dirty
    end

    test "gives :not_json for a term Jason encodes through its Jason.Encoder" do
      request = {"serializeHTML", holding(text(~D[2024-01-02]))}
      assert Tarnish.TestNative.convert_light(request) === :not_json
      assert Tarnish.TestNative.convert([request, request]) === [:not_json, :not_json]
    end

    test "raises for a term Jason can't encode as Jason.encode! raises for it", %{opts: opts} do
      for term <- [self() | @refused],
          input <- [%{"v" => term}, holding(text(term)), holding_data(term)],
          operation <- ["serializeMarkdown", "serializeHTML"] do
        raised = catch_error(Jason.encode!(input))
        request = {operation, input}
        assert catch_error(Tarnish.Bridge.each([request], opts)) === raised
        assert catch_error(Tarnish.Bridge.each([{"parseHTML", "x"}, request], opts)) === raised
      end
    end

    test "starts no pool, but a lazy one, for a call that picks the Node workers" do
      assert Tarnish.Bridge.start_link(backend: :nif) === :ignore
      assert Process.whereis(Tarnish.Bridge.Pool) === nil

      start_supervised!({Tarnish.Bridge, name: :lazy, backend: :nif, lazy: true})
      assert worker_pids(:lazy) === []
      parsed = Tarnish.Bridge.parse_html("<p>x</p>", %{}, backend: :node, pool: :lazy)
      assert parsed === {:ok, document(["x"])}
    end
  end

  describe "the Node workers" do
    @describetag backend: :node

    test "keep across calls", %{opts: opts} do
      before = worker_pids(Tarnish.Bridge.Pool)
      assert length(before) === 2

      for _ <- 1..3 do
        assert Tarnish.Bridge.parse_html("<p>x</p>", %{}, opts) === {:ok, document(["x"])}
      end

      assert worker_pids(Tarnish.Bridge.Pool) === before
    end

    test "are replaced when a worker's process dies", %{opts: opts} do
      NimblePool.checkout!(Tarnish.Bridge.Pool, :bridge, fn _from, port ->
        {:os_pid, os_pid} = Port.info(port, :os_pid)
        System.cmd("kill", ["-9", to_string(os_pid)])
        {:ok, :error}
      end)

      assert Tarnish.Bridge.each(List.duplicate({"parseHTML", "<p>x</p>"}, 4), opts) ===
               List.duplicate({:ok, document(["x"])}, 4)
    end

    test "of a lazy pool start once a call needs one", %{opts: opts} do
      start_supervised!({Tarnish.Bridge, name: :lazy, backend: :node, lazy: true})
      assert worker_pids(:lazy) === []
      parsed = Tarnish.Bridge.parse_html("<p>x</p>", %{}, [pool: :lazy] ++ opts)
      assert parsed === {:ok, document(["x"])}
      assert length(worker_pids(:lazy)) === 1
    end

    test "that exit before they are ready are reported" do
      # NimblePool logs a worker that fails to start and starts another, until the pool stops.
      log =
        ExUnit.CaptureLog.capture_log(fn ->
          start_supervised!(
            {Tarnish.Bridge, name: :exits, backend: :node, size: 1, node: "false"}
          )

          catch_exit(
            NimblePool.checkout!(:exits, :bridge, fn _from, _port -> {:ok, :ok} end, 1_000)
          )

          stop_supervised!(:exits)
        end)

      assert log =~ "bridge worker exited with status 1 before it was ready"
    end
  end

  defp assert_same_answers(requests) do
    nif = Tarnish.Bridge.each(requests, backend: :nif)
    node = Tarnish.Bridge.each(requests, backend: :node)
    assert Enum.zip(requests, nif) === Enum.zip(requests, node)
  end

  defp escape(text) do
    String.replace(text, ["&", "\""], fn
      "&" -> "&amp;"
      "\"" -> "&quot;"
    end)
  end

  defp document(texts),
    do: %{"type" => "doc", "content" => Enum.map(texts, &paragraph([text(&1)]))}

  defp holding(node), do: %{"type" => "doc", "content" => [paragraph([node])]}

  # A document whose paragraph's data is `data`.
  defp holding_data(data) do
    paragraph = %{"type" => "paragraph", "attrs" => %{"data" => data}, "content" => [text("x")]}
    %{"type" => "doc", "content" => [paragraph]}
  end

  defp paragraph(content),
    do: %{"type" => "paragraph", "attrs" => %{"data" => nil}, "content" => content}

  defp text(text), do: %{"type" => "text", "text" => text}

  defp worker_pids(pool) do
    :sys.get_state(pool)
    {:links, links} = Process.info(Process.whereis(pool), :links)
    links |> Enum.filter(&is_port/1) |> Enum.map(&Port.info(&1, :os_pid)) |> Enum.sort()
  end
end
