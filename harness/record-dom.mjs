// Records what ProseMirror's DOMParser and DOMSerializer do in linkedom, to fixtures/dom.json, for
// tarnish-html's tests to expect. The schema is prosemirror-test-builder's: prosemirror-schema-
// basic's with prosemirror-schema-list's lists.
// - Each HTML input is parsed twice: as a <template>'s content holds it, and as a whole document,
//   whose body is parsed. Each records the tree linkedom built, as innerHTML writes it, and the
//   document DOMParser.parse makes of it; the template also DOMParser.parseSlice's slice.
// - Each document, from fixtures/transform.json, the parses and a few of its own, records the
//   innerHTML of an element that DOMSerializer.serializeFragment fills.
// - Each inline style records what an element's style holds with the style attribute set to it,
//   and the attribute that setting style.cssText to it writes.
// - Each DOM output spec records the outerHTML of what DOMSerializer.renderSpec makes of it.
import { readFileSync, writeFileSync } from "node:fs"
import { parseHTML } from "linkedom"
import { DOMParser, DOMSerializer, Node, Schema } from "prosemirror-model"
import { schema as basic } from "prosemirror-schema-basic"
import { addListNodes } from "prosemirror-schema-list"

const schema = new Schema({
  nodes: addListNodes(basic.spec.nodes, "paragraph block*", "block"),
  marks: basic.spec.marks,
})
const window = parseHTML("")
const { document } = window
const parser = DOMParser.fromSchema(schema)
const serializer = DOMSerializer.fromSchema(schema)

const nbsp = " "
const html = [
  // prosemirror-model's test/test-dom.ts.
  "<p>hello</p>",
  "<p>hi<br/>there</p>",
  '<p>hi<img src="img.png" alt="x"/>there</p>',
  "<p>one<strong>two</strong><em><strong>three</strong>four</em>five</p>",
  '<p>a <a href="foo">big </a><a href="bar">nested</a><a href="foo"> link</a></p>',
  "<ul><li><p>one</p></li><li><p>two</p></li><li><p>three<strong>!</strong></p></li></ul><p>after</p>",
  "<ol><li><p>one</p></li><li><p>two</p></li><li><p>three<strong>!</strong></p></li></ol><p>after</p>",
  "<blockquote><p>hello</p><p>bye</p></blockquote>",
  "<blockquote><blockquote><blockquote><p>he said</p></blockquote></blockquote><p>i said</p></blockquote>",
  "<h1>one</h1><h2>two</h2><p>text</p>",
  "<p>text and <code>code that is </code><em><code>emphasized</code></em><code>...</code></p>",
  "<blockquote><pre><code>some code</code></pre></blockquote><p>and</p>",
  "<p><em>hi<br>x</em></p>",
  `<p>${nbsp} ${nbsp}hello${nbsp}</p>`,
  '<p>one</p><div class="comment"><p>two</p><p><strong>three</strong></p></div><p>four</p>',
  '<p><span class="comment" data-id="1"><span class="comment" data-id="2">double comment</span></span></p>',
  '<p><test>a</test><test><img src="x"></test><test>b</test></p>',
  '<svg xmlns="http://www.w3.org/2000/svg"><use xmlns:ns1="http://www.w3.org/1999/xlink" ns1:href="#svg-id"/></svg>',
  "<ol><p>Oh no</p></ol>",
  "<li>hey</li>",
  "<div>hi</div><div>bye</div>",
  "<p><i>hello <b>there</b></i></p>",
  "hi",
  "<div><p>one</p><p>two</p></div>",
  " <blockquote> <p>woo  \n  <em> hooo</em></p> </blockquote> ",
  "<p>hello<br>\n  world</p>",
  "<pre>foo<br>bar</pre>",
  "<ul><li>hi</li><p>whoah</p><li>again</li></ul>",
  "<div>hello<hr/>bye</div>",
  "<p><em>one</em> <strong>two</strong></p>",
  "<p> <b>&#09;</b></p>",
  "<p><b>1 </b>  </p>",
  "<pre></pre>",
  "<pre>foo\n</pre>",
  "<p>foo  bar\nbaz</p>",
  "<p>hello<script>alert('x')</script>!</p>",
  "<head><title>T</title><meta charset='utf8'/></head><body>hi</body>",
  "<p>A <strong>big <strong>strong</strong> monster</strong>.</p>",
  "<p><span style='font-style: italic'>Hello</span>!</p>",
  "<p style='font-weight: bold'>Hello</p>",
  "<blockquote style='font-style: italic'><p style='font-style: normal'>One</p><p>Two</p></blockquote",
  "<ul><li style='font-style:italic'><p><span>Foo</span><span></span><span style='font-style:normal'>Bar</span></p></li></ul>",
  "<p><u>a</u>bc</p>",
  "<em>hi</em> you",
  "<p><strong><span>xx</span>bar</strong></p>",
  "<span> </span>x",
  "<div><br><div>CCC</div><div>DDD</div><br></div>",
  "<li>wow</li><li>such</li>",
  "<ul><li>x</li></ul>",
  "<hr><p>foo</p><p>bar</p><img>",
  "foo   bar",
  "foo",
  "foo<p>bar</p>",
  "<ul><li>foo</li><li>bar<br></li></ul>",
  "<li><ul><li>a</li></ul></li>",
  "<li>foo</li><li></li>",
  "<div><div>foo</div><div>bar</div></div>",
  "<div>foo</div><div><em>bar</em></div>",
  "<ul style='font-weight: bold'><li>foo</li></ul>",
  "<ul style='font-weight: bold'><li>foo</li><li>bar</li></ul>",
  "<p style='font-weight: bold'>foo<strong style='font-weight: bold;'>bar</strong>baz</p>",
  "<div> </div>",
  "<b> </b>",
  "<p style='text-decoration: line-through;'>o<s style='text-decoration: line-through;'>o</s>o</p>",
  "<p><span style='text-decoration: line-through;'><s style='text-decoration: line-through;'>o</s>o</span>o</p>",
  '<p><span style="color: red">abc<span style="color: blue">def</span>ghi</span></p>',
  "<p><var></var>hello</p>",
  "<p>hello<var></var></p>",
  "<p>hel<var></var>lo</p>",
  "<p>hi</p><object><var></var>foo</object><p>ok</p>",
  "<ul><li>foo</li><var></var><li>bar</li></ul>",
  "<var></var><p>hi</p>",
  "<p>hi</p><var></var>",
  "<foo></foo><blockquote><foo></foo><p><foo></foo></p></blockquote>",
  "<foo></foo><blockquote><foo></foo><ol><li><p>a</p><foo></foo></li></ol></blockquote>",
  "<foo></foo><blockquote><foo></foo><ol><foo></foo><li><p>a</p><foo></foo></li></ol></blockquote>",
  "<foo></foo><blockquote><p></p><foo></foo></blockquote><ol><li><p>a</p><foo></foo></li></ol>",
  "<p>one<br>two</p>",
  "<ol><p>one</p></ol>",
  "<p><span style='font-weight: 800'>one</span></p>",
  "<div><strong><strong>A</strong></strong>B</div><span>C</span>",
  "<p>abc <span style='font-weight: strong'>def</span></p>",
  "<pre>  hello </pre>   ",
  "  <div style='white-space: pre'>  okay  then </div>  <p> x</p>",
  "<p><span style='white-space: pre'>one\ntwo\n\nthree</span></p>",
  "<p><span>one\ntwo\n\nthree</span></p>",
  "foobar<strong>baz</strong>",
  '<strong>foo<code>bar</code></strong><em><i data-emphasis="true"><strong><code>baz</code></strong>quux</i></em>xyz',
  '<div class="outer"><p class="inner">hello</p></div>',

  // Entities.
  "<p>&amp; &lt; &gt; &quot; &#39; &apos; &nbsp; &copy; &#x1F600; &#128512;</p>",
  "<p>&notin; &notit; &amp &lt3 &copy2024</p>",
  "<p>&AMP; &Aacute &aacute; &AElig;</p>",
  "<p>AT&amp;T &unknown; &#0; &#xD800; &#x110000; &#128; &#x9F;</p>",
  "<p>&NotEqualTilde; &#xFDD0; &#x1FFFF; &#x0D;</p>",
  "<p>a&nbsp;&nbsp;b&#160;c</p>",
  '<p><a href="?a=1&b=2&amp;c=3&copy=4">query</a></p>',
  '<p title="&lt;tag&gt; &quot;q&quot;">attribute entities</p>',

  // Whitespace.
  "<p>  leading and trailing  </p>",
  "<p>a\tb\nc\r\nd\re</p>",
  "<p>line<br>  next</p>",
  "<pre>\nfirst line</pre>",
  "<pre>\n\nsecond line</pre>",
  "<pre>  indented\n    more\n</pre>",
  '<pre><code>fn main() {\n    println!("hi");\n}\n</code></pre>',
  "<textarea>\nkept</textarea>",
  "<p>a <b> b </b> c</p>",
  "<p> <em> </em> </p>",
  "<div>  <p>x</p>  </div>",
  "\n\n<p>x</p>\n\n",
  "<p>a b c　d</p>",
  "<p>a\u000cb</p>",
  "<p><span style='white-space: pre-wrap'>  a  b  </span></p>",
  "<div style='white-space:pre-line'>a\n  b</div>",
  "<p>  a  \n  b  </p>",
  "<p><b>a</b> <i>b</i></p>",
  "<p>a<span> </span>b</p>",
  "<blockquote>\n  <p>quoted</p>\n</blockquote>",

  // Lists.
  "<ul><li>a<ul><li>b<ul><li>c</li></ul></li></ul></li></ul>",
  '<ol start="3"><li>three</li><li>four</li></ol>',
  '<ol start="0"><li>zero</li></ol>',
  '<ol start="-2"><li>minus two</li></ol>',
  '<ol start=" 7 "><li>spaced</li></ol>',
  '<ol start=""><li>empty start</li></ol>',
  '<ol start="1e1"><li>exponent</li></ol>',
  '<ol start="0x10"><li>hex</li></ol>',
  '<ol start="2.5"><li>fraction</li></ol>',
  '<ol start="1"><li>one</li></ol>',
  '<ol start="abc"><li>not a number</li></ol>',
  '<ol start="Infinity"><li>infinite</li></ol>',
  "<ul><li>a</li><ul><li>nested directly</li></ul><li>b</li></ul>",
  "<ol><li>a</li><ol><li>b</li><ul><li>c</li></ul></ol></ol>",
  "<ul><ul><li>first is a list</li></ul></ul>",
  "<ol><li><p>a</p><ol><li><p>b</p></li></ol></li></ol>",
  "<ul><li><p>para</p><p>para2</p></li></ul>",
  "<ul><li></li></ul>",
  "<ul></ul>",
  "<li>orphan</li><li>items</li>",
  "<ul><li>a<ol><li>mixed</li></ol></li></ul>",
  "<ul><li>text <b>bold</b><ul><li>sub</li></ul>after</li></ul>",
  "<ul><li><h2>heading in item</h2></li></ul>",
  "<ul><li><blockquote>quote in item</blockquote></li></ul>",
  "<dl><dt>term</dt><dd>definition</dd></dl>",
  "<ul>\n  <li>one</li>\n  <li>two</li>\n</ul>",

  // Marks.
  "<p><b>b</b><strong>s</strong><i>i</i><em>e</em><code>c</code></p>",
  "<p><b style='font-weight: normal'>not bold</b></p>",
  "<b style='font-weight:normal;' id='docs-internal-guid-1'><p><span style='font-weight:700'>bold</span> <span style='font-weight:400'>plain</span></p></b>",
  "<p><span style='font-weight: 400'>x</span></p>",
  "<p><strong><span style='font-weight: 400'>unbold</span></strong></p>",
  "<p><span style='font-weight: bolder'>a</span><span style='font-weight: 500'>b</span><span style='font-weight: 499'>c</span><span style='font-weight: 1000'>d</span></p>",
  "<p><span style='font-style: oblique'>x</span></p>",
  "<p><em><span style='font-style: normal'>plain</span></em></p>",
  "<p><a href='https://example.com' title='T'>link</a> <a>no href</a> <a href=''>empty href</a></p>",
  "<p><a href='x'><b>bold link</b></a></p>",
  "<p><code><b>x</b></code></p>",
  "<pre><b>bold</b> in pre</pre>",
  "<p><strong>a<em>b</em></strong><em>c</em></p>",
  "<p><em>a</em><em>b</em></p>",
  "<p style='font-style: italic; font-weight: bold'>both</p>",
  "<p><span style='font: italic bold 12px/30px Georgia, serif'>font shorthand</span></p>",
  "<p><span style='font: bold 12px black'>color in font</span></p>",
  "<p><span style='font: inherit'>inherit</span><span style='font: 12px serif'>size</span></p>",
  "<p><span style='FONT-WEIGHT: BOLD'>caps</span><span style='Font-Style: Italic'>mixed</span></p>",
  "<p><span style='font-weight: bold !important'>important</span></p>",
  "<p><span style='font-weight: bold; font-weight: normal'>last wins</span></p>",
  "<p><span style='font-weight: bold; font-weight: ;'>emptied</span></p>",
  `<p><span style="font-family: 'Comic Sans'; font-weight: 700">quoted</span></p>`,
  "<p><span style='font-weight:700;font-style:italic;text-decoration:underline'>Docs</span></p>",
  "<p><span style=';font-weight: bold'>bad start</span><span style='font-weight: bold;; font-style: italic'>double</span></p>",
  "<p><span style='font-weight: bold /* comment */'>comment</span></p>",
  "<p><span style='font-weight: bold}'>brace</span></p>",
  `<p><span style="font-family: 'unclosed; font-weight: bold">unclosed</span></p>`,
  "<p><span style='--weight: bold; font-weight: var(--weight)'>custom</span></p>",
  "<p><span style='mso-bidi-font-weight: bold; font-weight: 600'>word</span></p>",
  "<p><span style='font-weight:bold;white-space:pre'>  pre bold  </span></p>",
  "<p><img src='a.png' alt='A' title='T'><img alt='no src'><img src=''></p>",
  "<img src='top.png'>",
  "<p><br></p>",
  "<br>",
  "<p>a<br><br>b</p>",

  // Malformed HTML.
  "<p>unclosed",
  "<p>one<p>two",
  "<b><i>overlap</b></i>",
  "<p><div>block in p</div></p>",
  "</p>stray end",
  "<em>a<p>b</em>c</p>",
  "<table><tr><td>cell</td></tr></table>",
  "<p>a<table><tr><td>b</td></tr></table>c</p>",
  "<table>foster<tr><td>x</td></tr></table>",
  "<table><b>bold</b><tr><td>x</td></tr></table>",
  "<table><tr><td>a</td></tr><i>i</i></table>",
  "<ul><li><table><tr><td>cell in item</td></tr></table></li></ul>",
  "<tr><td>a</td><td>b</td></tr>",
  "<td>x</td>",
  "<li>a<li>b",
  "<h1>a<h2>b</h2></h1>",
  "<a href='1'>a<a href='2'>b</a></a>",
  "<ul><li>a</ul>after",
  "<p>x</P>",
  "<P CLASS=x>upper</P>",
  "<p>a</br>b</p>",
  "<b>1<p>2</b>3</p>",
  "<div<p>x",
  "<p>x<!-- unclosed comment",
  "<p>1 < 2 && 3 > 2</p>",
  "<p>a<b</p>",
  "<<p>x</p>",
  "<select><option>a<b>x</b></option></select>",
  "<button><p>in button</p></button>",
  "<form><p>in form</p></form>",
  "<frameset><frame></frameset>",
  "<body><p>x</p></body><p>after body</p>",
  "<html><body><p>x</p></body></html>",
  "<!DOCTYPE html><html><head><title>t</title></head><body><p>doc</p></body></html>",
  "<p>x</p></body></html><p>y</p>",
  "<body onload=x><p>y</p>",
  "<p>x</p><html lang=en><p>y</p>",
  "<image src=x.png>",
  "<isindex>",
  "<p><span>a<isindex>b</span>c</p>",
  "<listing>\nlisting</listing>",

  // Attributes with quotes.
  `<p title='single "double" inside'>x</p>`,
  `<p title="double 'single' inside">x</p>`,
  "<p title=unquoted>x</p>",
  `<img src="a b.png" alt='it&#39;s "it"'>`,
  '<p><a href="x" href="y">duplicate</a></p>',
  '<p title="line\nbreak">x</p>',
  '<p><img src="x.png" alt="nbsp&nbsp;here" title="<b>"></p>',
  '<p><a href="javascript:alert(1)" title=\'a&b\'>js</a></p>',
  '<p data-x="1" DATA-Y="2" aria-label="a">x</p>',
  "<p title>empty attribute</p>",
  "<img src=x.png alt=a>b",
  `<p><a href=https://example.com/?q="x">unquoted with quote</a></p>`,

  // Comments.
  "<p>a<!-- comment -->b</p>",
  "<!-- top --><p>x</p>",
  "<p><!---->x</p>",
  "<p>a<!-- <b>not bold</b> -->b</p>",
  "<!--x-->",
  "<p>a<!-- -- -->b</p>",
  "<p><?php echo 1; ?>x</p>",
  "<![CDATA[x]]><p>y</p>",

  // Unknown and special elements.
  "<custom-el>x</custom-el>",
  "<p><custom-el>inline</custom-el></p>",
  "<foo><p>x</p></foo>",
  "<section><article><p>x</p></article></section>",
  "<p><span>a</span><font color=red>b</font><u>c</u><s>d</s><sub>e</sub><sup>f</sup></p>",
  "<figure><img src='x'><figcaption>caption</figcaption></figure>",
  "<details><summary>s</summary><p>d</p></details>",
  "<main><nav><ul><li>n</li></ul></nav></main>",
  "<x-y><z>deep</z></x-y>",
  "<math><mi>x</mi></math>",
  "<svg><circle r='1'/><text>svg text</text></svg>",
  "<svg><foreignObject><p>html in svg</p></foreignObject></svg>",
  "<svg><title><b>integration point</b></title></svg>",
  "<svg><![CDATA[x<y]]></svg>",
  '<math><mi>x</mi><annotation-xml encoding="text/html"><p>y</p></annotation-xml></math>',
  "<math><mtext><b>bold</b></mtext></math>",
  "<math><mi><![CDATA[x<y]]></mi></math>",
  "<svg viewBox='0 0 1 1' xlink:href='#a' xml:lang='en'><path d='M0'/></svg>",
  "<p><svg><p>breaks out</p></svg></p>",
  "<template><p>inner</p></template><p>outer</p>",
  "<noscript><p>no script</p></noscript>",
  "<style>p{color:red}</style><p>x</p>",
  "<script>var a = '<p>';</script><p>x</p>",
  "<textarea><p>raw</p></textarea>",
  "<title>t</title><p>x</p>",
  "<iframe><p>raw</p></iframe>",
  "<xmp><b>raw</b></xmp>",
  "<plaintext><b>raw",
  "<object><p>o</p></object>",
  "<video><source src=x></video>",
  "<input value=x><button>b</button>",
  "<p>a<wbr>b</p>",
  "<ruby>漢<rt>kan</rt></ruby>",
  "<address>address</address>",
  "<hgroup><h1>t</h1></hgroup>",
  "<h3 id=x>three</h3><h6>six</h6><h7>seven</h7>",
  "<p>para</p><hr><hr/><p>x</p>",
  "<center>centered</center>",

  // Text.
  "<p>héllo wörld 😀 中文 עברית</p>",
  "﻿<p>byte order mark</p>",
  "<p>a\u0000b</p>",
  "<p>  </p>",
  "<p>é</p>",
  "<p>a</p>b<p>c</p>",
  "plain text",
  "",
]

const withOptions = [
  ["<p>foo  bar\nbaz</p>", { preserveWhitespace: true }],
  ["<span> </span>x", { preserveWhitespace: "full" }],
  ["foo   bar", { preserveWhitespace: true }],
  ["<div> </div>", { preserveWhitespace: true }],
  ["<b> </b>", { preserveWhitespace: true }],
  ["<p>  a  \n  b  </p>", { preserveWhitespace: true }],
  ["<p>  a  \n  b  </p>", { preserveWhitespace: "full" }],
  [" <blockquote> <p>woo  \n  <em> hooo</em></p> </blockquote> ", { preserveWhitespace: true }],
  [" <blockquote> <p>woo  \n  <em> hooo</em></p> </blockquote> ", { preserveWhitespace: "full" }],
  ["<p>a<br>\n  b</p>", { preserveWhitespace: "full" }],
  ["<ul>\n  <li>one</li>\n  <li>two</li>\n</ul>", { preserveWhitespace: true }],
]

function outcome(run) {
  try {
    return run()
  } catch (error) {
    const name = error instanceof window.DOMException ? { name: error.name } : {}
    return { error: { class: error.constructor.name, ...name, message: error.message } }
  }
}

// Each run parses the HTML again, since parsing can move nested lists in the DOM.
function fromTemplate(source) {
  const template = document.createElement("template")
  template.innerHTML = source
  return template
}

function fromDocument(source) {
  return new window.DOMParser().parseFromString(source, "text/html").body
}

const parses = [...html.map(source => [source, undefined]), ...withOptions].map(([source, options]) => ({
  html: source,
  ...(options ? { options } : {}),
  template: {
    tree: fromTemplate(source).innerHTML,
    doc: outcome(() => parser.parse(fromTemplate(source).content, options).toJSON()),
    slice: outcome(() => parser.parseSlice(fromTemplate(source).content, options).toJSON()),
  },
  document: {
    tree: fromDocument(source).innerHTML,
    doc: outcome(() => parser.parse(fromDocument(source), options).toJSON()),
  },
}))

const own = [
  { type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text: `a < b & c > d ${nbsp} "q" 'a' \` = ` }] }] },
  {
    type: "doc",
    content: [
      {
        type: "paragraph",
        content: [
          { type: "image", attrs: { src: "a.png?x=1&y=2", alt: `a "quoted" <alt> & ${nbsp}`, title: null } },
          { type: "text", marks: [{ type: "link", attrs: { href: "https://x.com/?a=1&b=2", title: '"T"' } }], text: "link" },
          { type: "hard_break" },
          { type: "text", marks: [{ type: "em" }, { type: "strong" }, { type: "code" }], text: "<all>" },
        ],
      },
    ],
  },
  {
    type: "doc",
    content: [
      { type: "ordered_list", attrs: { order: 3 }, content: [{ type: "list_item", content: [{ type: "paragraph" }] }] },
      { type: "ordered_list", attrs: { order: 0 }, content: [{ type: "list_item", content: [{ type: "paragraph" }] }] },
      { type: "ordered_list", attrs: { order: 2.5 }, content: [{ type: "list_item", content: [{ type: "paragraph" }] }] },
      ...[1, 2, 3, 4, 5, 6].map(level => ({ type: "heading", attrs: { level }, content: [{ type: "text", text: `h${level}` }] })),
      { type: "code_block", content: [{ type: "text", text: "a\n  <b> & c\n" }] },
      { type: "horizontal_rule" },
      { type: "blockquote", content: [{ type: "paragraph", content: [{ type: "text", text: "😀 é" }] }] },
    ],
  },
]
const transforms = JSON.parse(readFileSync(new URL("../fixtures/transform.json", import.meta.url), "utf8"))
const recorded = transforms.tests
  .filter(test => test.schema === 0 || test.schema === 2)
  .flatMap(test => [test.start, test.result])
const parsed = parses.flatMap(parse => [parse.template.doc, parse.document.doc]).filter(doc => !doc.error)
// Each document is read from its JSON text, as the tests read it: a NaN attribute is null there.
const texts = new Set([...own, ...recorded, ...parsed].map(doc => JSON.stringify(doc)))
const docs = [...texts].map(text => JSON.parse(text))

const serializes = docs.map(doc => ({
  doc,
  ...outcome(() => {
    const container = document.createElement("div")
    container.appendChild(serializer.serializeFragment(Node.fromJSON(schema, doc).content, { document }))
    return { html: container.innerHTML }
  }),
}))

// cssstyle rewrites or refuses more values than tarnish-html's port does, which reads those mark
// rules read: fonts and colors. See crates/tarnish-html/src/style/mod.rs.
const css = [
  "font-weight: bold",
  "font-weight:bold;font-style:italic",
  "FONT-WEIGHT: Bold",
  "font-weight: bold !important",
  "font-weight: bold ! important",
  "font-weight: bold!important;color:red",
  "font-weight: bold; font-weight: normal",
  "font-weight: bold; font-weight: ;",
  "font-weight: ; font-weight: bold",
  "font-weight:",
  "font-weight",
  ":bold",
  ";font-weight: bold",
  "font-weight: bold;; font-style: italic",
  "font-weight: bold /* c */; font-style: italic",
  "/* c */font-weight: bold",
  "font-weight: bold /* unclosed",
  "font-weight: bold}",
  "a}b",
  "font-weight: bold; }",
  "font-family: 'a; b'; font-weight: 600",
  'font-family: "unclosed; font-weight: bold',
  "font-family: 'it\\'s'; font-style: oblique",
  "list-style-image: url(a;b.png); font-weight: 700",
  "min-width: calc(1px + (2px)); font-style: italic",
  "color: rgb(1,2,3",
  "@media x { }",
  "color: red; @media print",
  "--x: 1; --X: 2; font-weight: 700",
  "unknown: 1; mso-list: l0; font-style: italic",
  "-webkit-text-fill-color: red; webkit-text-fill-color: blue !important",
  "white-space: pre-wrap",
  "text-decoration: underline line-through",
  "text-align:center; vertical-align: super",
  "",
  "   ",
  "font: italic bold 12px/30px Georgia, serif",
  "font: bold 12px black",
  "font: inherit",
  "font: 12px serif",
  "font: caption",
  "font: small-caps 900 larger 'Comic Sans'",
  "font: normal",
  'font: 7.0pt "Times New Roman"',
  "font: 12PX serif",
  "font: bold; font-weight: normal",
  "font-weight: 300; font: italic",
  "font: bold !important",
  "font: 1.5 serif",
  "font: 0 serif",
  "font-size: 0",
  "font-size: 12px",
  "font-size: 12PX",
  "font-size: LARGE",
  "font-size: 50%",
  "font-size: calc(1em + 2px)",
  "font-size: big",
  "line-height: 1.5; font-variant: small-caps; font-family: Arial, sans-serif",
  "color: #f00",
  "color: #F00A",
  "color: #ff000080",
  "color: #1234567",
  "color: #12",
  "color: rgb(255, 0, 0)",
  "color: rgb(300,-5,0)",
  "color: rgb(50%, 0%, 100%)",
  "color: rgb(1,2)",
  "color: rgb(1, 2.5, 3)",
  "color: rgba(1, 2, 3, 0.5)",
  "color: rgba(1,2,3,1)",
  "color: rgba(1,2,3,x)",
  "color: rgba(1,2,3,7)",
  "color: Red",
  "color: transparent",
  "color: currentColor",
  "color: WindowText",
  "color: nonsense",
  "color: RGB(1,2,3)",
  "background-color: #0f0",
  "background-color: transparent",
  "background-color: Inherit",
  "background-color: nonsense",
  "background-color: yellow; color: black",
  "text-underline-color: #abc",
  "webkit-tap-highlight-color: #abc",
]
const properties = [
  "font", "font-family", "font-size", "font-style", "font-variant", "font-weight", "line-height", "color",
  "background-color", "text-decoration", "white-space", "text-align", "vertical-align", "list-style-image",
  "min-width", "-webkit-text-fill-color", "webkit-text-fill-color", "-webkit-tap-highlight-color",
  "text-underline-color", "--x", "--X", "FONT-WEIGHT", "unknown",
]
const styles = css.map(text => {
  const element = document.createElement("p")
  element.setAttribute("style", text)
  const values = properties.map(name => [name, element.style.getPropertyValue(name)])
  const written = document.createElement("p")
  written.style.cssText = text
  return {
    css: text,
    length: element.style.length,
    values: Object.fromEntries(values.filter(([, value]) => value !== "")),
    cssText: written.getAttribute("style"),
  }
})

const specs = [
  ["div", { class: "a", "data-X": "1", title: `a "b" & c${nbsp}<d>` }, "text & <more>", ["span", 0]],
  ["DIV", { ID: "x", Title: "t" }],
  ["http://www.w3.org/2000/svg svg", { viewBox: "0 0 1 1", style: "fill: red; COLOR: #0f0" }, ["use", { "http://www.w3.org/1999/xlink href": "#a" }]],
  ["svg", { viewBox: "0 0 1 1" }],
  ["p", { style: "font: bold 12px serif; color: rgb(1, 2, 3)" }, 0],
  ["p", { style: "color: 'unclosed" }],
  ["p", { style: "" }],
  ["p", { style: "nonsense: 1" }],
  ["p", { style: "--x: 1; font-weight: 700 !important; -webkit-text-fill-color: red; webkit-text-fill-color: #00f" }],
  ["urn:x foo", { style: "color: #f00" }],
  ["p", { title: 1, "data-b": true, "data-n": null, "data-o": { a: 1 }, "data-a": [1, null, 2], "data-f": 1.5, "data-e": 1e21 }],
  ["1bad"],
  ["p", { " y": "1" }],
  ["p", { "x:y": "1", "bad name": "2" }],
  ["urn:x p:q", { "urn:x p:r": "1", "urn:y p:r": "2" }, ["child"]],
  ["p", { "http://x.com xmlns": "1" }],
  ["p", { "http://www.w3.org/2000/xmlns/ xmlns:a": "urn:a" }],
  ["p", { "urn:x xml:lang": "en" }],
  ["p", { "xml:lang": "en", "http://www.w3.org/XML/1998/namespace xml:space": "preserve" }],
  ["template", ["p", "in template"]],
  ["br", "text"],
  ["http://www.w3.org/1999/xhtml x:br", "text"],
  ["style", "a < b & c"],
  ["http://www.w3.org/1999/xhtml x:style", "a < b & c"],
  ["script", "if (a < b) {}"],
  ["noscript", "<b>"],
  ["pre", "\nx"],
  ["textarea", "\n<x>"],
  ["p", ["span", 0], ["span", 0]],
  ["p", "x", 0],
  ["p", { a: "1" }, ["b", { c: "2" }, 0]],
  ["a", { href: "x", HREF: "y" }],
  ["http://www.w3.org/1999/xhtml P", { CLASS: "c" }],
  ["http://www.w3.org/1998/Math/MathML math", ["mi", "x"]],
  ["http://www.w3.org/2000/svg svg", ["http://www.w3.org/1999/xhtml div", "html in svg"]],
  ["img", { src: "a.png", alt: null }],
  [],
  [1],
  ["p", { class: "x" }, "a", "b", ["i", "c"]],
  ["p", `${nbsp}&`],
  ["ns:tag"],
  ["x y z"],
]
const renders = specs.map(spec => ({
  spec,
  ...outcome(() => {
    const { dom, contentDOM } = DOMSerializer.renderSpec(document, spec)
    return { html: dom.outerHTML, hole: contentDOM ? contentDOM.outerHTML : null }
  }),
}))

const spec = { topNode: schema.topNodeType.name, nodes: schema.spec.nodes.toObject(), marks: schema.spec.marks.toObject() }
const oneEach = records => `[\n${records.map(record => `    ${JSON.stringify(record)}`).join(",\n")}\n  ]`
writeFileSync(
  new URL("../fixtures/dom.json", import.meta.url),
  `{\n  "schema": ${JSON.stringify(spec)},\n  "parses": ${oneEach(parses)},\n  "serializes": ${oneEach(serializes)},\n` +
    `  "styleProperties": ${JSON.stringify(properties)},\n  "styles": ${oneEach(styles)},\n  "renders": ${oneEach(renders)}\n}\n`,
)
