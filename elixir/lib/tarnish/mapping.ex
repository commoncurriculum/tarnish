defmodule Tarnish.Mapping do
  @moduledoc """
  Positions mapped through the changes steps make, as ProseMirror's `Mapping` maps them.
  """

  @native Application.compile_env(:tarnish, :native, Tarnish.Native)
  @compile {:no_warn_undefined, @native}

  @doc """
  Maps a position through the steps' maps, as `new Mapping(maps).map(pos, assoc)` does.

  With `assoc` below zero, a position where content is inserted stays before it; otherwise it
  moves after it.
  """
  @spec map(Tarnish.Schema.t(), [Tarnish.json()], non_neg_integer(), integer()) ::
          {:ok, non_neg_integer()} | Tarnish.error()
  def map(schema, steps, pos, assoc \\ 1) do
    with :dirty <- @native.map_position(schema, steps, pos, assoc),
         do: @native.map_position_dirty(schema, steps, pos, assoc)
  end
end
