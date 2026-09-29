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
  The steps that undo `steps`, applied to `doc`, last first, as `step.invert(doc)` gives each.
  """
  @spec invert(Tarnish.Node.t(), [Tarnish.json()]) :: {:ok, [Tarnish.json()]} | Tarnish.error()
  def invert(%Tarnish.Node{ref: ref}, steps) do
    with :dirty <- @native.invert_steps(ref, steps), do: @native.invert_steps_dirty(ref, steps)
  end
end
