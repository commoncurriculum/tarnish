//! Reading nodes, fragments and marks from JSON, as their `fromJSON`s do.

use super::fragment::Fragment;
use super::mark::{Mark, Marks};
use super::node::Node;
use super::schema::Schema;
use crate::error::{Error, Result};
use crate::js::{Given, Json};
use crate::json::Value;
use crate::stack;
use crate::text::Text;

/// Reads a document, sharing what JavaScript shares, or could without it showing: each mark
/// type's mark with its defaults, as the type's `instance`, and the set of just that mark.
pub(crate) struct Reader<'s> {
    schema: &'s Schema,
    /// Each mark type's mark with its defaults, and the set of just that mark, once read.
    instances: Vec<Option<(Mark, Marks)>>,
    /// The children of the fragments being read, the innermost's last.
    children: Vec<Node>,
    /// The marks of the node being read.
    marks: Vec<Mark>,
}

impl<'s> Reader<'s> {
    pub fn new(schema: &'s Schema) -> Reader<'s> {
        Reader {
            schema,
            instances: Vec::new(),
            children: Vec::new(),
            marks: Vec::new(),
        }
    }

    pub fn node<'a>(&mut self, json: impl Json<'a>) -> Result<Node> {
        if !json.truthy() {
            return Err(Error::Range("Invalid input for Node.fromJSON".into()));
        }
        // The likeliest first, for a reader that looks each up and stops once it has found as
        // many as the object has.
        let [name, text, content, marks, attrs] =
            json.fields(["type", "text", "content", "marks", "attrs"]);
        let marks = match marks.filter(|marks| marks.truthy()) {
            None => Mark::none(),
            Some(marks) => self.marks(marks)?,
        };
        let name = name.map_or("undefined".into(), Json::string);
        if name == "text" {
            let text = text
                .and_then(Json::text)
                .ok_or_else(|| Error::Range("Invalid text node in JSON".into()))?;
            let text_type = self.schema.text_type();
            let attrs = text_type
                .default_attrs()
                .cloned()
                .expect("text has no attributes");
            return Node::new_text(text_type, attrs, Text::from(&*text), marks);
        }
        let content = match content {
            Some(content) => self.fragment(content)?,
            None => Fragment::empty(),
        };
        let node_type = self.schema.expect_node_type(&name)?;
        let attrs = attrs.map_or(Given::Falsy(Value::Null), Json::attrs);
        let attrs = node_type.attrs_given(&attrs)?;
        node_type.check_attrs(&attrs)?;
        Ok(Node::new(node_type, attrs, content, marks))
    }

    pub fn fragment<'a>(&mut self, json: impl Json<'a>) -> Result<Fragment> {
        if !json.truthy() {
            return Ok(Fragment::empty());
        }
        let items = json
            .items()
            .ok_or_else(|| Error::Range("Invalid input for Fragment.fromJSON".into()))?;
        let start = self.children.len();
        for item in items {
            match stack::grow(|| self.node(item)) {
                Ok(node) => self.push_child(start, node),
                Err(failed) => {
                    self.children.truncate(start);
                    return Err(failed);
                }
            }
        }
        Ok(Fragment::from_drain(self.children.drain(start..)))
    }

    /// Adds a child to the fragment whose children start at `start`, joined to the text before
    /// it when they have the same marks, as `Fragment.fromArray` joins them.
    fn push_child(&mut self, start: usize, node: Node) {
        if self.children.len() > start {
            let last = self.children.last_mut().expect("a child");
            if let Some(joined) = last.join_text(&node) {
                *last = joined;
                return;
            }
        }
        self.children.push(node);
    }

    pub fn mark<'a>(&mut self, json: impl Json<'a>) -> Result<Mark> {
        if !json.truthy() {
            return Err(Error::Range("Invalid input for Mark.fromJSON".into()));
        }
        let [name, attrs] = json.fields(["type", "attrs"]);
        let name = name.map_or("undefined".into(), Json::string);
        let mark_type = self
            .schema
            .mark_type(&name)
            .ok_or_else(|| Error::Range(format!("There is no mark type {name} in this schema")))?;
        let attrs = attrs.map_or(Given::Falsy(Value::Null), Json::attrs);
        let mark = match (&attrs, mark_type.default_attrs()) {
            (Given::Falsy(_), Some(_)) => self.instance(mark_type.rank()).0.clone(),
            _ => mark_type.create_given(&attrs)?,
        };
        mark_type.check_attrs(mark.attrs())?;
        Ok(mark)
    }

    /// A node's marks, as a set.
    fn marks<'a>(&mut self, json: impl Json<'a>) -> Result<Marks> {
        let items = json
            .items()
            .ok_or_else(|| Error::Range("Invalid mark data for Node.fromJSON".into()))?;
        self.marks.clear();
        for item in items {
            let mark = self.mark(item)?;
            self.marks.push(mark);
        }
        if let [mark] = self.marks.as_slice()
            && let Some(Some((instance, alone))) = self.instances.get(mark.mark_type().rank())
            && instance.ptr_eq(mark)
        {
            return Ok(alone.clone());
        }
        Ok(Mark::set_from_vec(&mut self.marks))
    }

    /// The mark type's mark with its defaults, which the type must have, and the set of just
    /// that mark.
    fn instance(&mut self, rank: usize) -> &(Mark, Marks) {
        if self.instances.is_empty() {
            self.instances.resize(self.schema.mark_types().len(), None);
        }
        self.instances[rank].get_or_insert_with(|| {
            let mark_type = self.schema.mark_types().nth(rank).expect("a mark type");
            let defaults = mark_type.default_attrs().cloned().expect("defaults");
            let mark = Mark::new(mark_type, defaults);
            (mark.clone(), Marks::from([mark]))
        })
    }
}
