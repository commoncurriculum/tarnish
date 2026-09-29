defmodule Tarnish.DOMParser do
  @moduledoc """
  HTML to a document, as ProseMirror's `DOMParser` parses it, by the application's conversions
  (see `Tarnish`).
  """

  @doc "Parses HTML to a document's JSON: `{:ok, doc_json}` or `{:error, message}`."
  @spec parse(String.t(), keyword()) :: Tarnish.converted()
  def parse(html_string, opts \\ []), do: Tarnish.convert_one({"parseHTML", html_string}, opts)
end
