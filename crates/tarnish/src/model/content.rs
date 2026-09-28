//! Content expressions, compiled to automata whose states are [`ContentMatch`]es.
//!
//! The automaton's states and edges come out in the order ProseMirror builds them in, since
//! that order decides which nodes [`ContentMatch::fill_before`] and
//! [`ContentMatch::find_wrapping`] choose.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::{Arc, LazyLock, Mutex};

use super::fragment::Fragment;
use super::node::Node;
use super::schema::{NodeType, NodeTypeData, Schema};
use crate::error::{Error, Result};
use crate::js;
use crate::stack;
use crate::text::is_js_space;

/// The compiled form of one content expression.
pub struct Automaton {
    states: Vec<State>,
    wrappings: Mutex<Wrappings>,
}

/// [`ContentMatch::find_wrapping`]'s answers, by state and target node type.
type Wrappings = HashMap<(usize, usize), Option<Arc<[usize]>>>;

struct State {
    valid_end: bool,
    /// Edges: a node type's index in the schema, and the state after it.
    next: Vec<(usize, usize)>,
}

static EMPTY: LazyLock<Arc<Automaton>> = LazyLock::new(|| {
    Arc::new(Automaton {
        states: vec![State {
            valid_end: true,
            next: Vec::new(),
        }],
        wrappings: Mutex::default(),
    })
});

impl Automaton {
    /// The automaton of the empty expression, whose nodes are leaves.
    pub(crate) fn empty() -> Arc<Automaton> {
        EMPTY.clone()
    }

    pub(crate) fn is_empty_match(&self) -> bool {
        std::ptr::eq(self, &**EMPTY)
    }

    /// `ContentMatch.parse`: compile an expression over the node types of a schema.
    pub(crate) fn parse(expr: &str, nodes: &[NodeTypeData]) -> Result<Arc<Automaton>> {
        let mut stream = TokenStream::new(expr, nodes);
        if stream.next().is_none() {
            return Ok(Automaton::empty());
        }
        let parsed = parse_expr(&mut stream)?;
        if stream.next().is_some() {
            return Err(stream.err("Unexpected trailing text"));
        }
        let nfa = nfa(&parsed);
        let automaton = dfa(&nfa);
        check_for_dead_ends(&automaton, &stream)?;
        Ok(Arc::new(automaton))
    }

    /// Whether the start state's first edge is an inline node.
    pub(crate) fn inline_content(&self, nodes: &[NodeTypeData]) -> bool {
        self.states[0]
            .next
            .first()
            .is_some_and(|&(node, _)| nodes[node].is_inline())
    }

    /// The state a node of the type at `node` leads to from `state`.
    #[inline]
    fn step(&self, state: usize, node: usize) -> Option<usize> {
        self.states[state]
            .next
            .iter()
            .find(|&&(edge, _)| edge == node)
            .map(|&(_, next)| next)
    }

    /// Whether nodes of these types, in order, are content the expression matches in full.
    /// `None` is a node of another schema's.
    pub(crate) fn accepts(&self, types: impl Iterator<Item = Option<usize>>) -> bool {
        let mut state = 0;
        for node in types {
            match node.and_then(|node| self.step(state, node)) {
                Some(next) => state = next,
                None => return false,
            }
        }
        self.states[state].valid_end
    }
}

/// A content expression compiled on its own, for [`ContentExpr::start`] to match against.
#[derive(Clone)]
pub struct ContentExpr(Arc<Automaton>);

impl ContentExpr {
    /// `ContentMatch.parse(expr, schema.nodes)`: an expression that no node type of the schema
    /// needs to have.
    pub fn parse(schema: &Schema, expr: &str) -> Result<ContentExpr> {
        Ok(ContentExpr(Automaton::parse(expr, &schema.0.nodes)?))
    }

    pub fn start<'s>(&'s self, schema: &'s Schema) -> ContentMatch<'s> {
        ContentMatch::start(schema, &self.0)
    }
}

/// A state of a node type's content expression: what may come next, and whether content may end
/// here.
#[derive(Clone, Copy)]
pub struct ContentMatch<'s> {
    schema: &'s Schema,
    automaton: &'s Automaton,
    state: usize,
}

impl PartialEq for ContentMatch<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state && std::ptr::eq(self.automaton, other.automaton)
    }
}

impl Eq for ContentMatch<'_> {}

impl std::hash::Hash for ContentMatch<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

impl<'s> ContentMatch<'s> {
    pub(crate) fn start(schema: &'s Schema, automaton: &'s Automaton) -> ContentMatch<'s> {
        ContentMatch {
            schema,
            automaton,
            state: 0,
        }
    }

    /// The match at state `state` of the same automaton.
    pub fn at_state(&self, state: usize) -> Option<ContentMatch<'s>> {
        (state < self.automaton.states.len()).then(|| self.at(state))
    }

    /// The match's state in its automaton.
    pub fn state_index(&self) -> usize {
        self.state
    }

    fn state(&self) -> &'s State {
        &self.automaton.states[self.state]
    }

    fn at(&self, state: usize) -> ContentMatch<'s> {
        ContentMatch {
            schema: self.schema,
            automaton: self.automaton,
            state,
        }
    }

    fn node_type(&self, index: usize) -> NodeType<'s> {
        NodeType {
            schema: self.schema,
            index,
        }
    }

    pub fn schema(&self) -> &'s Schema {
        self.schema
    }

    /// Whether this is the start of the empty expression.
    pub fn is_empty(&self) -> bool {
        self.automaton.is_empty_match()
    }

    /// An identity for the match, the same for the same state of the same automaton.
    pub fn id(&self) -> (usize, usize) {
        (
            self.automaton as *const Automaton as *const u8 as usize,
            self.state,
        )
    }

    pub fn valid_end(&self) -> bool {
        self.state().valid_end
    }

    pub fn match_type(&self, node_type: &NodeType) -> Option<ContentMatch<'s>> {
        self.step(self.state, node_type).map(|next| self.at(next))
    }

    fn step(&self, state: usize, node_type: &NodeType) -> Option<usize> {
        if node_type.schema != self.schema {
            return None;
        }
        self.automaton.step(state, node_type.index)
    }

    /// The match after all of `fragment`'s children, `None` where they don't fit.
    pub fn match_fragment(&self, fragment: &Fragment) -> Option<ContentMatch<'s>> {
        let mut state = self.state;
        for child in fragment.children() {
            state = self.step(state, &child.node_type())?;
        }
        Some(self.at(state))
    }

    /// `matchFragment(fragment, start, end)`: the match after the children from index `start` to
    /// `end`. Reading past the last child is ProseMirror's RangeError, where they fit up to it.
    pub fn match_fragment_range(
        &self,
        fragment: &Fragment,
        start: usize,
        end: usize,
    ) -> Result<Option<ContentMatch<'s>>> {
        let mut state = self.state;
        for index in start..end {
            match self.step(state, &fragment.child(index)?.node_type()) {
                Some(next) => state = next,
                None => return Ok(None),
            }
        }
        Ok(Some(self.at(state)))
    }

    pub fn inline_content(&self) -> bool {
        self.state()
            .next
            .first()
            .is_some_and(|&(node, _)| self.schema.0.nodes[node].is_inline())
    }

    /// The first type that can come next and be generated.
    pub fn default_type(&self) -> Option<NodeType<'s>> {
        self.state()
            .next
            .iter()
            .map(|&(node, _)| node)
            .find(|&node| self.schema.0.nodes[node].is_generatable())
            .map(|node| self.node_type(node))
    }

    pub fn compatible(&self, other: &ContentMatch) -> bool {
        self.state().next.iter().any(|&(mine, _)| {
            other
                .state()
                .next
                .iter()
                .any(|&(theirs, _)| mine == theirs && self.schema == other.schema)
        })
    }

    /// The nodes to insert before `after`'s children from `start_index` on for them to match,
    /// and to reach an end when `to_end`. `None` when no nodes make them match.
    pub fn fill_before(
        &self,
        after: &Fragment,
        to_end: bool,
        start_index: usize,
    ) -> Result<Option<Fragment<'static>>> {
        let mut seen = vec![false; self.automaton.states.len()];
        seen[self.state] = true;
        self.search_fill(self, after, to_end, start_index, &mut Vec::new(), &mut seen)
    }

    /// A node of the type, filled, as `fillBefore` makes each node it inserts.
    fn generate(&self, node: usize) -> Result<Node<'static>> {
        thread_local! {
            /// The types being generated on this thread, each inside the one before.
            static GENERATING: RefCell<Vec<(usize, usize)>> = const { RefCell::new(Vec::new()) };
        }
        struct Generating;
        impl Drop for Generating {
            fn drop(&mut self) {
                GENERATING.with_borrow_mut(Vec::pop);
            }
        }
        let key = (self.schema.id(), node);
        // Generating a type inside its own generation generates it the same way again, without
        // end, so ProseMirror recurses until its stack runs out.
        if GENERATING.with_borrow(|generating| generating.contains(&key)) {
            return Err(Error::Range("Maximum call stack size exceeded".into()));
        }
        GENERATING.with_borrow_mut(|generating| generating.push(key));
        let _generating = Generating;
        // A type nothing fills gives ProseMirror a `null` node, whose size the fragment reads.
        let filled = self
            .node_type(node)
            .create_and_fill(None, Fragment::empty(), &[])?;
        js::non_null(filled, "nodeSize")
    }

    fn search_fill(
        &self,
        current: &ContentMatch<'s>,
        after: &Fragment,
        to_end: bool,
        start_index: usize,
        types: &mut Vec<usize>,
        seen: &mut [bool],
    ) -> Result<Option<Fragment<'static>>> {
        let finished = current.match_fragment_range(after, start_index, after.child_count())?;
        if finished.is_some_and(|finished| !to_end || finished.valid_end()) {
            let nodes = types
                .iter()
                .map(|&node| self.generate(node))
                .collect::<Result<Vec<Node>>>()?;
            return Ok(Some(Fragment::from_array(nodes)));
        }
        for &(node, next) in &current.state().next {
            let data = &self.schema.0.nodes[node];
            if data.is_generatable() && !seen[next] {
                seen[next] = true;
                types.push(node);
                let found = stack::grow(|| {
                    self.search_fill(&current.at(next), after, to_end, start_index, types, seen)
                })?;
                types.pop();
                if found.is_some() {
                    return Ok(found);
                }
            }
        }
        Ok(None)
    }

    /// The node types to wrap a node of type `target` in for it to fit here: empty when it fits
    /// as it is, `None` when no wrapping makes it fit.
    pub fn find_wrapping(&self, target: &NodeType) -> Option<Vec<NodeType<'s>>> {
        if target.schema != self.schema {
            return None;
        }
        let key = (self.state, target.index);
        let cached = self
            .automaton
            .wrappings
            .lock()
            .expect("unpoisoned")
            .get(&key)
            .cloned();
        let wrapping = match cached {
            Some(wrapping) => wrapping,
            None => {
                let computed = self.compute_wrapping(target.index).map(Arc::from);
                self.automaton
                    .wrappings
                    .lock()
                    .expect("unpoisoned")
                    .insert(key, computed.clone());
                computed
            }
        };
        wrapping.map(|types| types.iter().map(|&node| self.node_type(node)).collect())
    }

    fn compute_wrapping(&self, target: usize) -> Option<Vec<usize>> {
        struct Active<'s> {
            at: ContentMatch<'s>,
            node: Option<usize>,
            via: Option<usize>,
        }
        let mut seen = vec![false; self.schema.0.nodes.len()];
        let mut visited: Vec<Active> = vec![Active {
            at: *self,
            node: None,
            via: None,
        }];
        let mut queue = VecDeque::from([0]);
        while let Some(current) = queue.pop_front() {
            let at = visited[current].at;
            if at.state().next.iter().any(|&(node, _)| node == target) {
                let mut result = Vec::new();
                let mut walk = Some(current);
                while let Some(index) = walk {
                    let Some(node) = visited[index].node else {
                        break;
                    };
                    result.push(node);
                    walk = visited[index].via;
                }
                result.reverse();
                return Some(result);
            }
            for &(node, next) in &at.state().next {
                let data = &self.schema.0.nodes[node];
                let valid = visited[current].node.is_none() || at.automaton.states[next].valid_end;
                if !data.content.is_empty_match()
                    && !data.attrs.has_required()
                    && !seen[node]
                    && valid
                {
                    visited.push(Active {
                        at: ContentMatch::start(self.schema, &data.content),
                        node: Some(node),
                        via: Some(current),
                    });
                    queue.push_back(visited.len() - 1);
                    seen[node] = true;
                }
            }
        }
        None
    }

    pub fn edge_count(&self) -> usize {
        self.state().next.len()
    }

    /// The `n`th edge out of this state: a node type and the match after it.
    pub fn edge(&self, n: usize) -> Result<(NodeType<'s>, ContentMatch<'s>)> {
        match self.state().next.get(n) {
            Some(&(node, next)) => Ok((self.node_type(node), self.at(next))),
            None => Err(Error::Range(format!(
                "There's no {n}th edge in this content match"
            ))),
        }
    }
}

impl fmt::Display for ContentMatch<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let order = depth_first(self.automaton, self.state);
        let lines: Vec<String> = order
            .iter()
            .enumerate()
            .map(|(i, &state)| {
                let state = &self.automaton.states[state];
                let mut out = format!("{i}{} ", if state.valid_end { "*" } else { " " });
                for (j, &(node, next)) in state.next.iter().enumerate() {
                    if j > 0 {
                        out.push_str(", ");
                    }
                    let position = order.iter().position(|&s| s == next).unwrap_or(0);
                    out.push_str(&format!("{}->{position}", self.schema.0.nodes[node].name));
                }
                out
            })
            .collect();
        f.write_str(&lines.join("\n"))
    }
}

fn depth_first(automaton: &Automaton, start: usize) -> Vec<usize> {
    fn scan(automaton: &Automaton, state: usize, seen: &mut Vec<usize>) {
        seen.push(state);
        for &(_, next) in &automaton.states[state].next {
            if !seen.contains(&next) {
                stack::grow(|| scan(automaton, next, seen));
            }
        }
    }
    let mut seen = Vec::new();
    scan(automaton, start, &mut seen);
    seen
}

impl fmt::Debug for ContentMatch<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ContentMatch({self})")
    }
}

struct TokenStream<'a> {
    string: String,
    nodes: &'a [NodeTypeData],
    tokens: Vec<String>,
    pos: usize,
    inline: Option<bool>,
}

impl<'a> TokenStream<'a> {
    fn new(string: &str, nodes: &'a [NodeTypeData]) -> Self {
        let mut tokens = split_tokens(string);
        if tokens.last().is_some_and(|token| token.is_empty()) {
            tokens.pop();
        }
        if tokens.first().is_some_and(|token| token.is_empty()) {
            tokens.remove(0);
        }
        TokenStream {
            string: string.to_owned(),
            nodes,
            tokens,
            pos: 0,
            inline: None,
        }
    }

    fn next(&self) -> Option<&str> {
        self.tokens.get(self.pos).map(String::as_str)
    }

    /// The next token as JavaScript would write it into a string: `undefined` past the end.
    fn next_display(&self) -> &str {
        self.next().unwrap_or("undefined")
    }

    fn eat(&mut self, token: &str) -> bool {
        if self.next() == Some(token) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn err(&self, message: &str) -> Error {
        Error::Syntax(format!(
            "{message} (in content expression '{}')",
            self.string
        ))
    }
}

/// `string.split(/\s*(?=\b|\W|$)/)`, on the UTF-16 units JavaScript splits.
fn split_tokens(string: &str) -> Vec<String> {
    let units: Vec<u16> = string.encode_utf16().collect();
    let size = units.len();
    let word = |at: usize| units.get(at).is_some_and(|&unit| is_word(unit));
    let ahead = |at: usize| {
        let before = at > 0 && word(at - 1);
        before != word(at) || (at < size && !word(at)) || at == size
    };
    if size == 0 {
        return Vec::new();
    }
    let mut tokens = Vec::new();
    let (mut p, mut q) = (0, 0);
    while q < size {
        let spaces = units[q..]
            .iter()
            .take_while(|&&unit| is_js_space(unit))
            .count();
        let Some(end) = (q..=q + spaces).rev().find(|&end| ahead(end)) else {
            q += 1;
            continue;
        };
        if end == p {
            q += 1;
            continue;
        }
        tokens.push(String::from_utf16_lossy(&units[p..q]));
        p = end;
        q = p;
    }
    tokens.push(String::from_utf16_lossy(&units[p..]));
    tokens
}

fn is_word(unit: u16) -> bool {
    matches!(unit, 0x30..=0x39 | 0x41..=0x5A | 0x5F | 0x61..=0x7A)
}

fn has_non_word(token: &str) -> bool {
    token.encode_utf16().any(|unit| !is_word(unit))
}

enum Expr {
    Choice(Vec<Expr>),
    Seq(Vec<Expr>),
    Plus(Box<Expr>),
    Star(Box<Expr>),
    Opt(Box<Expr>),
    /// `max` is `None` for an open range.
    Range(usize, Option<usize>, Box<Expr>),
    Name(usize),
}

impl Drop for Expr {
    /// An expression nests as deeply as its parentheses and repeats do.
    fn drop(&mut self) {
        let inner = match self {
            Expr::Choice(exprs) | Expr::Seq(exprs) => std::mem::take(exprs),
            Expr::Plus(expr) | Expr::Star(expr) | Expr::Opt(expr) | Expr::Range(_, _, expr) => {
                vec![std::mem::replace(&mut **expr, Expr::Name(0))]
            }
            Expr::Name(_) => return,
        };
        stack::grow(|| drop(inner));
    }
}

fn parse_expr(stream: &mut TokenStream) -> Result<Expr> {
    let mut exprs = Vec::new();
    loop {
        exprs.push(parse_expr_seq(stream)?);
        if !stream.eat("|") {
            break;
        }
    }
    Ok(if exprs.len() == 1 {
        exprs.pop().expect("one")
    } else {
        Expr::Choice(exprs)
    })
}

fn parse_expr_seq(stream: &mut TokenStream) -> Result<Expr> {
    let mut exprs = Vec::new();
    loop {
        exprs.push(parse_expr_subscript(stream)?);
        match stream.next() {
            Some(next) if !next.is_empty() && next != ")" && next != "|" => {}
            _ => break,
        }
    }
    Ok(if exprs.len() == 1 {
        exprs.pop().expect("one")
    } else {
        Expr::Seq(exprs)
    })
}

fn parse_expr_subscript(stream: &mut TokenStream) -> Result<Expr> {
    let mut expr = parse_expr_atom(stream)?;
    loop {
        if stream.eat("+") {
            expr = Expr::Plus(Box::new(expr));
        } else if stream.eat("*") {
            expr = Expr::Star(Box::new(expr));
        } else if stream.eat("?") {
            expr = Expr::Opt(Box::new(expr));
        } else if stream.eat("{") {
            expr = parse_expr_range(stream, expr)?;
        } else {
            return Ok(expr);
        }
    }
}

fn parse_num(stream: &mut TokenStream) -> Result<usize> {
    let next = stream.next_display();
    if next.chars().any(|c| !c.is_ascii_digit()) {
        return Err(stream.err(&format!("Expected number, got '{next}'")));
    }
    let number = next.parse().unwrap_or(0);
    stream.pos += 1;
    Ok(number)
}

fn parse_expr_range(stream: &mut TokenStream, expr: Expr) -> Result<Expr> {
    let min = parse_num(stream)?;
    let mut max = Some(min);
    if stream.eat(",") {
        max = if stream.next() != Some("}") {
            Some(parse_num(stream)?)
        } else {
            None
        };
    }
    if !stream.eat("}") {
        return Err(stream.err("Unclosed braced range"));
    }
    Ok(Expr::Range(min, max, Box::new(expr)))
}

fn resolve_name(stream: &TokenStream, name: &str) -> Result<Vec<usize>> {
    if let Some(index) = stream.nodes.iter().position(|node| &*node.name == name) {
        return Ok(vec![index]);
    }
    let result: Vec<usize> = stream
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.groups.iter().any(|group| group == name))
        .map(|(index, _)| index)
        .collect();
    if result.is_empty() {
        return Err(stream.err(&format!("No node type or group '{name}' found")));
    }
    Ok(result)
}

fn parse_expr_atom(stream: &mut TokenStream) -> Result<Expr> {
    if stream.eat("(") {
        let expr = stack::grow(|| parse_expr(stream))?;
        if !stream.eat(")") {
            return Err(stream.err("Missing closing paren"));
        }
        return Ok(expr);
    }
    let next = stream.next_display().to_owned();
    if has_non_word(&next) {
        return Err(stream.err(&format!("Unexpected token '{next}'")));
    }
    let mut exprs = Vec::new();
    for node in resolve_name(stream, &next)? {
        let inline = stream.nodes[node].is_inline();
        match stream.inline {
            None => stream.inline = Some(inline),
            Some(current) if current != inline => {
                return Err(stream.err("Mixing inline and block content"));
            }
            Some(_) => {}
        }
        exprs.push(Expr::Name(node));
    }
    stream.pos += 1;
    Ok(if exprs.len() == 1 {
        exprs.pop().expect("one")
    } else {
        Expr::Choice(exprs)
    })
}

#[derive(Clone, Copy)]
struct Edge {
    term: Option<usize>,
    to: Option<usize>,
}

/// A dangling edge: a state and the index of an edge out of it whose target is still unset.
type EdgeRef = (usize, usize);

struct Nfa {
    states: Vec<Vec<Edge>>,
}

impl Nfa {
    fn node(&mut self) -> usize {
        self.states.push(Vec::new());
        self.states.len() - 1
    }

    fn edge(&mut self, from: usize, to: Option<usize>, term: Option<usize>) -> EdgeRef {
        self.states[from].push(Edge { term, to });
        (from, self.states[from].len() - 1)
    }

    fn connect(&mut self, edges: &[EdgeRef], to: usize) {
        for &(state, edge) in edges {
            self.states[state][edge].to = Some(to);
        }
    }

    fn compile(&mut self, expr: &Expr, from: usize) -> Vec<EdgeRef> {
        stack::grow(|| match expr {
            Expr::Choice(exprs) => {
                let mut out = Vec::new();
                for expr in exprs {
                    out.extend(self.compile(expr, from));
                }
                out
            }
            Expr::Seq(exprs) => {
                let mut from = from;
                for (i, expr) in exprs.iter().enumerate() {
                    let next = self.compile(expr, from);
                    if i == exprs.len() - 1 {
                        return next;
                    }
                    from = self.node();
                    self.connect(&next, from);
                }
                unreachable!("a sequence has expressions")
            }
            Expr::Star(expr) => {
                let looped = self.node();
                self.edge(from, Some(looped), None);
                let inner = self.compile(expr, looped);
                self.connect(&inner, looped);
                vec![self.edge(looped, None, None)]
            }
            Expr::Plus(expr) => {
                let looped = self.node();
                let first = self.compile(expr, from);
                self.connect(&first, looped);
                let again = self.compile(expr, looped);
                self.connect(&again, looped);
                vec![self.edge(looped, None, None)]
            }
            Expr::Opt(expr) => {
                let mut out = vec![self.edge(from, None, None)];
                out.extend(self.compile(expr, from));
                out
            }
            Expr::Range(min, max, expr) => {
                let mut current = from;
                for _ in 0..*min {
                    let next = self.node();
                    let inner = self.compile(expr, current);
                    self.connect(&inner, next);
                    current = next;
                }
                match max {
                    None => {
                        let inner = self.compile(expr, current);
                        self.connect(&inner, current);
                    }
                    Some(max) => {
                        for _ in *min..*max {
                            let next = self.node();
                            self.edge(current, Some(next), None);
                            let inner = self.compile(expr, current);
                            self.connect(&inner, next);
                            current = next;
                        }
                    }
                }
                vec![self.edge(current, None, None)]
            }
            Expr::Name(node) => vec![self.edge(from, None, Some(*node))],
        })
    }
}

fn nfa(expr: &Expr) -> Nfa {
    let mut nfa = Nfa {
        states: vec![Vec::new()],
    };
    let edges = nfa.compile(expr, 0);
    let end = nfa.node();
    nfa.connect(&edges, end);
    nfa
}

/// The states reachable from `node` over null edges, but for those with a single null edge out,
/// in descending order.
fn null_from(nfa: &Nfa, node: usize) -> Vec<usize> {
    /// `pushed` holds what `result` does, to look up in constant time.
    fn scan(nfa: &Nfa, node: usize, result: &mut Vec<usize>, pushed: &mut HashSet<usize>) {
        let edges = &nfa.states[node];
        if edges.len() == 1 && edges[0].term.is_none() {
            let to = edges[0].to.expect("connected");
            return stack::grow(|| scan(nfa, to, result, pushed));
        }
        result.push(node);
        pushed.insert(node);
        for edge in edges {
            let to = edge.to.expect("connected");
            if edge.term.is_none() && !pushed.contains(&to) {
                stack::grow(|| scan(nfa, to, result, pushed));
            }
        }
    }
    let mut result = Vec::new();
    scan(nfa, node, &mut result, &mut HashSet::new());
    result.sort_by(|a, b| b.cmp(a));
    result
}

fn dfa(nfa: &Nfa) -> Automaton {
    fn explore(
        nfa: &Nfa,
        states: &[usize],
        labeled: &mut HashMap<Vec<usize>, usize>,
        out_states: &mut Vec<State>,
    ) -> usize {
        let mut out: Vec<(usize, Vec<usize>)> = Vec::new();
        for &node in states {
            for edge in &nfa.states[node] {
                let Some(term) = edge.term else { continue };
                let reached = null_from(nfa, edge.to.expect("connected"));
                match out.iter_mut().rfind(|(t, _)| *t == term) {
                    Some((_, set)) => set.extend(reached),
                    None if !reached.is_empty() => out.push((term, reached)),
                    None => {}
                }
            }
        }
        let index = out_states.len();
        out_states.push(State {
            valid_end: states.contains(&(nfa.states.len() - 1)),
            next: Vec::new(),
        });
        labeled.insert(states.to_vec(), index);
        for (term, mut set) in out {
            set.sort_by(|a, b| b.cmp(a));
            set.dedup();
            let next = match labeled.get(&set) {
                Some(&next) => next,
                None => stack::grow(|| explore(nfa, &set, labeled, out_states)),
            };
            out_states[index].next.push((term, next));
        }
        index
    }

    let mut states = Vec::new();
    explore(nfa, &null_from(nfa, 0), &mut HashMap::new(), &mut states);
    Automaton {
        states,
        wrappings: Mutex::default(),
    }
}

fn check_for_dead_ends(automaton: &Automaton, stream: &TokenStream) -> Result<()> {
    let mut work = vec![0];
    let mut queued = vec![false; automaton.states.len()];
    queued[0] = true;
    let mut i = 0;
    while i < work.len() {
        let state = &automaton.states[work[i]];
        let mut dead = !state.valid_end;
        let mut names = Vec::new();
        for &(node, next) in &state.next {
            let data = &stream.nodes[node];
            names.push(data.name.to_string());
            if dead && data.is_generatable() {
                dead = false;
            }
            if !queued[next] {
                queued[next] = true;
                work.push(next);
            }
        }
        if dead {
            return Err(stream.err(&format!(
                "Only non-generatable nodes ({}) in a required position (see https://prosemirror.net/docs/guide/#generatable)",
                names.join(", ")
            )));
        }
        i += 1;
    }
    Ok(())
}
