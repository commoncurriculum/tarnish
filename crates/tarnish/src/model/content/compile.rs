//! Compiling a content expression: its tokens, parsed, then an NFA, and the DFA of that, as
//! ProseMirror builds them.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use super::{Automaton, State};
use crate::error::{Error, Result};
use crate::model::schema::NodeTypeData;
use crate::stack;
use crate::text::is_js_space;

impl Automaton {
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
