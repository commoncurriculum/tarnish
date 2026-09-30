//! A token as marked's JSON has it, as far as the lexer makes it: without the fields nothing
//! after the lexer reads, `lang` and `codeBlockStyle` (code), `pre` (paragraph, html), `escaped`
//! (text), `inLink` and `inRawBlock` (inline html), `listType` (list), and `align` and cells'
//! `text` and `header` (table).

use tarnish_js::json::{Map, Value, json};
use tarnish_js::{stack, utf16};

use super::{Cell, Token, TokenData};

impl Token {
    /// The token's fields, its children's among them, as `JSON.stringify` writes marked's.
    pub fn to_json(&self) -> Value {
        let string = |units: &[u16]| Value::String(utf16::to_string(units));
        let optional = |units: &Option<Vec<u16>>| units.as_deref().map_or(Value::Null, string);
        let tokens = |tokens: &[Token]| {
            Value::Array(
                tokens
                    .iter()
                    .map(|token| stack::grow(|| token.to_json()))
                    .collect(),
            )
        };
        let mut out = Map::new();
        let mut set = |key: &str, value: Value| out.insert(key.into(), value);
        set("type", json!(self.kind));
        set("raw", string(&self.raw));
        if let Some(text) = &self.text {
            set("text", string(text));
        }
        if let Some(children) = &self.tokens {
            set("tokens", tokens(children));
        }
        match &self.data {
            TokenData::None => {}
            TokenData::Heading { depth } => {
                set("depth", json!(depth));
            }
            TokenData::Link(link) => {
                set("href", string(&link.href));
                set("title", optional(&link.title));
            }
            TokenData::Html { block } => {
                set("block", json!(block));
            }
            TokenData::Def(def) => {
                set("tag", string(&def.tag));
                set("href", string(&def.href));
                set("title", optional(&def.title));
            }
            TokenData::List(list) => {
                let start = list.start.filter(|start| start.is_finite());
                set("ordered", json!(list.ordered));
                set("start", start.map_or(Value::Null, tarnish_js::number));
                set("loose", json!(list.loose));
                set("items", tokens(&list.items));
            }
            TokenData::ListItem {
                task,
                checked,
                loose,
            } => {
                set("task", json!(task));
                set("checked", checked.map_or(Value::Null, Value::Bool));
                set("loose", json!(loose));
            }
            TokenData::Table(table) => {
                let cells = |cells: &[Cell]| {
                    let cell = |cell: &Cell| json!({ "tokens": tokens(&cell.tokens) });
                    Value::Array(cells.iter().map(cell).collect())
                };
                set("header", cells(&table.header));
                set(
                    "rows",
                    Value::Array(table.rows.iter().map(|row| cells(row)).collect()),
                );
            }
        }
        for (key, value) in self.extra.iter().flat_map(|extra| extra.iter()) {
            set(key, value.clone());
        }
        Value::Object(out)
    }
}
