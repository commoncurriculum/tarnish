exclude = if System.get_env("TARNISH_BENCH_DOC"), do: [], else: [:bench_doc]
ExUnit.start(exclude: exclude)
