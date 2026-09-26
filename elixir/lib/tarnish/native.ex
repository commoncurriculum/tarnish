defmodule Tarnish.Native do
  @moduledoc false
  use Rustler, otp_app: :tarnish, crate: :tarnish_elixir, path: "../crates/tarnish_elixir"

  def schema(_spec), do: :erlang.nif_error(:nif_not_loaded)
  def check(_schema, _doc), do: :erlang.nif_error(:nif_not_loaded)
  def apply_steps(_schema, _doc, _steps), do: :erlang.nif_error(:nif_not_loaded)
  def invert_steps(_schema, _doc, _steps), do: :erlang.nif_error(:nif_not_loaded)
  def map_position(_schema, _steps, _pos, _assoc), do: :erlang.nif_error(:nif_not_loaded)
end
