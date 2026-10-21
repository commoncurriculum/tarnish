# Times Tarnish on the documents `npm run bench` writes to target/bench/: document.json, read in
# prosemirror-schema-basic with prosemirror-schema-list, and nested.json and cards.json, each in
# the schema it holds, and given.json too, the card stack TARNISH_BENCH_DOC names, when
# `npm run bench` has written it with TARNISH_BENCH_DOC set.
#
# Each call runs back to back, as a server's would, and the mean is what a call costs: a median
# hides the collections that free what earlier calls left, and the pages the allocator faults
# back in. Every case of every document runs in each of TARNISH_BENCH_ROUNDS rounds (7 unless
# given), in turn, so that the machine's drift lands on all of them alike; a round warms a case
# for 100 ms, then takes the mean of TARNISH_BENCH_MS milliseconds (300 unless given) of calls.
# A row gives the median of its rounds' means and the range they span, and the median of its
# minor page faults per call. RSS is where the process ends up.
#
# The "kept node" rows are a socket process's message: the %Tarnish.Node{} it read earlier, kept
# between messages, takes one change's steps and is written back. Each call takes the next change
# in turn, so a row's mean is one message's; the Step.apply row for one step takes each step in
# turn, applied alone to the node the steps before it made, which shows what a call costs before
# any step's work.
#
# Those rows apply every message to the node as read. A socket's node is what its earlier
# messages made of it, while what Node.to_json shares is the map it read, so the "typed" rows
# keep a node that 100 messages each typed "x" into, in one place or in a place of its own, or
# that 10 did in 10 places, and time one message more, typing in the first place. The "chained"
# rows are the 100 messages a socket takes after those 100: each types "x" in one of 10 or 100
# places spread evenly, in turn, into the node the message before it made, and is written back with
# Node.to_json_kept, whose node the next message takes, so that what each shares is the map the
# message before it wrote; each call takes the next message. Their messages are the scenarios
# bench/document.mjs writes. The Step.apply_kept rows take the same messages, applied and written
# in one call, as a socket's process makes them.
#
#     MIX_ENV=test mix run bench/apply_steps.exs

rounds = String.to_integer(System.get_env("TARNISH_BENCH_ROUNDS", "7"))
milliseconds = String.to_integer(System.get_env("TARNISH_BENCH_MS", "300"))
fixtures = Path.expand("../../fixtures/transform.json", __DIR__)
bench = &Path.expand("../../target/bench/#{&1}", __DIR__)

# A schema's spec, ordered, since the order of the node and mark types matters.
schema = fn spec ->
  {:ok, schema} = Tarnish.Schema.new(spec)
  schema
end

# prosemirror-schema-basic with prosemirror-schema-list, as prosemirror-transform's tests
# record it.
basic =
  fixtures
  |> File.read!()
  |> Jason.decode!(objects: :ordered_objects)
  |> Access.get("schemas")
  |> hd()
  |> schema.()

# A file's JSON as plain maps, but for its schema's spec, whose order matters.
plain = fn value ->
  walk = fn walk, value ->
    case value do
      %Jason.OrderedObject{values: values} ->
        Map.new(values, fn {key, value} -> {key, walk.(walk, value)} end)

      list when is_list(list) ->
        Enum.map(list, &walk.(walk, &1))

      value ->
        value
    end
  end

  walk.(walk, value)
end

# Each document: a name, its schema, its JSON, the step lists that each apply to it, and its
# scenarios.
documents =
  for {file, name} <- [
        {"document.json", "document.json: 200 paragraphs"},
        {"nested.json", "nested.json: a long list and a table"},
        {"cards.json", "cards.json: a 50-card stack"},
        {"given.json", "TARNISH_BENCH_DOC: #{System.get_env("TARNISH_BENCH_DOC")}"}
      ],
      path = bench.(file),
      file != "given.json" or (System.get_env("TARNISH_BENCH_DOC") && File.exists?(path)) do
    file = path |> File.read!() |> Jason.decode!(objects: :ordered_objects)

    %{
      name: name,
      schema: if(spec = file["schema"], do: schema.(spec), else: basic),
      json: plain.(file["doc"]),
      changes: Enum.map(file["changes"], &plain.(&1["steps"])),
      scenarios: plain.(file["scenarios"])
    }
  end

stat = fn ->
  fields = "/proc/self/stat" |> File.read!() |> String.split()
  {String.to_integer(Enum.at(fields, 9)), String.to_integer(Enum.at(fields, 23)) * 4096}
end

# A call that returns the next of `items` each time, round and round.
cycle = fn items ->
  items = List.to_tuple(items)
  counter = :counters.new(1, [])

  fn ->
    :counters.add(counter, 1, 1)
    elem(items, rem(:counters.get(counter, 1), tuple_size(items)))
  end
end

run_for = fn call, milliseconds ->
  deadline = System.monotonic_time(:millisecond) + milliseconds

  Stream.repeatedly(call)
  |> Stream.take_while(fn _ -> System.monotonic_time(:millisecond) < deadline end)
  |> Enum.count()
end

cases =
  Enum.flat_map(documents, fn %{
                                name: name,
                                schema: schema,
                                json: json,
                                changes: changes,
                                scenarios: scenarios
                              } ->
    {:ok, doc} = Tarnish.Node.from_json(schema, json)

    # Each change applies to the document as read, and the last one's result is what's written.
    apply_all = fn doc, changes ->
      Enum.reduce(changes, nil, fn steps, _ ->
        {:ok, changed} = Tarnish.Step.apply(doc, steps)
        changed
      end)
    end

    changed = apply_all.(doc, changes)
    count = changes |> Enum.map(&length/1) |> Enum.sum()

    applied =
      case changes do
        [_] -> "Step.apply, #{count} steps"
        _ -> "Step.apply, #{length(changes)} changes (#{count} steps)"
      end

    # Tarnish has no JSON text path in Elixir, so the text rows time Jason around it, as a caller
    # holding text would.
    text = Jason.encode!(json)
    changes_text = Enum.map(changes, &Jason.encode!/1)

    # Each change with what its steps make of the kept node, and each step with the node the
    # steps before it in its change made.
    message =
      Enum.map(changes, fn steps ->
        {:ok, changed} = Tarnish.Step.apply(doc, steps)
        {steps, changed}
      end)
      |> cycle.()

    one_step =
      Enum.flat_map(changes, fn steps ->
        steps
        |> Enum.map_reduce(doc, fn step, before ->
          {:ok, after_step} = Tarnish.Step.apply(before, [step])
          {{before, [step]}, after_step}
        end)
        |> elem(0)
      end)
      |> cycle.()

    # The node each message of `messages` makes of the one the message before it made, from
    # `start`, each with the node it was applied to and its steps.
    fold = fn start, messages ->
      messages
      |> Enum.map_reduce(start, fn steps, before ->
        {:ok, changed} = Tarnish.Step.apply(before, steps)
        {{before, steps, changed}, changed}
      end)
      |> elem(0)
    end

    last = fn folded -> folded |> List.last() |> elem(2) end

    # The first message of typed_once_100 types in the first place.
    first = hd(scenarios["typed_once_100"])

    message_after = fn kept ->
      fn ->
        {:ok, changed} = Tarnish.Step.apply(kept, first)
        Tarnish.Node.to_json(changed)
      end
    end

    in_one_place = last.(fold.(doc, scenarios["typed_once_100"]))
    in_10_places = last.(fold.(doc, scenarios["typed_10"]))
    in_100_places = last.(fold.(doc, scenarios["typed_100"]))
    {:ok, typed_once_more} = Tarnish.Step.apply(in_100_places, first)

    # The 100 messages after in_100_places, each with the node the message before it made and
    # wrote.
    chained = fn count ->
      {_, kept} = Tarnish.Node.to_json_kept(in_100_places)

      scenarios["chained_#{count}"]
      |> Enum.map_reduce(kept, fn steps, before ->
        {:ok, changed} = Tarnish.Step.apply(before, steps)
        {_, written} = Tarnish.Node.to_json_kept(changed)
        {{before, steps, changed}, written}
      end)
      |> elem(0)
      |> cycle.()
    end

    chained_in_10 = chained.(10)
    chained_in_100 = chained.(100)
    chained_in_100_written = chained.(100)

    chained_message = fn chain ->
      fn ->
        {before, steps, _} = chain.()
        {:ok, changed} = Tarnish.Step.apply(before, steps)
        Tarnish.Node.to_json_kept(changed)
      end
    end

    chained_kept = fn chain ->
      fn ->
        {before, steps, _} = chain.()
        Tarnish.Step.apply_kept(before, steps)
      end
    end

    [
      {"Node.from_json", fn -> Tarnish.Node.from_json(schema, json) end},
      {"Jason, then Node.from_json",
       fn -> Tarnish.Node.from_json(schema, Jason.decode!(text)) end},
      {"Node.to_json, unchanged", fn -> Tarnish.Node.to_json(doc) end},
      {"Node.to_json after the steps", fn -> Tarnish.Node.to_json(changed) end},
      {"JSON text after the steps", fn -> Jason.encode!(Tarnish.Node.to_json(changed)) end},
      {"Node.check", fn -> Tarnish.Node.check(doc) end},
      {applied, fn -> apply_all.(doc, changes) end},
      {"from JSON, apply, to JSON",
       fn ->
         {:ok, doc} = Tarnish.Node.from_json(schema, json)
         Tarnish.Node.to_json(apply_all.(doc, changes))
       end},
      {"JSON text in and out, Jason",
       fn ->
         {:ok, doc} = Tarnish.Node.from_json(schema, Jason.decode!(text))

         Jason.encode!(
           Tarnish.Node.to_json(apply_all.(doc, Enum.map(changes_text, &Jason.decode!/1)))
         )
       end},
      {"kept node: one message",
       fn ->
         {steps, _} = message.()
         {:ok, changed} = Tarnish.Step.apply(doc, steps)
         Tarnish.Node.to_json(changed)
       end},
      {"kept node: its Step.apply",
       fn ->
         {steps, _} = message.()
         Tarnish.Step.apply(doc, steps)
       end},
      {"kept node: its Node.to_json",
       fn ->
         {_, changed} = message.()
         Tarnish.Node.to_json(changed)
       end},
      {"kept node: Step.apply, one step",
       fn ->
         {before, step} = one_step.()
         Tarnish.Step.apply(before, step)
       end},
      {"kept node, typed in 1 place 100 times: a message", message_after.(in_one_place)},
      {"kept node, typed in 10 places: a message", message_after.(in_10_places)},
      {"kept node, typed in 100 places: a message", message_after.(in_100_places)},
      {"kept node, typed in 100 places: its Node.to_json",
       fn -> Tarnish.Node.to_json(typed_once_more) end},
      {"kept node, chained in 10 places: a message", chained_message.(chained_in_10)},
      {"kept node, chained in 100 places: a message", chained_message.(chained_in_100)},
      {"kept node, chained in 10 places: Step.apply_kept", chained_kept.(chained_in_10)},
      {"kept node, chained in 100 places: Step.apply_kept", chained_kept.(chained_in_100)},
      {"kept node, chained in 100 places: its to_json_kept",
       fn ->
         {_, _, changed} = chained_in_100_written.()
         Tarnish.Node.to_json_kept(changed)
       end}
    ]
    |> Enum.map(fn {row, call} -> {name, row, call} end)
  end)

# One round's mean µs per call of a case, and its faults per call.
slot = fn call ->
  run_for.(call, 100)
  {faults, _} = stat.()
  started = System.monotonic_time(:nanosecond)
  calls = run_for.(call, milliseconds)
  mean = (System.monotonic_time(:nanosecond) - started) / calls / 1000
  {faulted, _} = stat.()
  {mean, (faulted - faults) / calls}
end

measured =
  Enum.reduce(1..rounds, Map.new(cases, fn {name, row, _} -> {{name, row}, []} end), fn _, acc ->
    Enum.reduce(cases, acc, fn {name, row, call}, acc ->
      Map.update!(acc, {name, row}, &[slot.(call) | &1])
    end)
  end)

median = fn list -> list |> Enum.sort() |> Enum.at(div(length(list), 2)) end

format = fn
  value when value < 10 -> :erlang.float_to_binary(value, decimals: 1)
  value -> :erlang.float_to_binary(value, decimals: 0)
end

IO.puts("µs per call: the median of #{rounds} rounds' means, and the range of the rounds")

cases
|> Enum.chunk_by(fn {name, _, _} -> name end)
|> Enum.each(fn [{name, _, _} | _] = rows ->
  IO.puts("\n" <> name)

  Enum.each(rows, fn {_, row, _} ->
    {means, faults} = Enum.unzip(measured[{name, row}])

    IO.puts(
      "  " <>
        String.pad_trailing(row, 52) <>
        String.pad_leading(format.(median.(means)), 7) <>
        "   " <>
        String.pad_trailing("#{format.(Enum.min(means))}..#{format.(Enum.max(means))}", 14) <>
        "#{:erlang.float_to_binary(median.(faults), decimals: 2)} faults"
    )
  end)
end)

{_, rss} = stat.()
IO.puts("\nRSS #{div(rss, 1024 * 1024)} MiB")
