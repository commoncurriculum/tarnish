defmodule Tarnish.MixProject do
  use Mix.Project

  def project do
    [
      app: :tarnish,
      version: "0.1.0",
      elixir: "~> 1.15",
      start_permanent: Mix.env() == :prod,
      deps: deps(),
      description: "ProseMirror's document model and transforms, in Rust, for Elixir"
    ]
  end

  def application do
    [extra_applications: [:logger]]
  end

  defp deps do
    [
      {:rustler, "~> 0.38.0", runtime: false},
      {:jason, "~> 1.4"},
      # Tarnish.Bridge's pool of workers
      {:nimble_pool, "~> 1.1"}
    ]
  end
end
