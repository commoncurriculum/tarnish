defmodule Tarnish.Transform do
  @moduledoc """
  Changes made on the server, as a ProseMirror `Transform` makes them: its document and the
  steps it made, as `tr.doc` and `tr.steps`.
  """

  @native Application.compile_env(:tarnish, :native, Tarnish.Native)
  @compile {:no_warn_undefined, @native}

  @enforce_keys [:doc, :steps]
  defstruct [:doc, :steps]

  @type t :: %__MODULE__{doc: Tarnish.Node.t(), steps: [Tarnish.json()]}

  @doc """
  Applies `ops` in order to one `new Transform(doc)`, and gives the changed document and the
  steps the transform made, which editors can apply.

  An op is a map naming a `Transform` method in `"op"`, with the method's arguments by the
  names ProseMirror gives them. Nodes, fragments, slices, marks and steps are their JSON, and
  node and mark types their names:

      Tarnish.Transform.new(doc, [
        %{"op" => "addMark", "from" => 1, "to" => 6, "mark" => %{"type" => "em"}},
        %{"op" => "setBlockType", "from" => 1, "type" => "heading", "attrs" => %{"level" => 2}}
      ])

  The methods are `replace`, `replaceWith`, `delete`, `insert`, `replaceRange`,
  `replaceRangeWith`, `deleteRange`, `addMark`, `removeMark`, `addNodeMark`, `removeNodeMark`,
  `setNodeMarkup`, `setNodeAttribute`, `setDocAttribute`, `setBlockType`, `lift`, `wrap`,
  `join`, `split`, `clearIncompatible`, `step` and `maybeStep`.

    * `removeMark` and `removeNodeMark` take a mark's JSON or a mark type's name.
    * `lift` and `wrap` take `"from"`, `"to"` and an optional `"depth"` for their range, which
      is `$from.blockRange($to)` without a depth. Without a `"target"`, `lift` lifts to
      `liftTarget`'s. Without `"wrappers"`, `wrap` wraps in `"nodeType"` with `"attrs"`, as
      `findWrapping` finds.
    * Positions are whole numbers, and a range's `"to"` can't come before its `"from"`.

  An op that fails fails the whole call, with the error ProseMirror throws.
  """
  @spec new(Tarnish.Node.t(), [Tarnish.json()]) :: {:ok, t()} | Tarnish.error()
  def new(%Tarnish.Node{ref: ref} = doc, ops) do
    transformed =
      with :dirty <- @native.transform(ref, ops), do: @native.transform_dirty(ref, ops)

    with {:ok, ref, steps} <- transformed,
         do: {:ok, %__MODULE__{doc: %{doc | ref: ref}, steps: steps}}
  end
end
