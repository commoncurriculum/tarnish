// Node.check on documents ProseMirror's own tests don't build, run like its suites against both
// targets.
import ist from "ist"
import {Schema} from "prosemirror-model"

function schema() {
  return new Schema({
    nodes: {doc: {content: "para+"}, para: {content: "text*", marks: "em"}, text: {}},
    marks: {em: {}, strong: {}},
  })
}

describe("Node.check", () => {
  let own = schema(), other = schema()

  it("refuses a mark of another schema where a type allows only some marks", () => {
    let text = own.text("x", [other.marks.em.create()])
    let doc = own.nodes.doc.create(null, own.nodes.para.create(null, text))
    ist.throws(() => doc.check(), (e: Error) =>
      e instanceof RangeError && e.message == 'Invalid content for node para: <em("x")>')
  })

  it("takes a mark of the type's own schema", () => {
    let text = own.text("x", [own.marks.em.create()])
    own.nodes.doc.create(null, own.nodes.para.create(null, text)).check()
  })
})
