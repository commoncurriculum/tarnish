// Transforms ProseMirror's own tests don't make, run like its suites against both targets.
import ist from "ist"
import {Fragment, Node, Schema, Slice} from "prosemirror-model"
import {AttrStep, Transform, canSplit, dropPoint, replaceStep} from "prosemirror-transform"

// A title can't be made up, having an attribute with no default, so nothing fills the start of
// a figure before its paragraph.
const schema = new Schema({nodes: {
  doc: {content: "block+"},
  paragraph: {content: "inline*", group: "block"},
  figure: {content: "(title paragraph) | caption", group: "block"},
  title: {content: "inline*", attrs: {level: {}}},
  caption: {content: "inline*"},
  text: {group: "inline"}
}})

// <p>a</p><figure><title>t</title><p>cd</p></figure>, where 3 is between the blocks and 9 is
// between "c" and "d".
const doc = Node.fromJSON(schema, {type: "doc", content: [
  {type: "paragraph", content: [{type: "text", text: "a"}]},
  {type: "figure", content: [
    {type: "title", attrs: {level: 1}, content: [{type: "text", text: "t"}]},
    {type: "paragraph", content: [{type: "text", text: "cd"}]}
  ]}
]})
const end = doc.content.size
// Open three levels deep around a paragraph, which has two.
const deep = new Slice(Fragment.from(schema.nodes.paragraph.create(null, schema.text("ab"))), 3, 0)

function throws(f: () => unknown, name: string, message: string) {
  ist.throws(f, (e: Error) => e.name == name && e.message == message)
}

const nullRead = (property: string) => `Cannot read properties of null (reading '${property}')`

describe("replaceStep", () => {
  it("opens a node nothing fills empty", () => {
    ist(JSON.stringify(replaceStep(doc, 3, 9, Slice.empty)!.toJSON()),
        '{"stepType":"replace","from":3,"to":9,"slice":{"content":[{"type":"figure","content":[{"type":"paragraph"}]}],"openEnd":2}}')
  })

  it("reads a node a slice open deeper than its content doesn't have", () => {
    throws(() => replaceStep(doc, 1, 1, deep), "TypeError", nullRead("type"))
  })
})

describe("reading what isn't there", () => {
  it("canSplit at depth 0", () => {
    throws(() => canSplit(doc, 2, 0), "TypeError", "Cannot read properties of undefined (reading 'type')")
    ist(canSplit(doc, 2, 0, [{type: schema.nodes.paragraph}]), false)
  })

  it("clearIncompatible at the end", () => {
    throws(() => new Transform(doc).clearIncompatible(end, schema.nodes.paragraph), "TypeError", nullRead("childCount"))
  })

  it("AttrStep.invert where there's no node", () => {
    throws(() => new AttrStep(end, "level", 2).invert(doc), "TypeError", nullRead("attrs"))
  })

  it("dropPoint with a slice open deeper than its content", () => {
    throws(() => dropPoint(doc, 1, deep), "TypeError", nullRead("content"))
  })

  it("replaceRange with a slice open deeper than its content", () => {
    throws(() => new Transform(doc).replaceRange(1, 1, deep), "TypeError", nullRead("content"))
  })
})

describe("Transform", () => {
  it("joins before the start out of range", () => {
    throws(() => new Transform(doc).join(0, 1), "RangeError", "Position -1 out of range")
  })

  it("fails to split past the top", () => {
    throws(() => new Transform(doc).split(2, 2), "TransformError", "Inserted content deeper than insertion position")
  })
})
