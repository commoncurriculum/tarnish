defmodule Tarnish.NIF do
  @moduledoc """
  Declares the functions of tarnish's NIF, in the module that loads the library holding them.

  `Tarnish.Native` loads tarnish's own NIF. An application with a NIF of its own can hold
  tarnish's functions in that one library instead: its crate builds on the `tarnish-nif` crate,
  its `rustler::init!` registers tarnish's functions with its own, and its load hook calls
  `tarnish_nif::load`. The module that loads it declares them with `use Tarnish.NIF`, and
  `Tarnish` calls that module when it is configured:

      config :tarnish, native: MyApp.Native

  Then `Tarnish.Native` isn't built or loaded.

  A NIF that is `Tarnish.Bridge`'s `:nif` backend also implements `convert/1`, which takes a
  list of requests on a dirty scheduler, and `convert_light/1`, which takes one on the caller's
  and gives `:dirty` when it is too heavy for that. Each answer is `{:ok, value}`,
  `{:error, message}`, or `:not_json` for a request holding a term the NIF doesn't read.
  """

  defmacro __using__(_opts) do
    quote do
      @doc false
      def schema(_spec), do: :erlang.nif_error(:nif_not_loaded)
      @doc false
      def node_from_json(_schema, _json), do: :erlang.nif_error(:nif_not_loaded)
      @doc false
      def node_from_json_dirty(_schema, _json), do: :erlang.nif_error(:nif_not_loaded)
      @doc false
      def to_json(_doc, _json), do: :erlang.nif_error(:nif_not_loaded)
      @doc false
      def to_json_dirty(_doc, _json), do: :erlang.nif_error(:nif_not_loaded)
      @doc false
      def check(_doc), do: :erlang.nif_error(:nif_not_loaded)
      @doc false
      def check_dirty(_doc), do: :erlang.nif_error(:nif_not_loaded)
      @doc false
      def apply_steps(_doc, _steps), do: :erlang.nif_error(:nif_not_loaded)
      @doc false
      def apply_steps_dirty(_doc, _steps), do: :erlang.nif_error(:nif_not_loaded)
      @doc false
      def invert_steps(_doc, _steps), do: :erlang.nif_error(:nif_not_loaded)
      @doc false
      def invert_steps_dirty(_doc, _steps), do: :erlang.nif_error(:nif_not_loaded)
      @doc false
      def map_position(_schema, _steps, _pos, _assoc), do: :erlang.nif_error(:nif_not_loaded)
      @doc false
      def map_position_dirty(_schema, _steps, _pos, _assoc),
        do: :erlang.nif_error(:nif_not_loaded)

      @doc false
      def transform(_doc, _ops), do: :erlang.nif_error(:nif_not_loaded)
      @doc false
      def transform_dirty(_doc, _ops), do: :erlang.nif_error(:nif_not_loaded)
      @doc false
      def text_between(_doc, _from, _to, _separator, _leaf),
        do: :erlang.nif_error(:nif_not_loaded)

      @doc false
      def text_between_dirty(_doc, _from, _to, _separator, _leaf),
        do: :erlang.nif_error(:nif_not_loaded)

      @doc false
      def text_content(_doc), do: :erlang.nif_error(:nif_not_loaded)
      @doc false
      def text_content_dirty(_doc), do: :erlang.nif_error(:nif_not_loaded)

      @doc false
      def convert(_requests), do: :erlang.nif_error(:nif_not_loaded)
      @doc false
      def convert_light(_request), do: :erlang.nif_error(:nif_not_loaded)
    end
  end
end
