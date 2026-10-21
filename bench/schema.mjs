// Schemas as the bench files hold them: a spec as JSON, which drops its functions, toDOM,
// getAttrs and the like, and a schema read back from one.
import { Schema } from "prosemirror-model"

// A schema's spec as JSON.
export const specOf = (schema) => ({
  topNode: schema.topNodeType.name,
  nodes: schema.spec.nodes.toObject(),
  marks: schema.spec.marks.toObject(),
})

// A spec as data says an attribute's default is undefined with `hasDefault` and no `default`, as
// tarnish reads it, since JSON has no undefined. ProseMirror reads `default: undefined`.
export const fromData = (spec) => {
  const types = (types = {}) =>
    Object.fromEntries(
      Object.entries(types).map(([name, type]) => [
        name,
        type.attrs
          ? {
              ...type,
              attrs: Object.fromEntries(
                Object.entries(type.attrs).map(([attr, { hasDefault, ...rest }]) => [
                  attr,
                  hasDefault && !Object.hasOwn(rest, "default") ? { ...rest, default: undefined } : rest,
                ]),
              ),
            }
          : type,
      ]),
    )
  return new Schema({ ...spec, nodes: types(spec.nodes), marks: types(spec.marks) })
}
