// Records what V8's built-ins that tarnish-js ports give, to fixtures/js.json, for tarnish-js's
// tests to expect: JSON.parse, of chosen texts and of the texts random.rs's `json_texts` draws;
// Number and parseInt of strings; reading, calling and iterating JSON values, and the TypeError
// thrown where V8 can't; String of them; repeat and replaceAll; and Array.prototype.sort, as the
// comparisons it asks its comparator for, and localeCompare.
//
// A number is written as String writes it, but -0 as "-0", which tells it from 0. A parse that
// throws is recorded as its error's class, as tarnish-js's SyntaxError has no message.
import { outcome, writeFixture } from "./fixture.mjs"

const numberText = number => (Object.is(number, -0) ? "-0" : String(number))

// tarnish-js holds a lone surrogate, which a Rust string can't, as U+FFFD.
function wellFormed(value) {
  if (typeof value === "string") return value.toWellFormed()
  if (Array.isArray(value)) return value.map(wellFormed)
  if (value === null || typeof value !== "object") return value
  return Object.fromEntries(Object.entries(value).map(([key, item]) => [key.toWellFormed(), wellFormed(item)]))
}

// JSON.parse(text): the value as JSON.stringify writes it, each number in it in the order
// JSON.stringify writes them, and String(value).
function parsed(text) {
  let value
  try {
    value = wellFormed(JSON.parse(text))
  } catch (error) {
    return { error: { class: error.constructor.name } }
  }
  const numbers = []
  const walk = item => {
    if (typeof item === "number") numbers.push(numberText(item))
    else if (item !== null && typeof item === "object") Object.values(item).forEach(walk)
  }
  walk(value)
  return { written: JSON.stringify(value), numbers, ...outcome(() => ({ string: String(value) })) }
}

const texts = [
  "{}",
  " [1, 2 ] ",
  "\u{feff}{}",
  "01",
  "1.",
  ".5",
  "-",
  "+1",
  "-0",
  "[-0,0,-0.0,-0e5]",
  '{"a":-0}',
  "1e400",
  "-1e400",
  "[1e400,[1e400]]",
  "[-1e400,[-1e400]]",
  '{"a":1e400,"b":{"c":1e400}}',
  '{"a":-1e400,"b":{"c":-1e400}}',
  "1E2",
  "1e-400",
  "-1e-400",
  "0.1",
  "123456789012345678901234567890",
  "9007199254740993",
  "9007199254740993.0",
  "2.2250738585072011e-308",
  "1.00000000000000011102230246251565404236316680908203125",
  "1.7976931348623158e308",
  "1.7976931348623159e308",
  "4.9e-324",
  "2.4703282292062328e-324",
  "[1e5, 1.5e3, 2.50]",
  "1e21",
  "123e-20",
  '"a\\u0000b"',
  '"\\ud83d\\ude00"',
  '"a\u{1}b"',
  '"\\/"',
  '"\\x"',
  '"\\u00e9"',
  '"\u{2028}"',
  "[1,]",
  '{"a":1,}',
  '{"a":1,"b":2,"a":3}',
  '{"b":1,"2":2,"1":3,"a":4,"01":5,"4294967295":6,"4294967294":7}',
  '{"__proto__":1}',
  '{"__proto__":{"a":1},"b":[{"__proto__":null}]}',
  "nul",
  "true false",
  "\t\n\r [1]",
  "\u{a0}[1]",
  "[1]\u{2028}",
  '{"a":{"b":1}',
  "[[[[",
  '{"a":',
  '"abc',
  "",
  " ",
  "null",
  "true",
  '""',
  "[]",
  "[null]",
  "[[null],[]]",
  "[1,null,[2,3],{}]",
  '{"toString":1}',
  '[1,{"toString":1}]',
  '{"a":[{"toString":"x"}]}',
]

class Random {
  constructor() {
    this.state = 0x9e3779b97f4a7c15n
  }

  // xorshift, as random.rs's `Random::next`.
  next() {
    const mask = (1n << 64n) - 1n
    let x = this.state
    x ^= (x << 13n) & mask
    x ^= x >> 7n
    x ^= (x << 17n) & mask
    this.state = x
    return x
  }

  below(bound) {
    return Number(this.next() % BigInt(bound))
  }

  pick(list) {
    return list[this.below(list.length)]
  }
}

// random.rs's `json_texts`, drawn the same.
function* jsonTexts() {
  const NUMBERS = [
    "0",
    "-0",
    "1",
    "-1",
    "0.1",
    "2.50",
    "1e5",
    "1.5E3",
    "1e-7",
    "-1e21",
    "1e308",
    "4.9e-324",
    "0.30000000000000004",
    "9007199254740993",
    "-9007199254740993",
    "18446744073709551616",
    "123456789012345678901234567890",
    "1e400",
    "2.2250738585072011e-308",
    "1.00000000000000011102230246251565404236316680908203125",
    "9007199254740993.0",
    "1.7976931348623158e308",
  ]
  const STRINGS = ['"a"', '"é"', '"😀"', '"\\ud800"', '"é"', '"\\n"', '"\\/"', '""']
  const KEYS = ['"a"', '"b"', '"a"', '"1"', '"0"', '"01"', '"4294967295"', '"4294967294"', '"__proto__"']
  const random = new Random()
  const space = () => (random.below(4) === 0 ? " " : "")
  const value = depth => {
    switch (random.below(depth > 3 ? 5 : 7)) {
      case 0:
      case 1:
        return random.pick(NUMBERS)
      case 2:
        return random.pick(STRINGS)
      case 3:
        return ["true", "false"][random.below(2)]
      case 4:
        return "null"
      case 5: {
        let text = "["
        for (let index = 0, count = random.below(4); index < count; index++) {
          if (index > 0) text += ","
          text += space()
          text += value(depth + 1)
        }
        return text + "]"
      }
      default: {
        let text = "{"
        for (let index = 0, count = random.below(5); index < count; index++) {
          if (index > 0) text += ","
          text += random.pick(KEYS)
          text += ":"
          text += space()
          text += value(depth + 1)
        }
        return text + "}"
      }
    }
  }
  for (;;) yield value(0)
}

function take(iterator, count) {
  const taken = []
  for (const item of iterator) {
    if (taken.length === count) break
    taken.push(item)
  }
  return taken
}

// `count` strings of one to `most` pieces, each drawn from `pieces` by `random`.
function strings(pieces, most, count, random) {
  return Array.from({ length: count }, () => {
    let string = ""
    for (let index = 0, length = 1 + random.below(most); index < length; index++) string += random.pick(pieces)
    return string
  })
}

// Every string of up to two pieces.
const pairs = pieces => [...pieces, ...pieces.flatMap(first => pieces.map(second => first + second))]

// Digits, signs and prefixes, and JavaScript's whitespace around them, U+180E, which no longer
// is, among it.
const numberPieces = [
  ...["0", "1", "7", "9", "a", "f", "F", "x", "X", "o", "O", "b", "B", "+", "-", ".", "e", "E"],
  ...[" ", "\t", "\n", "\u{b}", "\u{a0}", "\u{2028}", "\u{feff}", "\u{3000}", "\u{180e}"],
  ...[",", "_", "n", "Infinity", "0x", "0o", "0b"],
]

// Digits past a double's 53 bits, past 2^64 and 2^128, and past 2^1024, where each base's
// rounding shows: ties to even, broken by a digit past them.
const longDigits = [
  ...["", "0x", "0X", "0o", "0b", "-", " +"].flatMap(prefix => {
    const digits = { "": "9", "-": "9", " +": "9", "0x": "f", "0X": "F", "0o": "7", "0b": "1" }[prefix]
    return [16, 17, 20, 22, 24, 30, 40, 43, 64, 65, 128, 129, 300, 310, 400, 1023, 1024, 1025, 1100].map(
      length => prefix + digits.repeat(length),
    )
  }),
  "0x20000000000001",
  "0x20000000000003",
  "0x200000000000018",
  "0x20000000000001000000000000000000",
  "0x20000000000001000000000000000001",
  "0x200000000000010000000000000000000000000",
  "0x1" + "0".repeat(255) + "8" + "0".repeat(10),
  "0x1" + "0".repeat(12) + "8" + "0".repeat(30) + "1",
  "0xfffffffffffff8" + "0".repeat(242),
  "0xfffffffffffff7" + "f".repeat(242),
  "0b1" + "0".repeat(52) + "1",
  "0b1" + "0".repeat(52) + "11",
  "0b1" + "0".repeat(1023),
  "0b1" + "0".repeat(1024),
  "0o1" + "0".repeat(17) + "4",
  "0o" + "0".repeat(400) + "17",
  "0x" + "0".repeat(400) + "1f",
  "9007199254740993",
  "9007199254740995",
  "1" + "0".repeat(308),
  "1" + "0".repeat(309),
  "0".repeat(400) + "1",
  "18446744073709551616",
  "340282366920938463463374607431768211456",
  "0x10000000000000000",
  "0x100000000000000000000000000000000",
  "0x" + "1" + "0".repeat(256),
  "0x" + "f".repeat(256),
]

const numberRandom = new Random()
const numberStrings = [...new Set([...pairs(numberPieces), ...strings(numberPieces, 6, 6000, numberRandom), ...longDigits])]

const parseIntPieces = [
  ...["0", "1", "7", "9", "a", "f", "z", "Z", "x", "X", "-", "+", " ", "\t", "\u{a0}", "\u{feff}"],
  ...[".", "e", "px", "0x", "_", "\u{661}"],
]
const radices = [undefined, 2, 3, 7, 8, 10, 16, 32, 36]
const parseIntRandom = new Random()
const parseIntCases = [
  ...[...new Set(strings(parseIntPieces, 5, 3000, parseIntRandom))].map((string, index) => ({
    string,
    radix: radices[index % radices.length],
  })),
  ...radices.flatMap(radix => {
    const digits = radix === undefined ? "9" : (radix - 1).toString(radix)
    return [12, 16, 20, 30, 64, 100, 330, 1100].flatMap(length => [
      { string: digits.repeat(length), radix },
      { string: "1" + "0".repeat(length - 2) + "1", radix },
      { string: "-" + "1" + digits.repeat(length - 1), radix },
    ])
  }),
  ...longDigits.flatMap(string => {
    const prefixes = [["0x", 16], ["0X", 16], ["0o", 8], ["0b", 2]]
    const [prefix, radix] = prefixes.find(([prefix]) => string.startsWith(prefix)) ?? ["", 10]
    const prefixed = radix === 16 ? [{ string, radix: undefined }] : []
    return [{ string: string.slice(prefix.length), radix }, ...prefixed]
  }),
  { string: "  -12px", radix: undefined },
  { string: "0x1A", radix: undefined },
  { string: "0x1A", radix: 16 },
  { string: "0x1A", radix: 10 },
  { string: "1A", radix: 16 },
  { string: "+7", radix: 10 },
  { string: "-0", radix: undefined },
  { string: "-0x", radix: undefined },
  { string: "0x", radix: 16 },
  { string: "x", radix: 10 },
  { string: "0b11", radix: undefined },
  { string: "0b11", radix: 2 },
  { string: "000123", radix: 7 },
  { string: "0".repeat(40) + "z".repeat(20), radix: 36 },
  { string: "0x" + "0".repeat(40) + "f".repeat(20), radix: 16 },
]

// The values each read is of, as JSON, or none for `undefined`.
const values = [
  undefined,
  "null",
  "true",
  "false",
  "0",
  "-0",
  "1.5",
  "1e21",
  "-1e-7",
  '""',
  '"ab"',
  '"😀é"',
  "[]",
  "[1,null,[2,3],{}]",
  "{}",
  '{"a":1}',
  '{"a":{"b":[1]},"color":"red"}',
  '{"toString":1}',
  '[1,{"toString":1}]',
]

// How tarnish-js's tests read a value: what `value.rs` has as `get`, `array_method`, `iterate`
// over an expression or describing the value, and `String`. Each is JavaScript that reads the
// value as `node.value`, as V8 words its TypeError from the source.
const reads = [
  { read: "get", key: "a", source: "return node.value.a" },
  { read: "get", key: "color", source: "return node.value.color" },
  { read: "arrayMethod", callee: "node.value.map", source: "return node.value.map(item => item)" },
  {
    read: "iterate",
    expression: "node.value",
    source: "const items = []; for (const item of node.value) items.push(item); return items",
  },
  { read: "iterate", source: "const [...items] = node.value; return items" },
  { read: "string", source: "return String(node.value)" },
]

const readCases = reads.flatMap(({ source, ...read }) =>
  values.map(json => {
    const node = json === undefined ? {} : { value: JSON.parse(json) }
    return {
      ...read,
      ...(json === undefined ? {} : { json }),
      ...outcome(() => {
        const result = new Function("node", source)(node)
        return result === undefined ? {} : { result }
      }),
    }
  }),
)

// Each count past what memory holds is past what a Rust allocation can ask for, so that no test
// depends on how much the system lets it reserve.
const counts = [-1, -1.5, -0.5, -0, 0, 0.5, 1, 2.9, 3, NaN, Infinity, -Infinity, 2 ** 63, 1e21]
const repeatCases = [
  ...["ab", "", "a", "😀"].flatMap(string => counts.map(count => ({ string, count }))),
  { string: "", count: 2 ** 40 },
].map(({ string, count }) => ({
  string,
  count: String(count),
  ...outcome(() => ({ repeated: string.repeat(count) })),
}))

const replacementPieces = ["$$", "$&", "$`", "$'", "$1", "$<", "$", "$0", "x"]
const replacements = ["[$$|$&|$`|$'|$1|$<|$]", ...pairs(replacementPieces)]
const replaceCases = [
  ["abcb", "b"],
  ["aaa", "aa"],
  ["a😀b😀", "😀"],
  ["$a$", "$"],
  ["xyz", "q"],
].flatMap(([string, pattern]) =>
  replacements.map(replacement => ({
    string,
    pattern,
    replacement,
    replaced: string.replaceAll(pattern, replacement),
  })),
)

// Sorting `items` with a comparator: "difference", `(a, b) => a - b`, or "contrary", which
// answers each pair by the items' places alone and contradicts itself. Each comparison is the
// places the two items had before sorting, and `sorted` their places after.
function sorted(items, comparator) {
  const comparisons = []
  const places = items.map((value, place) => ({ value, place }))
  places.sort((a, b) => {
    comparisons.push(`${a.place}:${b.place}`)
    return comparator === "difference" ? a.value - b.value : ((a.place * 31 + b.place * 17) % 3) - 1
  })
  return { items, comparator, comparisons: comparisons.join(" "), sorted: places.map(({ place }) => place) }
}

const range = (length, at) => Array.from({ length }, (_, index) => at(index))
const blocks = range(28, b => {
  const block = (11 * b) % 28
  const values = range(Math.min(37, 1000 - block * 37), index => block * 37 + index)
  return b % 3 === 0 ? values.reverse() : values
}).flat()
// Runs to merge, merges that gallop both ways, blocks of runs, ties, and a comparator that
// contradicts itself.
const sorts = [
  sorted([0, 3, 2, 1], "difference"),
  sorted([7, 6, 5, 4, 3, 2, 1, 0], "difference"),
  sorted([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 50, 20, 30, 15, 12], "difference"),
  sorted(range(150, i => (i * 37) % 150), "difference"),
  sorted(range(1000, i => (i * 7919) % 1000), "difference"),
  sorted(blocks, "difference"),
  sorted(range(600, i => (i * 7) % 13), "difference"),
  sorted(range(300, i => i), "contrary"),
]

const words = [
  ...["b", "a", "B", "A", "_", "-", "1", "é", "e", "ä", "z", "Z", "aa", "a b", "a-b", "ab", "Ab", "aB"],
  ...["", " ", "10", "2", "ö", "o", "ß", "ss", "æ", "ae", "ç", "c"],
]
const collations = words.flatMap(a => words.map(b => ({ a, b, order: a.localeCompare(b) })))
const collated = { words, sorted: [...words].sort((a, b) => a.localeCompare(b)) }

writeFixture("js", {
  parses: texts.map(text => ({ text, ...parsed(text) })),
  drawn: take(jsonTexts(), 3000).map(text => ({ text, ...parsed(text) })),
  numbers: numberStrings.map(string => ({ string, number: numberText(Number(string)) })),
  parseInts: parseIntCases.map(({ string, radix }) => ({
    string,
    ...(radix === undefined ? {} : { radix }),
    number: numberText(radix === undefined ? parseInt(string) : parseInt(string, radix)),
  })),
  reads: readCases,
  repeats: repeatCases,
  replaces: replaceCases,
  sorts,
  collations,
  collated: [collated],
})
