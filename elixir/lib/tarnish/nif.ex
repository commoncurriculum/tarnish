defmodule Tarnish.NIF do
  @moduledoc """
  Loads a NIF on tarnish, built with Rustler, and declares tarnish's functions in the module that
  loads it. It takes Rustler's options.

  `Tarnish.Native` loads tarnish's own NIF. An application with a NIF of its own can hold
  tarnish's functions in that one library instead: its crate builds on the `tarnish-nif` crate,
  its `rustler::init!` registers tarnish's functions with its own, and its load hook calls
  `tarnish_nif::load` with the load info and its conversions. `Tarnish` calls the module that
  loads it when it is configured:

      defmodule MyApp.Native do
        use Tarnish.NIF, otp_app: :my_app, crate: "my_app_nif"
      end

      config :tarnish, native: MyApp.Native

  Then `Tarnish.Native` isn't built or loaded.

  The load info is the count of the VM's dirty CPU schedulers: `tarnish-nif` runs a batch of
  conversions on that many threads of its own.

  The library also holds `Tarnish.Bridge`'s `:nif` backend: `convert/1`, which takes a list of
  requests on a dirty scheduler, and `convert_light/1`, which takes one on the caller's and gives
  `:dirty` when it is too heavy for that. They answer with the conversions the crate's load hook
  serves; a library that serves none raises when they are called. A request is `{operation,
  input}`, or `{operation, input, options}` when the options aren't empty. Each answer is `{:ok,
  value}`, `{:error, message}`, or `:not_json` for a request holding a term the NIF doesn't read.
  """

  @doc false
  def load_data, do: :erlang.system_info(:dirty_cpu_schedulers_online)

  defmacro __using__(opts) do
    quote do
      use Rustler, unquote(Keyword.put(opts, :load_data_fun, {__MODULE__, :load_data}))

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
