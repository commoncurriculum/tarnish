//! `@tiptap/extension-list`'s `TaskList`, which `@tiptap/extension-task-list` exports, with its
//! Markdown tokenizer of GFM task items, `- [ ]` and `- [x]`, nested by indentation.

use std::sync::LazyLock;

use super::bullet_list::render_list;
use crate::markdown::{IndentedBlockConfig, ParseHelpers, Parsed, parse_indented_blocks};
use crate::merge_attributes::render_wrapping;
use crate::{NodeExtension, ParseHtml};
use tarnish_js::json::Map;
use tarnish_js::regexp::RegExp;
use tarnish_js::units::Units;
use tarnish_js::{Error, utf16};
use tarnish_markdown::marked::{Lexer, List, Token, TokenData, Tokens};

pub struct TaskListOptions {
    /// The node type of the list's items.
    pub item_type_name: &'static str,
    pub html_attributes: Map,
}

impl Default for TaskListOptions {
    fn default() -> Self {
        TaskListOptions {
            item_type_name: "taskItem",
            html_attributes: Map::new(),
        }
    }
}

pub fn task_list(options: TaskListOptions) -> NodeExtension {
    let data_type = [("data-type", "taskList")];
    NodeExtension::create("taskList")
        .group("block list")
        .content(format!("{}+", options.item_type_name))
        .parse_html([ParseHtml::tag(r#"ul[data-type="taskList"]"#).priority(51)])
        .render_html(render_wrapping("ul", options.html_attributes, data_type))
        .parse_markdown(parse_markdown)
        .render_markdown(render_list)
        .markdown_block_tokenizer(start, tokenize)
        .indents_content()
}

fn parse_markdown(token: &Token, helpers: &ParseHelpers) -> Result<Parsed, Error> {
    let items = token.list().map_or(&[][..], |list| &list.items);
    let content = helpers.parse_children(items)?;
    Ok(Parsed::Node(helpers.create_node(
        "taskList",
        None,
        Some(content),
    )))
}

static START: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"^\s*[-+*]\s+\[([ xX])\]\s+", ""));
static ITEM: LazyLock<RegExp> =
    LazyLock::new(|| RegExp::new(r"^(\s*)([-+*])\s+\[([ xX])\]\s+(.*)$", ""));

/// The tokenizer's `start(src)`: a task item can only start `src`.
fn start(src: &Units) -> Option<usize> {
    START.test(src).then_some(0)
}

/// What the item pattern takes from a task item's line.
struct TaskItemData {
    main_content: Units,
    checked: bool,
}

/// The `parseIndentedBlocks` config of task items, whose nested content is parsed again for
/// task items first.
struct TaskItems;

impl IndentedBlockConfig for TaskItems {
    type Item = TaskItemData;

    fn item(&self, line: &[u16]) -> Option<(usize, TaskItemData)> {
        let found = ITEM.exec(line)?;
        let checked = found.get(3).unwrap_or_default();
        let checked = utf16::is(checked, "x") || utf16::is(checked, "X");
        let main_content = Units::from(found.get(4).unwrap_or_default());
        let indent_level = found.get(1).unwrap_or_default().len();
        Some((
            indent_level,
            TaskItemData {
                main_content,
                checked,
            },
        ))
    }

    fn create_token(
        &self,
        item: TaskItemData,
        nested_tokens: Option<Vec<Token>>,
        lexer: &mut Lexer,
    ) -> Result<Token, Error> {
        let inline = lexer.inline_tokens(&item.main_content)?;
        let nested = nested_tokens.map(|nested| vec![("nestedTokens", Tokens::from(nested))]);
        Ok(Token {
            text: Some(item.main_content),
            tokens: Some(inline.into()),
            data: TokenData::ListItem {
                task: true,
                checked: Some(item.checked),
                loose: false,
            },
            child_tokens: nested.map(Box::new),
            ..Token::new("taskItem", Units::default())
        })
    }

    /// `parseTaskListContent(content)`: the task list the content starts with and the blocks
    /// after it, or else the content's blocks.
    fn nested(&self, content: Units, lexer: &mut Lexer) -> Result<Option<Vec<Token>>, Error> {
        let mut tokens = Vec::new();
        match parse_indented_blocks(&content, self, lexer)? {
            Some(nested) => {
                let remainder = content.substring(nested.raw.len());
                tokens.push(task_list_token(nested.raw, nested.items));
                if !utf16::trim(&remainder).is_empty() {
                    lexer.block_tokens(&remainder, &mut tokens, false)?;
                }
            }
            None => lexer.block_tokens(&content, &mut tokens, false)?,
        }
        Ok(Some(tokens))
    }
}

fn task_list_token(raw: Units, items: Vec<Token>) -> Token {
    Token {
        data: TokenData::List(Box::new(List {
            items,
            ..List::default()
        })),
        ..Token::new("taskList", raw)
    }
}

/// The `markdownTokenizer`: the task list the source starts with, after any blank lines.
fn tokenize(src: &Units, _: &[Token], lexer: &mut Lexer) -> Result<Option<Token>, Error> {
    Ok(parse_indented_blocks(src, &TaskItems, lexer)?
        .map(|result| task_list_token(result.raw, result.items)))
}
