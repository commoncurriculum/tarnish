defmodule Tarnish.Native do
  @moduledoc false
  use Rustler, otp_app: :tarnish, crate: :tarnish_elixir, path: "../crates/tarnish_elixir"

  def schema(_spec), do: :erlang.nif_error(:nif_not_loaded)
  def node_from_json(_schema, _json), do: :erlang.nif_error(:nif_not_loaded)
  def node_from_json_dirty(_schema, _json), do: :erlang.nif_error(:nif_not_loaded)
  def to_json(_doc, _json), do: :erlang.nif_error(:nif_not_loaded)
  def to_json_dirty(_doc, _json), do: :erlang.nif_error(:nif_not_loaded)
  def check(_doc), do: :erlang.nif_error(:nif_not_loaded)
  def check_dirty(_doc), do: :erlang.nif_error(:nif_not_loaded)
  def apply_steps(_doc, _steps), do: :erlang.nif_error(:nif_not_loaded)
  def apply_steps_dirty(_doc, _steps), do: :erlang.nif_error(:nif_not_loaded)
  def invert_steps(_doc, _steps), do: :erlang.nif_error(:nif_not_loaded)
  def invert_steps_dirty(_doc, _steps), do: :erlang.nif_error(:nif_not_loaded)
  def map_position(_schema, _steps, _pos, _assoc), do: :erlang.nif_error(:nif_not_loaded)
  def map_position_dirty(_schema, _steps, _pos, _assoc), do: :erlang.nif_error(:nif_not_loaded)
end
