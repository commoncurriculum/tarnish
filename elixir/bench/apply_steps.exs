# Times Tarnish on target/bench/document.json, which `npm run bench` writes: the medians of
# 2,000 calls.
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
  |> Tarnish.schema()

%{"doc" => json, "steps" => steps} = document |> File.read!() |> Jason.decode!()
{:ok, doc} = Tarnish.node_from_json(schema, json)

median = fn call ->
  for _ <- 1..500, do: call.()
  times = for _ <- 1..2000, do: elem(:timer.tc(call), 0)
  times |> Enum.sort() |> Enum.at(1000)
end

round_trip = fn ->
  {:ok, doc} = Tarnish.node_from_json(schema, json)
  {:ok, doc} = Tarnish.apply_steps(doc, steps)
  Tarnish.to_json(doc)
end

IO.puts("node_from_json:           #{median.(fn -> Tarnish.node_from_json(schema, json) end)} µs")
IO.puts("to_json:                  #{median.(fn -> Tarnish.to_json(doc) end)} µs")
IO.puts("check:                    #{median.(fn -> Tarnish.check(doc) end)} µs")
IO.puts("apply_steps, 10 steps:    #{median.(fn -> Tarnish.apply_steps(doc, steps) end)} µs")
IO.puts("from JSON, apply, to JSON: #{median.(round_trip)} µs")
