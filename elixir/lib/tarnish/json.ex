defmodule Tarnish.JSON do
  @moduledoc false

  # `JSON.stringify` escapes a lone surrogate, which Jason refuses and an Elixir string can't hold.
  # It reads as U+FFFD, as tarnish writes it. `JSON.stringify` writes a surrogate pair unescaped,
  # so every surrogate escape in its text is a lone one; an escape is one only after an even run
  # of backslashes.
  @lone_surrogate ~r/(?<!\\)((?:\\\\)*)\\ud[89a-f][0-9a-f]{2}/i

  @doc "Decodes text `JSON.stringify` wrote, each lone surrogate as U+FFFD."
  def decode!(text) do
    case Jason.decode(text) do
      {:ok, value} -> value
      {:error, _} -> @lone_surrogate |> Regex.replace(text, "\\1\\\\ufffd") |> Jason.decode!()
    end
  end
end
