// Records Tiptap's block and embed extensions to fixtures/tiptap-blocks.json, as
// harness/tiptap.mjs records them, beside Document, Paragraph, Text, Heading, HardBreak, Bold and
// Italic: `blocks` has every extension here, each as it comes; the others each have an extension
// with the options that change what it makes, and `attributeValues` every one with
// HTMLAttributes, given values that aren't strings. The documents serialized hold attributes and
// content of the types the JS reads otherwise, and the embeds' sources run `new URL` through its
// edge inputs. `attributes` records what `parseAttributes` makes of each string, and
// `serializeAttributes` of each object.
import { parseAttributes, serializeAttributes } from "@tiptap/core"
import { Audio } from "@tiptap/extension-audio"
import { Blockquote } from "@tiptap/extension-blockquote"
import { Bold } from "@tiptap/extension-bold"
import { Code } from "@tiptap/extension-code"
import { CodeBlock } from "@tiptap/extension-code-block"
import { CodeBlockLowlight } from "@tiptap/extension-code-block-lowlight"
import { Details, DetailsContent, DetailsSummary } from "@tiptap/extension-details"
import { Document } from "@tiptap/extension-document"
import { Emoji } from "@tiptap/extension-emoji"
import { HardBreak } from "@tiptap/extension-hard-break"
import { Heading } from "@tiptap/extension-heading"
import { HorizontalRule } from "@tiptap/extension-horizontal-rule"
import { Italic } from "@tiptap/extension-italic"
import { Mathematics } from "@tiptap/extension-mathematics"
import { Paragraph } from "@tiptap/extension-paragraph"
import { RubyText } from "@tiptap/extension-ruby-text"
import { TableOfContents } from "@tiptap/extension-table-of-contents"
import { Text } from "@tiptap/extension-text"
import { Twitch } from "@tiptap/extension-twitch"
import { Youtube } from "@tiptap/extension-youtube"
import { common, createLowlight } from "lowlight"
import { outcome } from "./fixture.mjs"
import { doc, domInputs, markedInputs, otherAttributes, paragraph as p, record, text, withParsed } from "./tiptap.mjs"

const base = [Document, Paragraph, Text, Heading, HardBreak, Bold, Italic]
const details = [Details, DetailsSummary, DetailsContent]
const html = { class: "given more", style: "color: red", "data-given": "yes" }
// Emojis of each kind the node renders: with an image, without one, with neither an emoji nor a
// shortcode, and with empty strings.
const emojis = [
  { name: "smile", emoji: "😄", shortcodes: ["smile", "happy"], tags: [], fallbackImage: "https://e.test/smile.png", version: 0.6 },
  { name: "star", emoji: "★", shortcodes: ["star", "glow"], tags: [] },
  { name: "custom", shortcodes: ["custom_code"], tags: [], fallbackImage: "https://e.test/custom.png" },
  { name: "bare", shortcodes: [], tags: [] },
  { name: "empty", emoji: "", shortcodes: ["empty_code"], tags: [], fallbackImage: "" },
]
const x = p(text("x"))

// Blockquotes and details nested one to three deep, which tests/recorded/blocks.rs folds into the
// conversions of any depth.
const depths = [1, 2, 3]
const quoted = depth => (depth === 0 ? x : { type: "blockquote", content: [quoted(depth - 1)] })
const summary = { type: "detailsSummary", content: [text("s")] }
const nestedDetails = depth =>
  depth === 0 ? x : { type: "details", content: [summary, { type: "detailsContent", content: [nestedDetails(depth - 1)] }] }
const detailsMarkdown = depth =>
  depth === 0
    ? "x"
    : `:::details\n\n:::detailsSummary\n\ns\n\n:::\n\n:::detailsContent\n\n${detailsMarkdown(depth - 1)}\n\n:::\n\n:::`
const detailsHTML = depth =>
  depth === 0
    ? "<p>x</p>"
    : `<details><summary>s</summary><div data-type="detailsContent">${detailsHTML(depth - 1)}</div></details>`
const deepMarkdown = depths.flatMap(depth => ["> ".repeat(depth) + "x", ">".repeat(depth) + " x", detailsMarkdown(depth)])
const deepHTML = depths.flatMap(depth => ["<blockquote>".repeat(depth) + "x", detailsHTML(depth)])
const deepDocs = depths.flatMap(depth => [doc(quoted(depth)), doc(nestedDetails(depth))])

const values = [null, 0, 1, 5, "", "x", true, false, [], ["b", "x"], {}, { some: 1 }]

const markdown = {
  blockquote: [
    "> quote",
    "> one\n> two",
    "> one\n>\n> two",
    "> a\n>\n>\n>\n> b",
    "> # heading\n> para",
    "> a\n\n> b",
    "> > nested",
    ">>> deep",
    ">",
    "> lazy\ncontinuation",
    "> - list\n> - items",
    "> ```\n> code\n> ```",
    "para\n> quote",
    "> **bold** *it*",
    "> a  \n> b",
    "   > indented",
    "\\> escaped",
    "> quote\n\npara",
    "> \n> \n",
  ],
  code: [
    "`code`",
    "a `b` c",
    "``a ` b``",
    "`` `x` ``",
    "`*not em*` <b>",
    "`<b> &amp;`",
    "**bold `code` bold**",
    "`a\nb`",
    "` `",
    "`unclosed",
    "```\nplain\n```",
    "```js\nlet a = 1\n```",
    "~~~ruby startline=3\ncode\n~~~",
    "````\n```\nnested\n```\n````",
    "```\nunclosed",
    "    indented code",
    "    one\n\n    two",
    "  ```\n  shifted\n  ```",
    "```\n```",
    "```js\n```",
    "``` js \nx\n```",
    "```c\\+\\+\nx\n```",
    "```\n\n\n```",
    "```\n*a* <b> &amp;\n```",
    "para\n```\ncode\n```\npara",
  ],
  horizontalRule: ["---", "***", "___", "- - -", " * * *", "a\n---", "a\n\n---\n\nb", "--", "---\n---"],
  details: [
    detailsMarkdown(1),
    ":::details\n:::detailsSummary\nS\n:::\n:::detailsContent\nC\n:::\n:::",
    ":::details {open}\n\n:::detailsSummary\n\nS\n\n:::\n\n:::detailsContent\n\nC\n\n:::\n\n:::",
    ':::details {.a .b #id key="v" k2=\'w\' open flag}\n\nx\n\n:::',
    ":::details\nunclosed",
    ":::details\nx\n:::\nafter",
    "text\n:::details\nx\n:::",
    ":::details\n:::",
    ":::details\n\n:::",
    ":::detailsSummary\n\n**bold** summary\n\n:::",
    ":::detailsSummary\n\na\n\nb\n\n:::",
    ":::detailsContent\n\n# h\n\n- list\n\npara\n\n:::",
    ":::detailsContent\n\n\n\n:::",
    ":::details  {x}  \nx\n:::",
    ":::detailsx\ny\n:::",
    ":::details\n:::atom :::\n:::",
    ":::details\n```\n:::\n```\n:::",
    "> :::details\n> x\n> :::",
  ],
  mathematics: [
    "$x^2$",
    "a $x$ b",
    "$$x$$",
    "$$\nx^2\n$$",
    "text $$x$$ text",
    "$a$$",
    "$ $",
    "$$ $$",
    "$100 and $200",
    "\\$x\\$",
    "$x",
    "**$x$**",
    "$$a$b$$",
    "$  padded  $",
    "$$x$$ after",
  ],
  embeds: [
    ':::youtube {src="https://youtu.be/x" width="320"} :::',
    ":::youtube :::",
    ':::youtube{src="x"} :::',
    ":::youtube {src='x'}:::",
    ':::youtube {src="x"} :::\nafter',
    'text\n:::youtube {src="x"} :::',
    ":::youtube {.cls #id flag} :::",
    ":::youtube\n:::",
    ':::twitch {src="https://twitch.tv/x" autoplay muted="true"} :::',
    ':::audio {src="a.mp3" controls} :::',
    ":::youtubex :::",
  ],
  emoji: [":smile: and :star:", ":unknown:"],
}

const htmlInputs = {
  blockquote: [
    "<blockquote><p>x</p></blockquote>",
    "<blockquote>bare text</blockquote>",
    "<blockquote></blockquote>",
    "<blockquote><blockquote><p>x</p></blockquote></blockquote>",
    "<p>a<blockquote>b</blockquote>c</p>",
    "<blockquote class='x' data-a='1'><p>y</p></blockquote>",
    "<blockquote><h1>h</h1><p>p</p></blockquote>",
  ],
  code: [
    "<p><code>x</code></p>",
    "<p><code><b>x</b></code></p>",
    "<p><b><code>x</code></b></p>",
    "<p><i>a<code>b</code>c</i></p>",
    "<p><code class='c'>x</code></p>",
    "<p><code></code></p>",
    "<pre>x</pre>",
    "<pre><code class='language-js'>x</code></pre>",
    "<pre><code class='lang-js language-ts language-js'>x</code></pre>",
    "<pre><code class='language-'>x</code></pre>",
    "<pre><code class=' language-js  language-js'>x</code></pre>",
    "<pre>  a  \n\n b\t</pre>",
    "<pre><span class='language-py'>x</span></pre>",
    "<pre>text<code class='language-js'>x</code></pre>",
    "<pre><b>bold</b> <code>code</code></pre>",
    "<pre></pre>",
    "<pre>\nleading newline</pre>",
    "<pre><code class='LANGUAGE-js'>x</code></pre>",
    "<pre><code class='lang-rs'>x</code></pre>",
  ],
  horizontalRule: ["<hr>", "<hr class='x'>", "<p>a<hr>b</p>", "<hr><hr>", "<p>a</p><hr><p>b</p>"],
  details: [
    '<details><summary>S</summary><div data-type="detailsContent"><p>C</p></div></details>',
    '<details open><summary>S</summary><div data-type="detailsContent"><p>C</p></div></details>',
    '<details open="false" class="d"><summary>S</summary><div data-type="detailsContent">C</div></details>',
    "<details><summary>S</summary><p>no wrapper</p></details>",
    "<details><p>no summary</p></details>",
    "<details></details>",
    "<summary>alone</summary>",
    '<div data-type="detailsContent"><p>alone</p></div>',
    '<details><summary><b>bold</b> s</summary><div data-type="detailsContent"><p>C</p></div></details>',
  ],
  mathematics: [
    '<p>a <span data-type="inline-math" data-latex="x^2"></span> b</p>',
    '<p><span data-type="inline-math"></span></p>',
    '<p><span data-type="inline-math" data-latex="">ignored</span></p>',
    '<div data-type="block-math" data-latex="\\sum_i x_i"></div>',
    '<div data-type="block-math">content ignored</div>',
    '<span data-type="inline-math" data-latex="y"></span>',
    '<p><div data-type="block-math" data-latex="z"></div></p>',
  ],
  emoji: [
    '<p><span data-type="emoji" data-name="smile">😄</span></p>',
    '<p><span data-type="emoji">no name</span></p>',
    '<p><span data-type="emoji" data-name="">empty</span></p>',
    '<p>a<span data-type="emoji" data-name="unknown" class="c"></span>b</p>',
  ],
  tableOfContents: [
    "<h1 id='intro' data-toc-id='t1'>Intro</h1>",
    "<h2 id=''>empty id</h2>",
    "<h3 data-toc-id='only'>x</h3>",
    "<p id='p' data-toc-id='q'>para</p>",
  ],
  rubyText: [
    "<p><ruby>漢<rt>kan</rt></ruby></p>",
    "<p><ruby><rb>漢</rb><rt>kan</rt></ruby></p>",
    "<p><ruby>漢<rp>(</rp><rt>kan</rt><rp>)</rp></ruby></p>",
    "<p><ruby>no rt</ruby></p>",
    "<p><ruby>a<rt></rt></ruby></p>",
    "<p><ruby rt='attr'>x<rt>y</rt></ruby></p>",
    "<p><ruby rt='5'>x<rt>y</rt></ruby></p>",
    "<p><ruby><b>bold</b><rt>r</rt></ruby></p>",
    "<p><ruby>a<rt>x</rt>b<rt>y</rt></ruby></p>",
    "<p><ruby>a<span><rt>deep</rt></span></ruby></p>",
    "<p>x<ruby>a<rt>r</rt></ruby>y</p>",
  ],
}

// The sources an embed's iframe or audio element may have, through `new URL`'s edge cases.
const youtubeSources = [
  "https://www.youtube.com/embed/abc123",
  "https://www.youtube.com/embed/abc123?start=42",
  "https://www.youtube-nocookie.com/embed/abc?start=7",
  "https://www.youtube.com/embed/videoseries?list=PL123",
  "https://www.youtube.com/embed/videoseries",
  "https://www.youtube.com/embed/videoseries?list=",
  "https://www.youtube.com/embed/videoseries?list=a%26b",
  "https://www.youtube.com/embed/videoseries?list=a+b",
  "https://www.youtube.com/embed/videoseries?list=a%20b",
  "https://youtube.com/watch?v=abc",
  "https://YOUTUBE.COM/embed/abc",
  "https://www.youtube.com/embed/abc/extra",
  "https://www.youtube.com/embed/a b",
  "https://www.youtube.com/embed/abc?start=10s",
  "https://www.youtube.com/embed/abc?start=abc",
  "https://www.youtube.com/embed/abc?start=",
  "https://www.youtube.com/embed/abc?start=1&start=2",
  "https://www.youtube.com/embed/abc?start=%2B5",
  "https://www.youtube.com/embed/abc?start=+5",
  "https://www.youtube.com/embed/abc?start=-0",
  "https://www.youtube.com/embed/abc?start=1e3",
  "https://www.youtube.com/embed/abc?START=5",
  "https://www.youtube.com/embed/abc#t=5",
  "  https://www.youtube.com/embed/abc  ",
  "https://www.you&#9;tube.com/embed/ab&#10;c",
  "https:\\\\www.youtube.com\\embed\\abc",
  "https://www.youtube.com/embed/../embed/abc",
  "https://www.youtube.com/x/../embed/abc",
  "https://www.youtube.com/embed/%61bc",
  "https://www.%79outube.com/embed/abc",
  "https://www.ｙｏｕｔｕｂｅ.com/embed/abc",
  "https://user:pw@www.youtube.com/embed/abc",
  "https://www.youtube.com:443/embed/abc",
  "https://www.youtube.com:8080/embed/abc",
  "https://www.youtube.com./embed/abc",
  "ftp://youtube.com/embed/abc",
  "foo://youtube.com/embed/abc",
  "//www.youtube.com/embed/abc",
  "not a url",
  "https://[::1]/embed/x",
  "https://m.youtube.com/embed/abc",
  "https://www.www.youtube.com/embed/abc",
  "https://exa mple.com/embed/abc",
  "javascript:alert(1)",
]
const twitchSources = [
  "https://player.twitch.tv/?video=123&parent=x",
  "https://player.twitch.tv/?channel=foo&autoplay=true&muted=true",
  "https://player.twitch.tv/?video=123&time=1h2m3s&autoplay=false",
  "https://player.twitch.tv/?video=1&video=2",
  "https://player.twitch.tv/?video=1&muted=TRUE",
  "https://player.twitch.tv/?video=&channel=chan",
  "https://player.twitch.tv/?video=a%20b",
  "https://player.twitch.tv/?channel=a+b",
  "https://player.twitch.tv/?channel=a%2Fb",
  "https://player.twitch.tv/",
  "https://www.player.twitch.tv/?video=1",
  "https://PLAYER.TWITCH.TV/?video=1",
  "https://clips.twitch.tv/embed?clip=Abc-1&parent=x",
  "https://clips.twitch.tv/embed?clip=",
  "https://clips.twitch.tv/embed",
  "https://clips.twitch.tv/embed/?clip=x",
  "https://clips.twitch.tv/other?clip=x",
  "https://www.twitch.tv/videos/123",
  "https://player.twitch.tv/?video=1&time=",
  "not a url",
]
const audioSources = [
  "a.mp3",
  "https://a.test/a.mp3",
  "data:audio/mp3;base64,AA",
  "javascript:alert(1)",
  "",
]
const frame = (wrapper, src, rest = "") => `<div ${wrapper}><iframe src="${src}"${rest}></iframe></div>`
// Each embed with a width or a height of its own, which an option's default fills in where it
// has none, and each without a source, outside its wrapper and inside a paragraph, which the
// `inline` option places.
const youtubeSizes = [
  'width="320" height="240"',
  'width=" 12 "',
  'width=""',
  'width="abc"',
  'width="12px"',
  'width="0x10"',
  'width="1e2"',
  'width="-5"',
  'width="Infinity"',
  'height="  "',
].map(rest => frame("data-youtube-video", "https://www.youtube.com/embed/abc", " " + rest))
const youtubeShapes = [
  '<div data-youtube-video><iframe src=""></iframe></div>',
  "<div data-youtube-video><iframe></iframe></div>",
  '<iframe src="https://www.youtube.com/embed/abc"></iframe>',
  '<p>a<div data-youtube-video><iframe src="https://www.youtube.com/embed/abc"></iframe></div>b</p>',
]
const twitchSizes = ['width="100px" height="50"', 'width="abc"', 'width=""', 'width=" 7"'].map(rest =>
  frame("data-twitch-video", "https://player.twitch.tv/?video=1", " " + rest),
)
const twitchShapes = [
  "<div data-twitch-video><iframe></iframe></div>",
  '<p>a<div data-twitch-video><iframe src="https://player.twitch.tv/?video=1"></iframe></div>b</p>',
]
htmlInputs.youtube = [...youtubeSources.map(src => frame("data-youtube-video", src)), ...youtubeSizes, ...youtubeShapes]
htmlInputs.twitch = [...twitchSources.map(src => frame("data-twitch-video", src)), ...twitchSizes, ...twitchShapes]
htmlInputs.audio = [
  ...audioSources.map(src => `<audio src="${src}"></audio>`),
  "<audio src='a.mp3' controls autoplay loop muted preload='none' controlslist='nodownload' crossorigin='anonymous' disableremoteplayback></audio>",
  "<audio src='a.mp3' controls='false' loop='true' preload='5' muted='1.5'></audio>",
  "<audio>no src</audio>",
  "<p>a<audio src='a.mp3'></audio>b</p>",
]


const node = (type, attrs, content) => ({ type, ...(attrs === undefined ? {} : { attrs }), ...(content === undefined ? {} : { content }) })
const docs = {
  blockquote: [
    doc(node("blockquote", undefined, [p(text("one")), p(text("two"))])),
    doc(node("blockquote", undefined, [p(text("a")), p(), p(text("b"))])),
    doc(node("blockquote", undefined, [p(text("a"), { type: "hardBreak" }, text("b"))])),
    doc(node("blockquote", undefined, [node("heading", { level: 2 }, [text("h")]), node("blockquote", undefined, [p(text("in"))])])),
    doc(node("blockquote", undefined, [])),
    doc(node("blockquote")),
    doc(node("blockquote", undefined, [text("bare *text*")])),
    doc(node("blockquote", undefined, [node("unknown", undefined, [text("u")]), p(text("after"))])),
    doc(node("blockquote", undefined, [p(text("  spaced  ")), p(text(" "))])),
    ...values.map(content => doc(node("blockquote", undefined, content))),
    ...values.map(child => doc(node("blockquote", undefined, [child]))),
    doc(node("blockquote", { class: "q" }, [p(text("attrs"))])),
  ],
  code: [
    doc(p(text("a`b", { type: "code" }))),
    doc(p(text("``", { type: "code" }))),
    doc(p(text("*a* <b> & \\", { type: "code" }), text(" *c*"))),
    doc(p(text("x", { type: "code" }, { type: "bold" }))),
    doc(p(text("x", { type: "bold" }, { type: "code" }))),
    doc(p(text("a", { type: "code" }), text("b", { type: "code" }, { type: "italic" }), text("c"))),
    doc(p(text("lone", { type: "code", attrs: { a: 1 } }))),
    doc(node("code", undefined, [text("as a node")])),
    doc(node("code")),
    doc(node("codeBlock", { language: "js" }, [text("let a = 1\n```\nb")])),
    doc(node("codeBlock", { language: null }, [text("*not* <escaped>")])),
    doc(node("codeBlock", undefined, [text("no attrs")])),
    doc(node("codeBlock", { language: "" }, [])),
    doc(node("codeBlock", { language: "py" })),
    doc(node("codeBlock")),
    ...values.map(language => doc(node("codeBlock", { language }, [text("v")]))),
    doc(node("codeBlock", { language: "js" }, [text("a\n\nb\n")])),
    doc(node("codeBlock", { language: "js" }, { type: "text", text: "obj" })),
    doc(node("codeBlock", null, [text("null attrs")])),
  ],
  horizontalRule: [
    doc(p(text("a")), node("horizontalRule"), p(text("b"))),
    doc(node("horizontalRule", { a: 1 }, [text("ignored")])),
    doc(node("horizontalRule"), node("horizontalRule")),
  ],
  details: [
    doc(node("details", undefined, [node("detailsSummary", undefined, [text("S")]), node("detailsContent", undefined, [p(text("C"))])])),
    doc(node("details", { open: true }, [node("detailsSummary", undefined, [text("S")]), node("detailsContent", undefined, [p(text("C"))])])),
    doc(node("details", { open: false }, [node("detailsSummary", undefined, [text("S")]), node("detailsContent", undefined, [p(text("C"))])])),
    doc(
      node("details", undefined, [
        node("detailsSummary", undefined, [text("a "), text("b", { type: "bold" }), text(" c")]),
        node("detailsContent", undefined, [p(text("one")), p(text("two"))]),
      ]),
    ),
    doc(node("details", undefined, [node("detailsSummary"), node("detailsContent", undefined, [p()])])),
    doc(node("details", { class: "x y", id: "i", key: "v", flag: true, off: false, none: null, n: 5 }, [])),
    doc(node("details")),
    ...values.map(attrs => doc(node("details", attrs, []))),
    ...values.map(content => doc(node("details", undefined, content))),
    doc(node("detailsSummary", undefined, [text("top")])),
    doc(node("detailsContent", { class: "c" }, [p(text("top"))])),
  ],
  mathematics: [
    doc(p(text("a "), node("inlineMath", { latex: "x^2" }), text(" b"))),
    doc(node("blockMath", { latex: "\\sum_i x_i" })),
    doc(p(node("inlineMath")), node("blockMath")),
    ...values.map(latex => doc(p(node("inlineMath", { latex })), node("blockMath", { latex }))),
    doc(p(node("inlineMath", null)), node("blockMath", null)),
    doc(p(text("$x$ and $$y$$"))),
  ],
  emoji: [
    ...["smile", "happy", "star", "glow", "custom", "custom_code", "bare", "empty", "empty_code", "unknown", null, 5, ""].map(name =>
      doc(p(text("a"), node("emoji", { name }), text("b"))),
    ),
    doc(p(node("emoji"))),
    doc(p(node("emoji", null))),
  ],
  youtube: [
    ...[
      "https://www.youtube.com/watch?v=abc",
      "https://youtu.be/abc",
      "https://youtu.be/",
      "https://www.youtube.com/watch?list=PL1",
      "https://www.youtube.com/watch?v=abc&list=PL1",
      "https://www.youtube.com/shorts/abc",
      "https://www.youtube.com/embed/abc",
      "https://music.youtube.com/watch?v=x",
      "https://www.youtube.com/watch?feature=share",
      "youtube.com/v/abc",
      "https://example.com",
      "",
    ].map(src => doc(node("youtube", { src }))),
    ...values.map(src => doc(node("youtube", { src }))),
    ...[30, "30", 0, null, "x", 1.5].map(start => doc(node("youtube", { src: "https://www.youtube.com/watch?v=abc", start }))),
    doc(node("youtube", { src: "https://www.youtube.com/watch?list=PL1", start: 5 })),
    doc(node("youtube", { src: "https://youtu.be/abc", width: "100%", height: 10 })),
    doc(node("youtube", { src: "x", width: 1, height: 2, start: 3, extra: "no" })),
    doc(node("youtube", { src: "x", start: true, width: false, height: null })),
    ...["x", 5, true, ["a"]].map(attrs => doc(node("youtube", attrs))),
  ],
  twitch: [
    ...[
      "https://www.twitch.tv/videos/123",
      "https://twitch.tv/chan",
      "https://www.twitch.tv/chan/",
      "https://www.twitch.tv/chan/clip/Clip-1",
      "https://clips.twitch.tv/Clip-1",
      "https://clips.twitch.tv/",
      "https://twitch.tv/videos/123?t=1",
      "twitch.tv/chan",
      "https://example.com",
      "",
    ].map(src => doc(node("twitch", { src }))),
    ...values.map(src => doc(node("twitch", { src }))),
    ...[true, "x", null, 0, "false"].map(autoplay => doc(node("twitch", { src: "https://twitch.tv/videos/1", autoplay, muted: autoplay }))),
    ...["1h", 5, null, "", "a b"].map(time => doc(node("twitch", { src: "https://twitch.tv/videos/1", time }))),
    ...["1h", null].map(time => doc(node("twitch", { src: "https://twitch.tv/chan", time }))),
    doc(node("twitch", { src: "https://twitch.tv/chan/clip/C", autoplay: true, muted: true, time: "1h" })),
    ...["x", 5, ["a"]].map(attrs => doc(node("twitch", attrs))),
  ],
  audio: [
    ...[
      "https://x.test/a.mp3",
      "http://x.test/a.mp3",
      "/a.mp3",
      "./a.mp3",
      "../a.mp3",
      "//cdn.test/a.mp3",
      "a.mp3",
      "javascript:alert(1)",
      "ftp://x.test/a.mp3",
      "HTTPS://X.TEST/a.mp3",
      "data:audio/mp3;base64,AA",
      "DATA:AUDIO/MP3;BASE64,AA",
      "data:audio/;base64,AA",
      " https://x.test/a.mp3",
      "",
    ].map(src => doc(node("audio", { src }))),
    ...values.map(src => doc(node("audio", { src }))),
    doc(node("audio", { src: "/a.mp3", controls: false, autoplay: true, loop: "", muted: 0, preload: null, controlslist: "x", crossorigin: "", disableremoteplayback: true })),
    doc(node("audio", { src: "/a.mp3", controlslist: null })),
    ...["x", 5, ["a"]].map(attrs => doc(node("audio", attrs))),
  ],
  tableOfContents: [
    doc(node("heading", { level: 1, id: "intro", "data-toc-id": "t1" }, [text("Intro")])),
    doc(node("heading", { level: 2, id: null, "data-toc-id": null }, [text("none")])),
    doc(node("heading", { level: 3, id: 5, "data-toc-id": true }, [text("types")])),
    doc(node("paragraph", { id: "p", "data-toc-id": "q" }, [text("para")])),
  ],
  rubyText: [
    ...["kan", null, "", 5, true, ["b", "x"], ["b"], {}, [], "<b>"].map(rt =>
      doc(p(text("a"), text("漢", { type: "rubyText", attrs: { rt } }), text("b"))),
    ),
    doc(p(text("bold", { type: "bold" }, { type: "rubyText", attrs: { rt: "r" } }))),
    doc(p(text("x", { type: "rubyText" }))),
  ],
}
// Emojis of each name, and of names that aren't strings, in blocks beside other text.
const emojiTexts = [
  ...docs.emoji,
  ...[["smile"], {}, true].map(name => doc(p(node("emoji", { name })))),
  doc(
    p(node("emoji", { name: "star" }), node("hardBreak"), node("emoji", { name: "custom" })),
    p(text("x"), node("emoji", { name: "glow" })),
  ),
]

const attributeStrings = [
  "",
  "   ",
  ".a",
  ".a .b",
  ".a.b",
  "#id",
  "#one #two",
  'key="value"',
  "key='value'",
  'key = "spaced"',
  'a="1" a="2"',
  'class="kv" .cls',
  ".cls class=\"kv\"",
  "flag",
  "flag other-flag",
  "flag=unquoted",
  "9lead",
  'mixed="a\' b',
  'q="has .dot #hash"',
  'k="__QUOTED_0__"',
  "k=__QUOTED_5__",
  '"lone quoted"',
  'open flag="x" open',
  ".a #b c d=\"e\" f",
  "\t.tab\n#nl",
  ".ü #ü",
]
const attributeObjects = [
  {},
  { class: "a b" },
  { class: "  a   b  " },
  { class: "" },
  { class: 5 },
  { class: ["a", "b"] },
  { id: "x" },
  { id: 0 },
  { id: { a: 1 } },
  { open: true },
  { off: false, none: null },
  { n: 5, s: "s", q: 'a"b', o: { a: 1 }, arr: [1, 2] },
  { class: "c", id: "i", k: "v", flag: true },
  null,
  "ab",
  ["x", true],
  5,
  true,
]

// An extension with HTMLAttributes, which reach only renderHTML.
const attributed = (extension, docs) => ({ extensions: [...base, extension.configure({ HTMLAttributes: html })], serializesHTML: docs })
// An extension with options that reach its parse rules and renderHTML.
const optioned = (extensions, inputs, docs) => ({
  extensions: [...base, ...extensions],
  parsesHTML: inputs,
  serializesHTML: withParsed(docs),
})

record(
  "tiptap-blocks",
  {
    blocks: {
      extensions: [
        ...base,
        Blockquote,
        Code,
        CodeBlock,
        HorizontalRule,
        ...details,
        Mathematics,
        Emoji.configure({ emojis }),
        Youtube,
        Twitch,
        Audio,
        TableOfContents,
        RubyText,
      ],
      parsesMarkdown: [...Object.values(markdown).flat(), ...markedInputs(), ...deepMarkdown],
      parsesHTML: [...Object.values(htmlInputs).flat(), ...domInputs(), ...deepHTML],
      serializesMarkdown: withParsed([...Object.values(docs).flat(), ...deepDocs]),
      serializesHTML: withParsed([...Object.values(docs).flat(), ...deepDocs]),
      generatesText: emojiTexts.map(doc => ({ doc })),
    },
    blockquoteOptions: attributed(Blockquote, docs.blockquote),
    codeOptions: optioned(
      [
        Code.configure({ HTMLAttributes: html }),
        CodeBlock.configure({ languageClassPrefix: "lang-", defaultLanguage: "plain", HTMLAttributes: html }),
      ],
      htmlInputs.code,
      docs.code,
    ),
    codeNullPrefix: optioned([Code, CodeBlock.configure({ languageClassPrefix: null })], htmlInputs.code, docs.code),
    codeEmptyPrefix: optioned([Code, CodeBlock.configure({ languageClassPrefix: "" })], htmlInputs.code, docs.code),
    // CodeBlockLowlight is CodeBlock with a plugin that decorates the code it highlights.
    lowlight: {
      extensions: [...base, Code, CodeBlockLowlight.configure({ lowlight: createLowlight(common) })],
      parsesHTML: ["<pre><code class='language-js'>x</code></pre>"],
    },
    horizontalRuleOptions: attributed(HorizontalRule, docs.horizontalRule),
    detailsOptions: optioned(
      [
        Details.configure({ persist: true, HTMLAttributes: html }),
        DetailsSummary.configure({ HTMLAttributes: { class: "summary" } }),
        DetailsContent.configure({ HTMLAttributes: { class: "content", "data-type": "given" } }),
      ],
      htmlInputs.details,
      docs.details,
    ),
    emojiOptions: {
      extensions: [...base, Emoji.configure({ emojis, forceFallbackImages: true, HTMLAttributes: html })],
      serializesHTML: docs.emoji,
    },
    youtubeOptions: optioned(
      [
        Youtube.configure({
          allowFullscreen: false,
          autoplay: true,
          ccLanguage: "es",
          ccLoadPolicy: true,
          controls: false,
          disableKBcontrols: true,
          enableIFrameApi: true,
          endTime: 90,
          height: "50%",
          interfaceLanguage: "fr",
          ivLoadPolicy: 3,
          loop: true,
          modestBranding: true,
          HTMLAttributes: html,
          nocookie: true,
          origin: "https://o.test",
          playlist: "a,b",
          progressBarColor: "white",
          width: 320.5,
          rel: 0,
        }),
      ],
      [...youtubeSizes, ...youtubeShapes],
      docs.youtube,
    ),
    youtubeInline: optioned([Youtube.configure({ inline: true, rel: undefined })], [...youtubeSizes, ...youtubeShapes], docs.youtube),
    twitchOptions: optioned(
      [
        Twitch.configure({
          allowFullscreen: false,
          autoplay: true,
          muted: true,
          time: "1h2m3s",
          parent: "a b&c=d.test",
          height: "50%",
          width: 320,
          HTMLAttributes: html,
        }),
      ],
      [...twitchSizes, ...twitchShapes],
      docs.twitch,
    ),
    twitchInline: optioned([Twitch.configure({ inline: true, parent: "" })], [...twitchSizes, ...twitchShapes], docs.twitch),
    audioOptions: optioned(
      [
        Audio.configure({
          allowBase64: true,
          autoplay: true,
          controls: false,
          loop: true,
          muted: true,
          preload: null,
          controlslist: "nodownload",
          crossorigin: "anonymous",
          disableRemotePlayback: true,
          HTMLAttributes: html,
        }),
      ],
      htmlInputs.audio,
      docs.audio,
    ),
    audioInline: optioned([Audio.configure({ inline: true, preload: "auto" })], htmlInputs.audio, docs.audio),
    tableOfContentsAnchors: optioned(
      [TableOfContents.configure({ anchorTypes: ["paragraph", "heading"] })],
      htmlInputs.tableOfContents,
      docs.tableOfContents,
    ),
    rubyTextOptions: attributed(RubyText, docs.rubyText),
    attributeValues: {
      extensions: [
        ...base,
        ...[Blockquote, Code, CodeBlock, HorizontalRule, ...details, Youtube, Twitch, Audio, RubyText].map(extension =>
          extension.configure({ HTMLAttributes: otherAttributes }),
        ),
        Emoji.configure({ emojis, HTMLAttributes: otherAttributes }),
      ],
      serializesHTML: [
        ...docs.blockquote,
        ...docs.code,
        ...docs.horizontalRule,
        ...docs.details,
        ...docs.emoji,
        ...docs.youtube,
        ...docs.twitch,
        ...docs.audio,
        ...docs.rubyText,
      ],
    },
  },
  {
    attributes: [
      ...attributeStrings.map(input => ({ parse: input, ...outcome(() => ({ attributes: parseAttributes(input) })) })),
      ...attributeObjects.map(input => ({ serialize: input, ...outcome(() => ({ string: serializeAttributes(input) })) })),
    ],
  },
)
