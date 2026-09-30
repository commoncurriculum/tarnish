defmodule Tarnish.NIFTest do
  use ExUnit.Case, async: true

  # An application's release runs without Rustler, which `use Tarnish.NIF` loads the library
  # through, so a module there declares the functions with the macro and loads the library itself.
  @tag :tmp_dir
  test "declares the NIF's functions for a module that loads the library without Rustler", %{
    tmp_dir: tmp_dir
  } do
    script = Path.join(tmp_dir, "load.exs")

    File.write!(script, """
    [nif, library] = System.argv()
    Code.require_file(nif)

    defmodule Tarnish.TestNative do
      require Tarnish.NIF
      Tarnish.NIF.declare_functions()

      def load(library), do: :erlang.load_nif(String.to_charlist(library), Tarnish.NIF.load_data())
    end

    false = Code.ensure_loaded?(Rustler)
    :ok = Tarnish.TestNative.load(library)
    nodes = [["doc", %{"content" => "text*"}], ["text", %{}]]
    {:ok, _} = Tarnish.TestNative.schema(%{"nodes" => nodes})
    IO.write("loaded")
    """)

    nif = Path.expand("../lib/tarnish/nif.ex", __DIR__)
    library = Application.app_dir(:tarnish, "priv/native/tarnish_test_native")

    assert {"loaded", 0} = System.cmd("elixir", [script, nif, library], stderr_to_stdout: true)
  end
end
