defmodule Tarnish.Step do
  @moduledoc """
  Steps, as ProseMirror's `Step`s, given as their JSON.
  """

  @native Application.compile_env(:tarnish, :native, Tarnish.Native)
  @compile {:no_warn_undefined, @native}

  @doc """
  Applies the steps to the document in order, as `step.apply(doc)` does each: `{:ok, doc}`, or
  the error the first step that fails gives.

  In the same call it also turns the new document into JSON and saves that JSON in `doc`, so
  `Tarnish.Node.to_json/1` afterwards just returns it and costs nothing. A socket's process
  should keep the document between messages: each message's JSON reuses the previous message's
  JSON for every node the steps didn't change.
  """
  @spec apply(Tarnish.Node.t(), [Tarnish.json()]) :: {:ok, Tarnish.Node.t()} | Tarnish.error()
  def apply(%Tarnish.Node{} = doc, []), do: {:ok, doc}

  def apply(%Tarnish.Node{ref: ref, json: json} = doc, steps) do
    of = Map.get(doc, :of)

    applied =
      with :dirty <- @native.apply_write(ref, steps, json, of),
           do: @native.apply_write_dirty(ref, steps, json, of)

    with {:ok, {ref, written}} <- applied,
         do: {:ok, Map.merge(doc, %{ref: ref, json: written, of: ref})}
  end

  @doc """
  The steps that undo `steps`, applied to `doc`, last first, as `step.invert(doc)` gives each.
  """
  @spec invert(Tarnish.Node.t(), [Tarnish.json()]) :: {:ok, [Tarnish.json()]} | Tarnish.error()
  def invert(%Tarnish.Node{ref: ref}, steps) do
    with :dirty <- @native.invert_steps(ref, steps), do: @native.invert_steps_dirty(ref, steps)
  end
end
