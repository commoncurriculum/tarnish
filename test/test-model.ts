// Model calls ProseMirror's own tests don't make, run like its suites against both targets.
import ist from "ist"
import {Fragment} from "prosemirror-model"
import {schema} from "prosemirror-schema-basic"

const para = schema.nodes.paragraph
const two = Fragment.from([para.create(), para.create()])
const doc = schema.nodes.doc.create(null, two)

function throwsRange(f: () => unknown, message: string) {
  ist.throws(f, (e: Error) => e instanceof RangeError && e.message == message)
}

describe("ContentMatch.matchFragment", () => {
  it("reads past the last child where the children fit up to it", () => {
    throwsRange(() => schema.nodes.doc.contentMatch.matchFragment(two, 0, 3),
                "Index 2 out of range for <paragraph, paragraph>")
  })

  it("stops without reading past the last child where they don't fit", () => {
    ist(para.contentMatch.matchFragment(two, 0, 3), null)
  })

  it("matches nothing from a start past the end", () => {
    ist(schema.nodes.doc.contentMatch.matchFragment(two, 5, 3), schema.nodes.doc.contentMatch)
  })
})

describe("Node", () => {
  it("contentMatchAt past the last child is out of range", () => {
    throwsRange(() => doc.contentMatchAt(3), "Index 2 out of range for <paragraph, paragraph>")
  })

  it("canReplace with a replacement's end past its last child is out of range", () => {
    throwsRange(() => doc.canReplace(0, 0, two, 0, 3), "Index 2 out of range for <paragraph, paragraph>")
  })

  it("canReplace from past the last child at the end", () => {
    ist(doc.canReplace(2, 5), true)
  })
})
