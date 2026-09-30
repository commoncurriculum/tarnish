defmodule Tarnish.Bridge.Pool do
  @moduledoc """
  `Tarnish.Bridge`'s Node backend: a pool of worker processes, one port each.

  A worker writes `{"ready":true}` once it can answer, then answers each line holding
  `{"id", "operation", "input", "options"}` with a line holding `{"id", "result"}` or
  `{"id", "error"}`, in the order the requests came.
  """

  @behaviour NimblePool

  @default_size 2
  @default_timeout 30_000
  @ready_timeout 60_000
  @max_frame_bytes 16 * 1024 * 1024

  @doc false
  def start_link(opts) do
    [executable | args] = command(opts)

    NimblePool.start_link(
      worker: {__MODULE__, %{executable: executable, args: args}},
      pool_size: Keyword.get(opts, :size, configured(:size, @default_size)),
      name: Keyword.get(opts, :name, __MODULE__)
    )
  end

  # Sends the requests to the workers, a share to each, and gives their answers in order. A
  # request has options only when they aren't empty, as `Tarnish.Bridge.each` leaves them.
  @doc false
  @spec run([Tarnish.Bridge.request()], keyword()) :: [Tarnish.Bridge.result()]
  def run(requests, opts) do
    size = Keyword.get(opts, :size, configured(:size, @default_size))
    per_worker = max(ceil(length(requests) / size), 1)

    requests
    |> Enum.chunk_every(per_worker)
    |> Task.async_stream(&call_worker(&1, opts),
      max_concurrency: size,
      ordered: true,
      timeout: :infinity
    )
    |> Enum.flat_map(fn {:ok, answers} -> answers end)
  end

  # A worker takes every frame of its share before it answers any, which saves a round trip per
  # request.
  defp call_worker(requests, opts) do
    pool = Keyword.get(opts, :pool, __MODULE__)
    timeout = Keyword.get(opts, :timeout, @default_timeout)

    NimblePool.checkout!(
      pool,
      :bridge,
      fn _from, port ->
        numbered = Enum.with_index(requests)

        Enum.each(numbered, fn {request, id} -> Port.command(port, [encode(request, id), ?\n]) end)

        results =
          Enum.map(numbered, fn {_request, id} -> decode(receive_frame(port, timeout), id) end)

        # Port.connect links the borrower; unlink before its Task exits and closes the worker.
        Process.unlink(port)
        {results, :ok}
      end,
      timeout
    )
  end

  defp encode({operation, input}, id),
    do: Jason.encode_to_iodata!(%{id: id, operation: operation, input: input})

  defp encode({operation, input, options}, id),
    do: Jason.encode_to_iodata!(%{id: id, operation: operation, input: input, options: options})

  # A worker answers a frame it can't read, such as one past its size limit, without an id.
  # Answers come in order, so it is this request's.
  defp decode(frame, id) do
    case json(frame) do
      %{"id" => ^id, "result" => result} -> {:ok, result}
      %{"id" => ^id, "error" => message} -> {:error, message}
      %{"id" => nil, "error" => message} -> {:error, message}
      response -> raise "bridge worker answered request #{id} with id #{inspect(response["id"])}"
    end
  end

  # `JSON.stringify` escapes a lone surrogate, which Jason refuses and an Elixir string can't hold.
  # It reads as U+FFFD, as tarnish writes it. `JSON.stringify` writes a surrogate pair unescaped,
  # so every surrogate escape in a frame is a lone one.
  @lone_surrogate ~r/(?<!\\)((?:\\\\)*)\\ud[89a-f][0-9a-f]{2}/i

  defp json(frame) do
    case Jason.decode(frame) do
      {:ok, value} -> value
      {:error, _} -> @lone_surrogate |> Regex.replace(frame, "\\1\\\\ufffd") |> Jason.decode!()
    end
  end

  defp receive_frame(port, timeout) do
    receive do
      {^port, {:data, {:eol, line}}} -> line
      {^port, {:data, {:noeol, part}}} -> part <> receive_frame(port, timeout)
      {^port, {:exit_status, status}} -> raise "bridge worker exited with status #{status}"
    after
      timeout -> raise "bridge worker timed out after #{timeout}ms"
    end
  end

  defp command(opts) do
    case Keyword.get(opts, :command, configured(:command, nil)) do
      [executable | args] ->
        found =
          System.find_executable(executable) ||
            raise(ArgumentError, "bridge executable not found: #{executable}")

        [found | args]

      nil ->
        raise ArgumentError,
              "no bridge command: pass :command, or set config :tarnish, Tarnish.Bridge, command: [...]"
    end
  end

  defp configured(key, default),
    do: Keyword.get(Application.get_env(:tarnish, Tarnish.Bridge, []), key, default)

  @impl NimblePool
  def init_worker(%{executable: executable, args: args} = state) do
    port =
      Port.open({:spawn_executable, executable}, [
        :binary,
        :exit_status,
        {:args, args},
        {:line, @max_frame_bytes}
      ])

    receive do
      {^port, {:data, {:eol, line}}} ->
        if Jason.decode(line) != {:ok, %{"ready" => true}} do
          Port.close(port)
          raise "bridge worker's first line was #{inspect(line)}, not {\"ready\":true}"
        end

        {:ok, port, state}

      {^port, {:exit_status, status}} ->
        raise "bridge worker exited with status #{status} before it was ready"
    after
      @ready_timeout ->
        Port.close(port)
        raise "bridge worker did not become ready within #{@ready_timeout}ms"
    end
  end

  @impl NimblePool
  def handle_checkout(:bridge, {pid, _ref}, port, state) do
    Port.connect(port, pid)
    {:ok, port, port, state}
  rescue
    ArgumentError -> {:remove, :closed, state}
  end

  @impl NimblePool
  def handle_checkin(:ok, _from, port, state) do
    Port.connect(port, self())
    {:ok, port, state}
  rescue
    ArgumentError -> {:remove, :closed, state}
  end

  def handle_checkin(:error, _from, _port, state), do: {:remove, :closed, state}
  def handle_checkin({:error, _reason}, _from, _port, state), do: {:remove, :closed, state}

  @impl NimblePool
  def handle_info({port, {:exit_status, _status}}, port), do: {:remove, :closed}
  def handle_info(_message, port), do: {:ok, port}

  @impl NimblePool
  def terminate_worker(_reason, port, state) do
    Port.close(port)
    {:ok, state}
  catch
    :error, :badarg -> {:ok, state}
  end
end
