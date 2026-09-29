defmodule Tarnish.MarkdownSerializer do
  @moduledoc """
  A document to Markdown, as prosemirror-markdown's `MarkdownSerializer` writes it, by the
  application's conversions (see `Tarnish`).
  """

  @doc "Serializes a document's JSON to Markdown: `{:ok, markdown_string}` or `{:error, message}`."
  @spec serialize(Tarnish.json(), map(), keyword()) :: Tarnish.converted()
  def serialize(doc_json, markdown_options \\ %{}, opts \\ []),
    do: Tarnish.convert_one({"serializeMarkdown", doc_json, markdown_options}, opts)
end
