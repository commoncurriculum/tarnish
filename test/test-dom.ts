// DOM specs ProseMirror's own tests don't render, run like its suites against both targets.
import ist from "ist"
import {DOMParser, DOMSerializer, DOMOutputSpec, Schema} from "prosemirror-model"
import {schema as basic} from "prosemirror-schema-basic"

// @ts-ignore
import {JSDOM} from "jsdom"
const document = new JSDOM().window.document

function html(spec: DOMOutputSpec) {
  return (DOMSerializer.renderSpec(document, spec).dom as HTMLElement).outerHTML
}

describe("DOMParser.parse", () => {
  let dom = document.createElement("div")
  dom.innerHTML = "<p>a</p><p>b</p><p>c</p>"
  let parse = (from: number, to?: number) => DOMParser.fromSchema(basic).parse(dom, {from, to})

  it("reads past the last child when the end comes before the start", () => {
    ist.throws(() => parse(2, 1), (e: Error) =>
      e instanceof TypeError && e.message == "Cannot read properties of null (reading 'nodeType')")
  })

  it("reads a start past the last child when there is an end", () => {
    ist.throws(() => parse(5, 1), (e: Error) =>
      e instanceof TypeError && e.message == "Cannot read properties of undefined (reading 'nodeType')")
  })

  it("parses nothing from a start past the last child without an end", () => {
    ist(parse(5).childCount, 1)
    ist(parse(5).firstChild!.childCount, 0)
  })

  it("stops at the last child for an end past it", () => {
    ist(parse(1, 7).textContent, "bc")
  })
})

describe("DOMSerializer.renderSpec", () => {
  it("takes a rendered object after the name for attributes", () => {
    let span = document.createElement("span"), em = document.createElement("em")
    ist(html(["div", {dom: span, contentDOM: em}, "text"] as any),
        '<div dom="[object HTMLSpanElement]" contentdom="[object HTMLElement]">text</div>')
  })

  it("takes a rendered link's href, resolved, for an attribute", () => {
    let a = document.createElement("a")
    a.setAttribute("href", "HTTP://Example.COM")
    ist(html(["div", {dom: a}] as any), '<div dom="http://example.com/"></div>')
  })

  it("takes an object that isn't a node as String does, whatever its ownerDocument", () => {
    let title = {ownerDocument: {}, toString: () => "t"}
    ist(html(["p", {title}] as any), '<p title="t"></p>')
  })

  it("takes a rendered object after a child as a child", () => {
    let span = document.createElement("span")
    ist(html(["div", ["p", {class: "a"}], {dom: span}] as any), '<div><p class="a"></p><span></span></div>')
  })
})

describe("DOMSerializer.serializeNode", () => {
  let schema = new Schema({nodes: {
    doc: {content: "block+"},
    block: {group: "block", attrs: {spec: {default: null}}},
    text: {}
  }})
  let node = schema.nodes.block.create({spec: ["script", "alert(1)"]})

  it("refuses an attribute's own array as a spec", () => {
    let serializer = new DOMSerializer({block: node => node.attrs.spec}, {})
    ist.throws(() => serializer.serializeNode(node, {document}), /cross site scripting/)
  })

  it("renders a copy of an attribute's array", () => {
    let serializer = new DOMSerializer({block: node => [...node.attrs.spec] as any}, {})
    ist((serializer.serializeNode(node, {document}) as HTMLElement).outerHTML, "<script>alert(1)</script>")
  })
})
