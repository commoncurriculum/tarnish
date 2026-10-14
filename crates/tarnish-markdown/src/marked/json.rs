//! A token as marked's JSON has it, as far as the lexer makes it: without the fields nothing
//! after the lexer reads, `pre` (paragraph, html), `escaped` (text), `inLink` and `inRawBlock`
//! (inline html), `listType` (list), and cells' `text`, `header` and `align` (table).

use tarnish_js::json::{Map, Value, json};
use tarnish_js::{stack, utf16};

use super::{Align, Cell, Token, TokenData};

impl Token {
    /// The token's fields, its children's among them, as `JSON.stringify` writes marked's.
    pub fn to_json(&self) -> Value {
        let string = |units: &[u16]| Value::String(utf16::to_string(units));
        let optional = |units: &Option<Vec<u16>>| Value::from(units.as_deref().map(string));
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
        for (key, children) in self.child_tokens.iter().flat_map(|fields| fields.iter()) {
            set(key, tokens(children));
        }
        match &self.data {
            TokenData::None => {}
            TokenData::Heading { depth } => {
                set("depth", json!(depth));
            }
            TokenData::Code { lang: Some(lang) } => {
                set("lang", string(lang));
            }
            TokenData::Code { lang: None } => {
                set("codeBlockStyle", json!("indented"));
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
                set("start", Value::from(start));
                set("loose", json!(list.loose));
                set("items", tokens(&list.items));
            }
            TokenData::ListItem {
                task,
                checked,
                loose,
            } => {
                set("task", json!(task));
                set("checked", Value::from(*checked));
                set("loose", json!(loose));
            }
            TokenData::Table(table) => {
                let cells = |cells: &[Cell]| {
                    let cell = |cell: &Cell| json!({ "tokens": tokens(&cell.tokens) });
                    Value::Array(cells.iter().map(cell).collect())
                };
                set("header", cells(&table.header));
                let align = |align: &Option<Align>| Value::from(align.map(Align::as_str));
                set(
                    "align",
                    Value::Array(table.align.iter().map(align).collect()),
                );
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
