defmodule Tarnish.Native do
  @moduledoc false

  # Neither built nor loaded when `config :tarnish, native:` names an application's own NIF.
  @standalone Application.compile_env(:tarnish, :native, __MODULE__) == __MODULE__

  use Rustler,
    otp_app: :tarnish,
    crate: :tarnish_elixir,
    path: "../crates/tarnish_elixir",
    skip_compilation?: not @standalone,
    lib: @standalone

  use Tarnish.NIF
end
