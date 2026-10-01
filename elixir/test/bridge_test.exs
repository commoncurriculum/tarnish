defmodule Tarnish.BridgeTest do
  use ExUnit.Case, async: true

  @moduletag :bridge

  @invalid "Unknown operation or invalid input"
  @no_options "The HTML conversions take no options"

  # The tree builder takes time quadratic in the count of formatting elements open.
  @long_to_parse String.duplicate("<b><i>", 600) <> "x"

  setup %{backend: backend} do
    start_supervised!({Tarnish.Bridge, backend: backend})
    %{opts: [backend: backend]}
  end

  for backend <- [:node, :nif] do
    describe "#{backend}:" do
      @describetag backend: backend

      test "converts HTML to a document's JSON and back", %{opts: opts} do
        html = "<p>Hi <em>there</em></p>"
        assert {:ok, doc_json} = Tarnish.Bridge.parse_html(html, %{}, opts)

        assert doc_json == %{
                 "type" => "doc",
                 "content" => [
                   %{
                     "type" => "paragraph",
                     "content" => [
                       %{"type" => "text", "text" => "Hi "},
                       %{"type" => "text", "marks" => [%{"type" => "em"}], "text" => "there"}
                     ]
                   }
                 ]
               }

        assert Tarnish.Bridge.serialize_html(doc_json, %{}, opts) == {:ok, html}
      end

      test "spreads a batch over its workers or threads, and keeps its order", %{opts: opts} do
        requests = for n <- 1..20, do: {"parseHTML", "<p>#{n}</p>"}

        texts =
          for {:ok, doc_json} <- Tarnish.Bridge.each(requests, opts),
              do: get_in(doc_json, ["content", Access.at(0), "content", Access.at(0), "text"])

        assert texts == Enum.map(1..20, &to_string/1)
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

        assert Tarnish.Bridge.each(requests, opts) == List.duplicate({:error, @invalid}, 8)
        assert Tarnish.Bridge.parse_html("<p>x</p>", %{}, opts) == {:ok, document(["x"])}
      end

      test "gives a conversion its options, which it checks before it reads the document",
           %{opts: opts} do
        nonesuch = %{"type" => "nonesuch"}
        assert Tarnish.Bridge.parse_html("<p>x</p>", %{"a" => 1}, opts) == {:error, @no_options}
        assert Tarnish.Bridge.serialize_html(nonesuch, %{"a" => 1}, opts) == {:error, @no_options}

        assert Tarnish.Bridge.serialize_html(nonesuch, %{}, opts) ==
                 {:error, "Unknown node type: nonesuch"}
      end

      test "leaves out empty options, which a conversion that takes none refuses", %{opts: opts} do
        assert {:ok, _} = Tarnish.Bridge.parse_html("<p>x</p>", nil, opts)
        assert [{:ok, _}] = Tarnish.Bridge.each([{"parseHTML", "<p>x</p>", %{}}], opts)
      end

      test "reads a lone surrogate in an answer as U+FFFD, and keeps a backslash before text like its escape",
           %{opts: opts} do
        assert Tarnish.Bridge.parse_markdown("x", %{}, opts) ==
                 {:error, "No Markdown: �\\😀\\ud800"}
      end

      test "reads a request as Jason encodes it", %{opts: opts} do
        expected = Tarnish.Bridge.serialize_html(document(["Fïrst ✓"]), %{}, opts)
        assert expected == {:ok, "<p>Fïrst ✓</p>"}

        # An atom past Latin-1, whose name the VM gives out only in the external format.
        text = %{type: :text, text: :"Fïrst ✓"}
        paragraph = Jason.OrderedObject.new([{"type", "paragraph"}, {"content", [text]}])
        as_terms = %{type: :doc, content: [paragraph]}

        assert Tarnish.Bridge.serialize_html(as_terms, %{}, opts) == expected
        requests = [{"serializeHTML", as_terms}, {"serializeHTML", as_terms}]
        assert Tarnish.Bridge.each(requests, opts) == [expected, expected]
      end

      test "reads a map's keys as Jason writes them", %{opts: opts} do
        expected = Tarnish.Bridge.serialize_html(document(["First"]), %{}, opts)

        # A key written twice keeps its first place and its last value.
        twice = %{:text => "Old", "text" => "First", "type" => "text"}
        # An atom key past Latin-1, after a key already read, has the map read again whole.
        past_latin1 = %{:a => 1, :"✓" => 2, "text" => "First", "type" => "text"}

        for node <- [twice, past_latin1] do
          request = {"serializeHTML", holding(node)}
          assert Tarnish.Bridge.each([request], opts) == [expected]
          assert Tarnish.Bridge.each([request, request], opts) == [expected, expected]
        end
      end

      test "reads a struct as its Jason.Encoder writes it", %{opts: opts} do
        expected = Tarnish.Bridge.serialize_html(document(["2024-01-02"]), %{}, opts)
        as_struct = holding(text(~D[2024-01-02]))

        assert Tarnish.Bridge.serialize_html(as_struct, %{}, opts) == expected
        requests = [{"serializeHTML", as_struct}, {"serializeHTML", document(["2024-01-02"])}]
        assert Tarnish.Bridge.each(requests, opts) == [expected, expected]
      end

      test "converts a document of any size alone as in a batch", %{opts: opts} do
        for paragraphs <- [1, 5_000] do
          sized = document(List.duplicate("x", paragraphs))
          assert {:ok, html} = Tarnish.Bridge.serialize_html(sized, %{}, opts)
          requests = [{"serializeHTML", sized}, {"serializeHTML", sized}]
          assert [{:ok, ^html}, {:ok, ^html}] = Tarnish.Bridge.each(requests, opts)
        end
      end

      test "parses a text that takes long for its length alone as in a batch", %{opts: opts} do
        request = {"parseHTML", @long_to_parse}
        [answer] = Tarnish.Bridge.each([request], opts)
        assert {:ok, _} = answer
        assert Tarnish.Bridge.each([request, request], opts) == [answer, answer]
      end
    end
  end

  describe "the NIF" do
    @describetag backend: :nif

    test "converts a light request on the caller's scheduler, and gives :dirty for any other" do
      assert {:ok, _} = Tarnish.TestNative.convert_light({"serializeHTML", document(["x"])})
      heavy = document(List.duplicate("x", 5_000))
      assert Tarnish.TestNative.convert_light({"serializeHTML", heavy}) == :dirty
      assert Tarnish.TestNative.convert_light({"parseHTML", @long_to_parse}) == :dirty
    end

    test "gives :not_json for a term Jason encodes through its Jason.Encoder, and raises for one it can't",
         %{opts: opts} do
      request = {"serializeHTML", holding(text(~D[2024-01-02]))}
      assert Tarnish.TestNative.convert_light(request) == :not_json
      assert Tarnish.TestNative.convert([request, request]) == [:not_json, :not_json]

      assert_raise Protocol.UndefinedError, fn ->
        Tarnish.Bridge.serialize_html(holding({"First"}), %{}, opts)
      end
    end

    test "starts no pool" do
      assert Tarnish.Bridge.start_link(backend: :nif) == :ignore
      assert Process.whereis(Tarnish.Bridge.Pool) == nil
    end
  end

  describe "the Node workers" do
    @describetag backend: :node

    test "keep across calls", %{opts: opts} do
      before = worker_pids(Tarnish.Bridge.Pool)
      assert length(before) == 2

      for _ <- 1..3, do: assert({:ok, _} = Tarnish.Bridge.parse_html("<p>x</p>", %{}, opts))

      assert worker_pids(Tarnish.Bridge.Pool) == before
    end

    test "are replaced when a worker's process dies", %{opts: opts} do
      NimblePool.checkout!(Tarnish.Bridge.Pool, :bridge, fn _from, port ->
        {:os_pid, os_pid} = Port.info(port, :os_pid)
        System.cmd("kill", ["-9", to_string(os_pid)])
        {:ok, :error}
      end)

      assert Tarnish.Bridge.each(List.duplicate({"parseHTML", "<p>x</p>"}, 4), opts) ==
               List.duplicate({:ok, document(["x"])}, 4)
    end

    test "of a lazy pool start once a call needs one", %{opts: opts} do
      start_supervised!({Tarnish.Bridge, name: :lazy, backend: :node, lazy: true})
      assert worker_pids(:lazy) == []
      assert {:ok, _} = Tarnish.Bridge.parse_html("<p>x</p>", %{}, [pool: :lazy] ++ opts)
      assert length(worker_pids(:lazy)) == 1
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

  defp document(texts),
    do: %{"type" => "doc", "content" => Enum.map(texts, &paragraph([text(&1)]))}

  defp holding(node), do: %{"type" => "doc", "content" => [paragraph([node])]}

  defp paragraph(content), do: %{"type" => "paragraph", "content" => content}

  defp text(text), do: %{"type" => "text", "text" => text}

  defp worker_pids(pool) do
    :sys.get_state(pool)
    {:links, links} = Process.info(Process.whereis(pool), :links)
    links |> Enum.filter(&is_port/1) |> Enum.map(&Port.info(&1, :os_pid)) |> Enum.sort()
  end
end
