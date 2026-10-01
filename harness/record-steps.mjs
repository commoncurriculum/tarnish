// Records what a step type of an application's own does in the real ProseMirror packages, to
// fixtures/steps.json, for a Rust test that defines the same step to expect: applying it alone
// and among ProseMirror's steps, its maps, inverses, mapping and merging, and the errors of
// `Step.jsonID` and `Step.fromJSON`.
import { Fragment, Node, Schema, Slice } from "prosemirror-model"
import { Mapping, ReplaceStep, Step, StepMap, StepResult, Transform } from "prosemirror-transform"
import { outcome, writeFixture } from "./fixture.mjs"

class InsertTextStep extends Step {
  constructor(pos, text) {
    super()
    this.pos = pos
    this.text = text
  }

  apply(doc) {
    const slice = new Slice(Fragment.from(doc.type.schema.text(this.text)), 0, 0)
    return StepResult.fromReplace(doc, this.pos, this.pos, slice)
  }

  getMap() { return new StepMap([this.pos, 0, this.text.length]) }

  invert() { return new ReplaceStep(this.pos, this.pos + this.text.length, Slice.empty) }

  map(mapping) {
    const result = mapping.mapResult(this.pos, 1)
    return result.deletedAcross ? null : new InsertTextStep(result.pos, this.text)
  }

  merge(other) {
    return other instanceof InsertTextStep && other.pos == this.pos + this.text.length
      ? new InsertTextStep(this.pos, this.text + other.text)
      : null
  }

  toJSON() { return { stepType: "insertText", pos: this.pos, text: this.text } }

  static fromJSON(schema, json) {
    if (typeof json.pos != "number" || typeof json.text != "string")
      throw new RangeError("Invalid input for InsertTextStep.fromJSON")
    return new InsertTextStep(json.pos, json.text)
  }
}

const jsonID = ["insertText", "insertText", "replace"].map(id => ({
  id,
  ...outcome(() => (Step.jsonID(id, InsertTextStep), { result: null })),
}))

const spec = {
  nodes: { doc: { content: "paragraph+" }, paragraph: { content: "text*" }, text: {} },
  marks: { em: {} },
}
const schema = new Schema(spec)
const start = {
  type: "doc",
  content: [
    { type: "paragraph", content: [{ type: "text", text: "hello" }] },
    { type: "paragraph", content: [{ type: "text", text: "w😀rld" }] },
  ],
}
const doc = Node.fromJSON(schema, start)
const insert = (pos, text) => ({ stepType: "insertText", pos, text })

const transforms = [
  [insert(1, "ab")],
  [insert(6, "!"), insert(7, "?"), insert(10, "😀")],
  [insert(3, "x"), { stepType: "replace", from: 1, to: 2 }, insert(1, "y")],
  [insert(2, "z"), { stepType: "addMark", from: 1, to: 5, mark: { type: "em" } }],
  [insert(0, "no")],
  [insert(9, "k"), insert(99, "far")],
].map(steps => {
  const tr = new Transform(doc)
  const applied = []
  for (const json of steps) {
    const step = Step.fromJSON(schema, json)
    const before = tr.doc
    const { result: failed, error } = outcome(() => ({ result: tr.maybeStep(step).failed }))
    if (error) {
      applied.push({ error })
      break
    }
    applied.push(
      failed ? { failed } : { map: step.getMap().ranges, inverted: step.invert(before).toJSON() },
    )
  }
  const positions = Array.from({ length: doc.content.size + 1 }, (_, pos) => tr.mapping.map(pos))
  return { steps, applied, result: tr.doc.toJSON(), positions }
})

const mapped = [
  [insert(8, "x"), [{ stepType: "replace", from: 1, to: 4 }]],
  [insert(3, "x"), [{ stepType: "replace", from: 1, to: 4 }]],
  [insert(4, "x"), [{ stepType: "replace", from: 1, to: 4 }]],
  [insert(9, "x"), [insert(2, "abc"), insert(1, "😀")]],
  [insert(2, "x"), [insert(2, "abc")]],
].map(([step, over]) => {
  const mapping = new Mapping(over.map(json => Step.fromJSON(schema, json).getMap()))
  return { step, over, result: Step.fromJSON(schema, step).map(mapping)?.toJSON() ?? null }
})

const merged = [
  [insert(2, "ab"), insert(4, "cd")],
  [insert(2, "ab"), insert(3, "cd")],
  [insert(2, "😀"), insert(4, "!")],
  [insert(2, "ab"), { stepType: "replace", from: 4, to: 5 }],
  [{ stepType: "replace", from: 4, to: 5 }, insert(4, "ab")],
].map(([a, b]) => ({
  a,
  b,
  result: Step.fromJSON(schema, a).merge(Step.fromJSON(schema, b))?.toJSON() ?? null,
}))

const fromJSON = [
  insert(1, "a"),
  { stepType: "insertText", pos: "1", text: "a" },
  { stepType: "insertText", pos: 1 },
  { stepType: "unknown" },
].map(json => ({ json, ...outcome(() => ({ result: Step.fromJSON(schema, json).toJSON() })) }))

writeFixture("steps", { schema: spec, start, jsonID, transforms, mapped, merged, fromJSON })
