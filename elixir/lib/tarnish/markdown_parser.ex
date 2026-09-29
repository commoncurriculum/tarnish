defmodule Tarnish.MarkdownParser do
  @moduledoc """
  Markdown to a document, as prosemirror-markdown's `MarkdownParser` parses it, by the
  application's conversions (see `Tarnish`).
  """

  @doc "Parses Markdown to a document's JSON: `{:ok, doc_json}` or `{:error, message}`."
  @spec parse(String.t(), map(), keyword()) :: Tarnish.converted()
  def parse(markdown_string, markdown_options \\ %{}, opts \\ []),
    do: Tarnish.convert_one({"parseMarkdown", markdown_string, markdown_options}, opts)
end
