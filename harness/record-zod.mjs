// Records what zod's `safeParse` gives each schema tarnish-zod ports for each input, to
// fixtures/zod.json, for tarnish-zod's tests to expect: the data, none for `undefined`, or the
// message of the error a throwing `parse` throws, which is the JSON of every issue as `ZodError`
// writes it.
//
// A schema is written as JSON tarnish-zod's tests build the same schema from: "string", "number",
// "int" or "boolean", or [kind, ...arguments], `z.<kind>` or the method of that name on the schema
// built from the first argument. A shape is a list of [key, schema] pairs, which keeps its order.
// An input is the text `JSON.parse` reads, or none for `undefined`.
import { z } from "zod"
import { outcome, writeFixture } from "./fixture.mjs"

const shape = entries => Object.fromEntries(entries.map(([key, schema]) => [key, build(schema)]))
const mask = keys => Object.fromEntries(keys.map(key => [key, true]))

function build(schema) {
  if (typeof schema === "string") return { string: z.string(), number: z.number(), int: z.int(), boolean: z.boolean() }[schema]
  const [kind, inner, argument] = schema
  switch (kind) {
    case "enum":
      return z.enum(inner)
    case "array":
      return z.array(build(inner))
    case "record":
      return z.record(z.string(), build(inner))
    case "object":
      return z.object(shape(inner))
    case "strictObject":
      return z.strictObject(shape(inner))
    case "nullable":
    case "optional":
    case "nullish":
    case "strict":
    case "partial":
      return build(inner)[kind]()
    case "default":
      return build(inner).default(argument)
    case "catch":
      return build(inner).catch(argument)
    case "pick":
    case "omit":
      return build(inner)[kind](mask(argument))
    case "extend":
      return build(inner).extend(shape(argument))
    default:
      throw new Error(`No schema of kind ${kind}`)
  }
}

// Every kind of JSON, and the numbers at the edges of `z.number()` and `z.int()`. None is past a
// double, which `JSON.parse` reads as Infinity, and tarnish-js's JSON, which holds what JSON text
// can write, as null.
const values = [
  undefined,
  "null",
  "true",
  "false",
  "0",
  "-0",
  "1",
  "-1",
  "1.5",
  "9007199254740991",
  "9007199254740992",
  "-9007199254740991",
  "-9007199254740992",
  "1e21",
  "1e308",
  '""',
  '"a"',
  '"b"',
  '"d"',
  '"1"',
  '"a\\"b"',
  "[]",
  "[1]",
  '["a"]',
  '["a",1,null,"b"]',
  "[[]]",
  "{}",
  '{"a":"x"}',
  '{"a":1}',
  '{"a":null}',
  '{"a":"x","b":1}',
  '{"a":"x","b":null,"c":true}',
  '{"c":false,"b":2,"a":"y"}',
  '{"b":2,"a":"x","z":1,"y":[]}',
  '{"a":"1","b":2,"c":"3"}',
  '{"1":"one","a":"x","0":"zero"}',
  '{"flag":true}',
  '{"flag":1,"x":1,"y":2}',
  '{"toString":"t","a":"x"}',
  '{"__proto__":"p","a":"x"}',
  '{"a":{"b":1.5},"c":[{"d":1},{"d":"x"},{}]}',
  '{"a":{"b":"x"},"c":[{"d":"y"}]}',
]

const abc = [
  ["a", "string"],
  ["b", ["nullable", "number"]],
  ["c", ["default", "boolean", false]],
]
const nested = [
  ["a", ["object", [["b", "int"]]]],
  ["c", ["array", ["object", [["d", "string"]]]]],
]

const schemas = [
  "string",
  "number",
  "int",
  "boolean",
  ["enum", ["a", "b"]],
  ["enum", ["a"]],
  ["enum", ["a\"b", "1"]],
  ["enum", ["b", "10", "a", "b", "9", "01"]],
  ["array", "string"],
  ["array", ["nullable", "int"]],
  ["array", ["catch", "string", "x"]],
  ["array", ["catch", "string"]],
  ["array", ["array", "number"]],
  ["record", "number"],
  ["record", "string"],
  ["record", ["catch", "string"]],
  ["record", ["catch", "string", "caught"]],
  ["record", ["object", [["b", "int"]]]],
  ["object", abc],
  ["object", nested],
  ["object", []],
  ["object", [["a", ["optional", "string"]]]],
  ["object", [["a", ["nullish", "string"]]]],
  ["object", [["a", ["catch", "string", "x"]]]],
  ["object", [["a", ["catch", "string"]]]],
  ["object", [["a", ["catch", ["optional", "string"], "x"]]]],
  ["object", [["a", ["optional", ["catch", "string", "x"]]]]],
  ["object", [["a", ["catch", ["nullish", "string"]]]]],
  ["object", [["a", ["default", "string", "d"]]]],
  ["object", [["a", ["optional", ["default", "string", "d"]]]]],
  ["object", [["a", ["nullable", ["default", "string", "d"]]]]],
  ["object", [["a", ["default", ["nullable", "string"], null]]]],
  ["object", [["a", ["default", ["optional", "string"], "d"]]]],
  ["object", [["a", ["catch", ["default", "string", "d"], "x"]]]],
  ["object", [["a", ["nullable", ["optional", "string"]]]]],
  ["object", [["a", ["default", ["array", "string"], []]]]],
  ["object", [["a", ["default", ["catch", "string"], "d"]]]],
  ["object", [["b", "string"], ["1", ["optional", "string"]], ["0", "int"]]],
  ["object", [["toString", ["optional", "string"]]]],
  ["object", [["constructor", ["catch", "string", "x"]], ["valueOf", ["default", "number", 0]]]],
  ["object", [["__proto__", ["optional", "string"]]]],
  ["strict", ["object", [["__proto__", ["optional", "string"]], ["a", "string"]]]],
  ["object", [["a", ["enum", ["x", "y"]]], ["b", ["optional", ["catch", ["enum", ["x"]]]]]]],
  ["strictObject", [["flag", ["optional", "boolean"]]]],
  ["strict", ["object", abc]],
  ["strict", ["object", []]],
  ["pick", ["object", abc], ["c", "a"]],
  ["omit", ["object", abc], ["b"]],
  ["partial", ["object", abc]],
  ["extend", ["object", abc], [["b", "string"], ["d", "int"]]],
  ["extend", ["pick", ["object", abc], ["a"]], [["c", ["catch", "string", "x"]]]],
  ["pick", ["object", abc], ["z"]],
  ["omit", ["object", abc], ["z"]],
  ["pick", ["strict", ["object", abc]], ["a"]],
  ["omit", ["strict", ["object", abc]], ["b"]],
  ["partial", ["strict", ["object", abc]]],
  ["extend", ["strict", ["object", abc]], [["d", "int"]]],
  ["optional", "string"],
  ["nullable", "string"],
  ["nullish", "string"],
  ["default", "string", "d"],
  ["default", ["object", abc], { a: "default" }],
  ["catch", "string", "x"],
  ["catch", "string"],
  ["catch", ["object", abc], { a: "caught" }],
  ["optional", ["default", "string", "d"]],
  ["optional", ["catch", "string", "x"]],
  ["nullable", ["default", "string", "d"]],
  ["default", ["optional", "string"], "d"],
  ["default", ["catch", "string"], "d"],
  ["nullable", ["optional", "string"]],
  ["catch", ["optional", "string"], "x"],
]

const parses = schemas.flatMap((schema, index) =>
  values.map(json => ({
    schema: index,
    ...(json === undefined ? {} : { json }),
    ...outcome(() => {
      const parsed = build(schema).safeParse(json === undefined ? undefined : JSON.parse(json))
      return parsed.success ? { data: parsed.data } : { message: parsed.error.message }
    }),
  })),
)

writeFixture("zod", { schemas, parses })
