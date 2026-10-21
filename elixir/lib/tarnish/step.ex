defmodule Tarnish.Step do
  @moduledoc """
  Steps, as ProseMirror's `Step`s, given as their JSON.
  """

  @native Application.compile_env(:tarnish, :native, Tarnish.Native)
  @compile {:no_warn_undefined, @native}

  @doc "Applies the steps to the document in order, as `step.apply(doc)` does each."
  @spec apply(Tarnish.Node.t(), [Tarnish.json()]) :: {:ok, Tarnish.Node.t()} | Tarnish.error()
  def apply(%Tarnish.Node{ref: ref} = doc, steps) do
    applied =
      with :dirty <- @native.apply_steps(ref, steps), do: @native.apply_steps_dirty(ref, steps)

    with {:ok, ref} <- applied, do: {:ok, %{doc | ref: ref}}
  end

  @doc """
  Applies the steps, as `apply/2` does, and writes the document they make, as
  `Tarnish.Node.to_json_kept/1` does, in one call: `{:ok, json, node}`, the node keeping that
  JSON as its base, or the error `apply/2` gives.

  A socket's process, which keeps a node between messages, applies each message with this. The
  JSON shares what the node's base holds for every node the steps left, even where the document
  they make takes those nodes in a chunk of its own, which `to_json_kept/1` writes again. What it
  gives is what `apply/2` and then `to_json_kept/1` give, term for term.
  """
  @spec apply_kept(Tarnish.Node.t(), [Tarnish.json()]) ::
          {:ok, Tarnish.json(), Tarnish.Node.t()} | Tarnish.error()
  def apply_kept(%Tarnish.Node{ref: ref, json: json} = doc, steps) do
    case {steps, Map.get(doc, :base)} do
      {[], {^ref, written}} ->
        {:ok, written, doc}

      {steps, base} ->
        applied =
          with :dirty <- @native.apply_write(ref, steps, json, base),
               do: @native.apply_write_dirty(ref, steps, json, base)

        with {:ok, {ref, written}} <- applied,
             do: {:ok, written, Map.put(%{doc | ref: ref}, :base, {ref, written})}
    end
  end

  @doc """
  The steps that undo `steps`, applied to `doc`, last first, as `step.invert(doc)` gives each.
  """
  @spec invert(Tarnish.Node.t(), [Tarnish.json()]) :: {:ok, [Tarnish.json()]} | Tarnish.error()
  def invert(%Tarnish.Node{ref: ref}, steps) do
    with :dirty <- @native.invert_steps(ref, steps), do: @native.invert_steps_dirty(ref, steps)
  end
end
