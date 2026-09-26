# Times Tarnish on target/bench/document.json, which `npm run bench` writes: the medians of
# 2,000 calls, each converting the whole document from terms and back.
#
#     MIX_ENV=test mix run bench/apply_steps.exs

fixtures = Path.expand("../../fixtures/transform.json", __DIR__)
document = Path.expand("../../target/bench/document.json", __DIR__)

# prosemirror-schema-basic with prosemirror-schema-list, as prosemirror-transform's tests
# record it.
%Jason.OrderedObject{values: spec} =
  fixtures |> File.read!() |> Jason.decode!(objects: :ordered_objects) |> Access.get("schemas") |> hd()

plain = fn
  %Jason.OrderedObject{} = object -> object |> Jason.encode!() |> Jason.decode!()
  other -> other
end

spec =
  Map.new(spec, fn
    {key, %Jason.OrderedObject{values: types}} when key in ["nodes", "marks"] ->
      {key, Enum.map(types, fn {name, type_spec} -> {name, plain.(type_spec)} end)}

    {key, value} ->
      {key, plain.(value)}
  end)

{:ok, schema} = Tarnish.schema(spec)
%{"doc" => doc, "steps" => steps} = document |> File.read!() |> Jason.decode!()

median = fn call ->
  for _ <- 1..500, do: call.()
  times = for _ <- 1..2000, do: elem(:timer.tc(call), 0)
  times |> Enum.sort() |> Enum.at(1000)
end

IO.puts("apply_steps, 10 steps: #{median.(fn -> Tarnish.apply_steps(schema, doc, steps) end)} µs")
IO.puts("apply_steps, no steps: #{median.(fn -> Tarnish.apply_steps(schema, doc, []) end)} µs")
IO.puts("check:                 #{median.(fn -> Tarnish.check(schema, doc) end)} µs")
