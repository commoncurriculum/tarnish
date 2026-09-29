defmodule Tarnish.DOMSerializer do
  @moduledoc """
  A document to HTML, as ProseMirror's `DOMSerializer` renders it, by the application's
  conversions (see `Tarnish`).
  """

  @doc "Serializes a document's JSON to HTML: `{:ok, html_string}` or `{:error, message}`."
  @spec serialize(Tarnish.json(), keyword()) :: Tarnish.converted()
  def serialize(doc_json, opts \\ []), do: Tarnish.convert_one({"serializeHTML", doc_json}, opts)
end
