defmodule Tarnish.Doc do
  @moduledoc false

  # A document kept in Rust, and the JSON it was read from, whose parts its own JSON shares
  # where they're what ProseMirror writes.
  @derive {Inspect, only: []}
  @enforce_keys [:ref, :json]
  defstruct [:ref, :json]
end
