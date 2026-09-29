defmodule Tarnish.Schema do
  @moduledoc """
  A schema, as ProseMirror's `Schema`: built once from its spec, and kept in Rust.
  """

  @native Application.compile_env(:tarnish, :native, Tarnish.Native)
  @compile {:no_warn_undefined, @native}

  @opaque t :: reference()

  @doc """
  Builds a schema from its spec, as `new Schema(spec)` does: a map of `"nodes"`, `"marks"` and
  `"topNode"`, as ProseMirror's `SchemaSpec`.

  The order of the node and mark types matters: it decides which type comes first in a group,
  and how marks sort. So `"nodes"` and `"marks"` are lists of `{name, spec}` pairs, or
  `Jason.OrderedObject`s, which keep their order where a map doesn't.
  """
  @spec new(map()) :: {:ok, t()} | Tarnish.error()
  def new(spec) do
    spec
    |> Map.new(fn {key, value} -> {to_string(key), value} end)
    |> in_order("nodes")
    |> in_order("marks")
    |> @native.schema()
  end

  defp in_order(spec, key) do
    case spec do
      %{^key => types} when is_list(types) ->
        %{spec | key => Enum.map(types, &pair/1)}

      %{^key => types} when is_map(types) and not is_struct(types) ->
        raise ArgumentError,
              "#{key} must be a list of {name, spec} pairs, since a map doesn't keep their order"

      _ ->
        spec
    end
  end

  defp pair({name, type}), do: [to_string(name), type]
  defp pair([name, type]), do: [to_string(name), type]
end
