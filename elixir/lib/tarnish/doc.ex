defmodule Tarnish.Doc do
  @moduledoc false

  # A document, as `{schema, chunks}`: the binaries of the chunks that hold it, newest first,
  # which Rust reads in place. And the JSON it was read from, whose parts its own JSON shares
  # where they're what ProseMirror writes.
  @derive {Inspect, only: []}
  @enforce_keys [:ref, :json]
  defstruct [:ref, :json]
end
