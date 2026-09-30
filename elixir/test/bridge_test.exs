defmodule Tarnish.BridgeTest do
  # The backend is set for the whole VM, and a test kills a worker.
  use ExUnit.Case, async: false
  import ExUnit.CaptureLog

  @moduletag :bridge

  @worker ["node", Path.expand("support/bridge_worker.mjs", __DIR__)]

  setup do
    Application.put_env(:tarnish, Tarnish.Bridge, backend: :node)
    on_exit(fn -> Application.delete_env(:tarnish, Tarnish.Bridge) end)
    pool = start_supervised!({Tarnish.Bridge, name: nil, size: 2, command: @worker})
    %{opts: [pool: pool, size: 2]}
  end

  test "converts HTML to a document's JSON and back", %{opts: opts} do
    html = "<p>Hi <em>there</em></p>"
    assert {:ok, doc_json} = Tarnish.Bridge.parse_html(html, %{}, opts)
    assert %{"type" => "doc", "content" => [%{"type" => "paragraph"}]} = doc_json
    assert Tarnish.Bridge.serialize_html(doc_json, %{}, opts) == {:ok, html}
  end

  test "answers each request in order, across its workers", %{opts: opts} do
    requests = for n <- 1..9, do: {"parseHTML", "<p>#{n}</p>"}

    texts =
      for {:ok, doc_json} <- Tarnish.Bridge.each(requests, opts),
          do: get_in(doc_json, ["content", Access.at(0), "content", Access.at(0), "text"])

    assert texts == Enum.map(1..9, &to_string/1)
  end

  test "gives a worker's error as the answer", %{opts: opts} do
    assert Tarnish.Bridge.parse_markdown("# x", %{}, opts) ==
             {:error, "Unknown operation parseMarkdown"}
  end

  test "keeps its workers across calls", %{opts: opts} do
    before = worker_pids(opts[:pool])
    assert length(before) == 2

    for _ <- 1..3, do: assert({:ok, _} = Tarnish.Bridge.parse_html("<p>x</p>", %{}, opts))

    assert worker_pids(opts[:pool]) == before
  end

  test "replaces a worker whose process dies", %{opts: opts} do
    NimblePool.checkout!(opts[:pool], :bridge, fn _from, port ->
      {:os_pid, os_pid} = Port.info(port, :os_pid)
      System.cmd("kill", ["-9", to_string(os_pid)])
      {:ok, :error}
    end)

    assert Tarnish.Bridge.each(List.duplicate({"parseHTML", "<p>x</p>"}, 4), opts) ==
             List.duplicate({:ok, %{"type" => "doc", "content" => [paragraph("x")]}}, 4)
  end

  test "gives the error a worker answers without an id to the request it couldn't read",
       %{opts: opts} do
    requests = [{"parseHTML", "<p>a</p>"}, {"parseHTML", String.duplicate("x", 5000)}]

    assert [{:ok, _}, {:error, "Request exceeds 4096 bytes"}] =
             Tarnish.Bridge.each(requests, Keyword.put(opts, :size, 1))
  end

  test "leaves out empty options, which a worker refuses", %{opts: opts} do
    assert {:ok, _} = Tarnish.Bridge.parse_html("<p>x</p>", nil, opts)
    assert [{:ok, _}] = Tarnish.Bridge.each([{"parseHTML", "<p>x</p>", %{}}], opts)
  end

  test "reports a worker that exits before it is ready" do
    command = ["node", "-e", "process.exit(3)"]

    # NimblePool logs a worker that fails to start and starts another.
    log =
      capture_log(fn ->
        pool =
          start_supervised!({Tarnish.Bridge, name: nil, size: 1, command: command}, id: :exits)

        catch_exit(NimblePool.checkout!(pool, :bridge, fn _from, _port -> {:ok, :ok} end, 1_000))
      end)

    assert log =~ "bridge worker exited with status 3 before it was ready"
  end

  test "starts no pool when the NIF converts" do
    Application.put_env(:tarnish, Tarnish.Bridge, backend: :nif)
    assert Tarnish.Bridge.start_link(command: @worker) == :ignore
  end

  defp paragraph(text),
    do: %{"type" => "paragraph", "content" => [%{"type" => "text", "text" => text}]}

  defp worker_pids(pool) do
    :sys.get_state(pool)
    {:links, links} = Process.info(pool, :links)
    links |> Enum.filter(&is_port/1) |> Enum.map(&Port.info(&1, :os_pid)) |> Enum.sort()
  end
end
