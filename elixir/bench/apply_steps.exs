# Times Tarnish on target/bench/document.json, which `npm run bench` writes. Each call runs back
# to back for two seconds, as a server's would, and the mean is what a call costs: a median hides
# the collections that free what earlier calls left, and the pages the allocator faults back in.
# Faults are minor page faults per call, and RSS is where the process ends up.
#
#     MIX_ENV=test mix run bench/apply_steps.exs

fixtures = Path.expand("../../fixtures/transform.json", __DIR__)
document = Path.expand("../../target/bench/document.json", __DIR__)

# prosemirror-schema-basic with prosemirror-schema-list, as prosemirror-transform's tests
# record it.
{:ok, schema} =
  fixtures
  |> File.read!()
  |> Jason.decode!(objects: :ordered_objects)
  |> Access.get("schemas")
  |> hd()
  |> Tarnish.Schema.new()

%{"doc" => json, "steps" => steps} = document |> File.read!() |> Jason.decode!()
{:ok, doc} = Tarnish.Node.from_json(schema, json)
{:ok, changed} = Tarnish.Step.apply(doc, steps)

stat = fn ->
  fields = "/proc/self/stat" |> File.read!() |> String.split()
  {String.to_integer(Enum.at(fields, 9)), String.to_integer(Enum.at(fields, 23)) * 4096}
end

run_for = fn call, milliseconds ->
  deadline = System.monotonic_time(:millisecond) + milliseconds

  Stream.repeatedly(call)
  |> Stream.take_while(fn _ -> System.monotonic_time(:millisecond) < deadline end)
  |> Enum.count()
end

sustained = fn name, call ->
  run_for.(call, 500)
  {faults, _} = stat.()
  started = System.monotonic_time(:nanosecond)
  calls = run_for.(call, 2000)
  mean = (System.monotonic_time(:nanosecond) - started) / calls / 1000
  {faulted, rss} = stat.()

  IO.puts(
    String.pad_trailing(name, 27) <>
      "#{:erlang.float_to_binary(mean, decimals: 1)} µs, " <>
      "#{:erlang.float_to_binary((faulted - faults) / calls, decimals: 2)} faults, " <>
      "RSS #{div(rss, 1024 * 1024)} MiB"
  )
end

sustained.("Node.from_json", fn -> Tarnish.Node.from_json(schema, json) end)
sustained.("Node.to_json, unchanged", fn -> Tarnish.Node.to_json(doc) end)
sustained.("Node.to_json after the steps", fn -> Tarnish.Node.to_json(changed) end)
sustained.("Node.check", fn -> Tarnish.Node.check(doc) end)
sustained.("Step.apply, 10 steps", fn -> Tarnish.Step.apply(doc, steps) end)

sustained.("from JSON, apply, to JSON", fn ->
  {:ok, doc} = Tarnish.Node.from_json(schema, json)
  {:ok, doc} = Tarnish.Step.apply(doc, steps)
  Tarnish.Node.to_json(doc)
end)
