// Replacing inside a textblock, where the text on either side of the range joins the text put in
// when it has the same marks, run like ProseMirror's suites against both targets.
import ist from "ist"
import {Fragment, Node, Slice} from "prosemirror-model"
import {schema} from "prosemirror-schema-basic"

type Json = {[key: string]: unknown}

const doc = (...content: Json[]) => Node.fromJSON(schema, {type: "doc", content})
const p = (...content: Json[]) => ({type: "paragraph", content})
const t = (text: string, ...marks: string[]) =>
  ({type: "text", text, ...(marks.length ? {marks: marks.map(type => ({type}))} : {})})
const slice = (...nodes: Json[]) => new Slice(Fragment.fromJSON(schema, nodes), 0, 0)
const open = (...nodes: Json[]) => new Slice(Fragment.fromJSON(schema, nodes), 1, 1)
const json = (value: Node | Slice) => JSON.stringify(value.toJSON())
const paragraphs = (...content: string[]) =>
  `{"type":"doc","content":[{"type":"paragraph","content":[${content.join(",")}]}]}`

describe("Node.replace inside a textblock", () => {
  it("joins the text on both sides", () => {
    ist(json(doc(p(t("abcdef"))).replace(4, 4, slice(t("x")))), paragraphs('{"type":"text","text":"abcxdef"}'))
  })

  it("joins only the side with the same marks", () => {
    let before = doc(p(t("ab"), t("cd", "em")))
    ist(json(before.replace(3, 3, slice(t("x")))),
        paragraphs('{"type":"text","text":"abx"}', '{"type":"text","marks":[{"type":"em"}],"text":"cd"}'))
    ist(json(before.replace(3, 3, slice(t("x", "em")))),
        paragraphs('{"type":"text","text":"ab"}', '{"type":"text","marks":[{"type":"em"}],"text":"xcd"}'))
  })

  it("joins the parts of text nodes it cuts across", () => {
    let before = doc(p(t("ab"), t("cd", "em"), t("ef")))
    ist(json(before.replace(2, 6, slice(t("x")))), paragraphs('{"type":"text","text":"axf"}'))
    ist(json(before.replace(2, 6, slice(t("x", "em")))),
        paragraphs('{"type":"text","text":"a"}', '{"type":"text","marks":[{"type":"em"}],"text":"x"}',
                   '{"type":"text","text":"f"}'))
  })

  it("joins the first and last nodes put in, and not those between", () => {
    ist(json(doc(p(t("ab"), t("cd", "em"), t("ef"))).replace(3, 5, slice(t("x"), t("y", "strong"), t("z")))),
        paragraphs('{"type":"text","text":"abx"}', '{"type":"text","marks":[{"type":"strong"}],"text":"y"}',
                   '{"type":"text","text":"zef"}'))
  })

  it("keeps nodes with other marks between the ends apart", () => {
    ist(json(doc(p(t("ab"), t("cd", "em"), t("ef"))).replace(3, 5, slice(t("x", "strong"), t("y"), t("z", "em")))),
        paragraphs('{"type":"text","text":"ab"}', '{"type":"text","marks":[{"type":"strong"}],"text":"x"}',
                   '{"type":"text","text":"y"}', '{"type":"text","marks":[{"type":"em"}],"text":"z"}',
                   '{"type":"text","text":"ef"}'))
  })

  it("puts text into an empty textblock, and over all of one", () => {
    ist(json(doc(p()).replace(1, 1, slice(t("x")))), paragraphs('{"type":"text","text":"x"}'))
    ist(json(doc(p(t("ab"))).replace(1, 3, slice(t("x")))), paragraphs('{"type":"text","text":"x"}'))
  })

  // Their text, as tarnish's toJSON can't hold a lone surrogate.
  it("joins text to the halves of surrogate pairs it cuts", () => {
    let one = doc(p(t("a😀b"))).replace(3, 4, slice(t("x")))
    ist(one.firstChild!.childCount, 1)
    ist(one.textContent, "a\ud83dxb")
    let two = doc(p(t("a😀b😀c"))).replace(3, 6, slice(t("x")))
    ist(two.firstChild!.childCount, 1)
    ist(two.textContent, "a\ud83dx\ude00c")
  })

  it("joins text in a textblock deeper down", () => {
    ist(json(doc(p(t("ab")), {type: "blockquote", content: [p(t("cd"))]}).replace(6, 6, slice(t("x")))),
        '{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"ab"}]},' +
        '{"type":"blockquote","content":[{"type":"paragraph","content":[{"type":"text","text":"xcd"}]}]}]}')
  })
})

describe("Slice", () => {
  it("joins the text either side of what removeBetween removes", () => {
    ist(json(open(p(t("abc"))).removeBetween(1, 2)),
        '{"content":[{"type":"paragraph","content":[{"type":"text","text":"ac"}]}],"openStart":1,"openEnd":1}')
  })

  it("joins the halves of a surrogate pair removeBetween brings together", () => {
    let split = doc(p(t("a😀b"))).replace(3, 3, slice(t("x")))
    ist(json(split.slice(1, split.content.size - 1).removeBetween(2, 3)),
        '{"content":[{"type":"text","text":"a😀b"}]}')
  })

  it("joins the text insertAt puts in", () => {
    ist(json(open(p(t("abc"))).insertAt(1, Fragment.fromJSON(schema, [t("x")]))),
        '{"content":[{"type":"paragraph","content":[{"type":"text","text":"axbc"}]}],"openStart":1,"openEnd":1}')
  })
})
