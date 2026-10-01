//! Finds recursion that nothing guards: a function that calls itself, or a cycle of functions in
//! one file calling each other, with no call on the way inside a `grow(..)` or `maybe_grow(..)`
//! closure. Input nests as deeply as a document can, so each such recursion has to make room on
//! the stack at some level, or say why its depth has a bound with a `// bounded: <reason>`
//! comment above the function, or above the `impl` or trait that holds it. A recursion that
//! makes room where this can't see it, as in a function it hands a closure to, says so with a
//! `// guarded: <reason>` comment.
//!
//! It reads the syntax alone, so it knows the type of a method call's receiver only as far as
//! the syntax says: `self`; a parameter's or a `let`'s type; a struct literal or `Type::new(..)`;
//! a field of `self` whose type the file declares; an item of one of those; and what a `match`
//! binds from an enum the file declares. What is got from `self` by a method call, or from a
//! field whose type isn't the caller's own, is taken to be another type that `self` holds, whose
//! method of the same name the caller hands the call to. A value of no type it knows is taken to
//! be the caller's own type's. Recursion through a trait's generic or derived implementation
//! (`Vec<T>: Clone` calling `T::clone`), through a function in another file, and through a macro
//! whose arguments aren't expressions, isn't seen.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};

use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Attribute, Block, Expr, Pat, Signature, Token, Type};

/// Recursion with no guard: the functions of one cycle, each `Owner::name` or `name`, with the
/// line of its `fn`.
#[derive(Debug)]
pub struct Unguarded {
    pub file: PathBuf,
    pub cycle: Vec<(String, usize)>,
}

impl fmt::Display for Unguarded {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        let functions: Vec<String> = self
            .cycle
            .iter()
            .map(|(name, line)| format!("{name} (line {line})"))
            .collect();
        write!(
            formatter,
            "{}: {}",
            self.file.display(),
            functions.join(" -> ")
        )
    }
}

/// The unguarded recursion in each `.rs` file under `dir`, skipping `target`, `vendor`,
/// `node_modules` and hidden directories. Panics on a file that doesn't parse.
pub fn unguarded_recursion(dir: &Path) -> Vec<Unguarded> {
    let mut files = Vec::new();
    rust_files(dir, &mut files);
    files.sort();
    files
        .iter()
        .flat_map(|file| {
            let source =
                std::fs::read_to_string(file).unwrap_or_else(|error| panic!("{file:?}: {error}"));
            in_source(file, &source)
        })
        .collect()
}

// bounded: as deep as the directory tree
fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|error| panic!("{dir:?}: {error}"));
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if path.is_dir() {
            if !name.starts_with('.') && !["target", "vendor", "node_modules"].contains(&name) {
                rust_files(&path, files);
            }
        } else if name.ends_with(".rs") {
            files.push(path);
        }
    }
}

fn in_source(file: &Path, source: &str) -> Vec<Unguarded> {
    let syntax = syn::parse_file(source).unwrap_or_else(|error| panic!("{file:?}: {error}"));
    let mut items = Items {
        lines: source.lines().collect(),
        functions: Vec::new(),
        types: HashMap::new(),
        scopes: Vec::new(),
    };
    items.visit_file(&syntax);

    // A bounded function calls nothing here, so it is in no cycle.
    let functions = &items.functions;
    let mut calls: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); functions.len()];
    for (index, caller) in functions.iter().enumerate() {
        if caller.bounded {
            continue;
        }
        let mut finder = Calls {
            caller,
            functions,
            types: &items.types,
            bindings: HashMap::new(),
            closure: None,
            calls: Vec::new(),
            grown: 0,
        };
        for input in &caller.sig.inputs {
            if let syn::FnArg::Typed(typed) = input {
                let class = finder.of_type(&typed.ty);
                finder.bind(&typed.pat, class);
            }
        }
        finder.visit_block(caller.body);
        for callee in finder.calls {
            calls[index].extend(resolve(&callee, index, functions));
        }
    }

    cycles(&calls)
        .into_iter()
        .map(|cycle| Unguarded {
            file: file.to_owned(),
            cycle: cycle
                .into_iter()
                .map(|index| (functions[index].qualified(), functions[index].line))
                .collect(),
        })
        .collect()
}

/// A function: its owner, the type of its `impl` or its trait, if it has one.
struct Function<'a> {
    owner: Option<String>,
    kind: ScopeKind,
    name: String,
    /// The line of `fn`.
    line: usize,
    bounded: bool,
    sig: &'a Signature,
    body: &'a Block,
}

impl Function<'_> {
    fn qualified(&self) -> String {
        match &self.owner {
            Some(owner) => format!("{owner}::{}", self.name),
            None => self.name.clone(),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ScopeKind {
    /// A function's body, or the file: what's declared here is free.
    Free,
    Inherent,
    TraitImpl,
    Trait,
}

/// An `impl` or trait, or a function's body.
struct Scope {
    owner: Option<String>,
    kind: ScopeKind,
    bounded: bool,
}

/// The fields of each struct, and of each variant of each enum, with the names in each field's
/// type: `"Struct"` or `"Enum::Variant"`, then the fields by name, or by index for a tuple's.
type Types = HashMap<String, Vec<(String, BTreeSet<String>)>>;

struct Items<'a> {
    lines: Vec<&'a str>,
    functions: Vec<Function<'a>>,
    types: Types,
    scopes: Vec<Scope>,
}

impl<'a> Items<'a> {
    /// Whether a `// bounded: <reason>` or `// guarded: <reason>` comment is among the
    /// attributes of an item whose keyword is on `line`, or in the comment lines right above them.
    fn marked_bounded(&self, attrs: &[Attribute], line: usize) -> bool {
        let start = attrs
            .iter()
            .map(|attr| attr.span().start().line)
            .min()
            .unwrap_or(line);
        let is_bounded = |line: &&str| {
            let line = line.trim();
            let reason = line
                .strip_prefix("// bounded:")
                .or_else(|| line.strip_prefix("// guarded:"));
            reason.is_some_and(|reason| !reason.trim().is_empty())
        };
        let above = self.lines[..start - 1]
            .iter()
            .rev()
            .take_while(|line| line.trim().starts_with("//"));
        self.lines[start - 1..line]
            .iter()
            .chain(above)
            .any(is_bounded)
    }

    fn function(&mut self, attrs: &[Attribute], sig: &'a Signature, body: &'a Block) {
        let line = sig.fn_token.span.start().line;
        let (owner, kind, scope_bounded) = match self.scopes.last() {
            Some(scope) => (scope.owner.clone(), scope.kind, scope.bounded),
            None => (None, ScopeKind::Free, false),
        };
        self.functions.push(Function {
            owner,
            kind,
            name: sig.ident.to_string(),
            line,
            bounded: scope_bounded || self.marked_bounded(attrs, line),
            sig,
            body,
        });
        self.within(
            Scope {
                owner: None,
                kind: ScopeKind::Free,
                bounded: false,
            },
            |items| items.visit_block(body),
        );
    }

    fn within(&mut self, scope: Scope, visit: impl FnOnce(&mut Self)) {
        self.scopes.push(scope);
        visit(self);
        self.scopes.pop();
    }

    fn fields(&mut self, name: String, fields: &syn::Fields) {
        let fields = fields.iter().enumerate().map(|(index, field)| {
            let name = field
                .ident
                .as_ref()
                .map_or(index.to_string(), |n| n.to_string());
            (name, names_in(&field.ty))
        });
        self.types.insert(name, fields.collect());
    }
}

impl<'a> Visit<'a> for Items<'a> {
    fn visit_item_fn(&mut self, item: &'a syn::ItemFn) {
        self.function(&item.attrs, &item.sig, &item.block);
    }

    fn visit_impl_item_fn(&mut self, item: &'a syn::ImplItemFn) {
        self.function(&item.attrs, &item.sig, &item.block);
    }

    fn visit_trait_item_fn(&mut self, item: &'a syn::TraitItemFn) {
        if let Some(body) = &item.default {
            self.function(&item.attrs, &item.sig, body);
        }
    }

    fn visit_item_impl(&mut self, item: &'a syn::ItemImpl) {
        let line = item.impl_token.span.start().line;
        // An implementation for a type parameter is one of its own: `impl<T> Trait for &T`.
        let owner = type_name(&item.self_ty).map(|name| {
            let generic = item.generics.type_params().any(|param| param.ident == name);
            if generic {
                format!("{name}@{line}")
            } else {
                name
            }
        });
        let scope = Scope {
            owner,
            kind: match item.trait_ {
                None => ScopeKind::Inherent,
                Some(_) => ScopeKind::TraitImpl,
            },
            bounded: self.marked_bounded(&item.attrs, line),
        };
        self.within(scope, |items| visit::visit_item_impl(items, item));
    }

    fn visit_item_trait(&mut self, item: &'a syn::ItemTrait) {
        let scope = Scope {
            owner: Some(item.ident.to_string()),
            kind: ScopeKind::Trait,
            bounded: self.marked_bounded(&item.attrs, item.trait_token.span.start().line),
        };
        self.within(scope, |items| visit::visit_item_trait(items, item));
    }

    fn visit_item_struct(&mut self, item: &'a syn::ItemStruct) {
        self.fields(item.ident.to_string(), &item.fields);
        visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'a syn::ItemEnum) {
        for variant in &item.variants {
            self.fields(
                format!("{}::{}", item.ident, variant.ident),
                &variant.fields,
            );
        }
        visit::visit_item_enum(self, item);
    }
}

/// The name of a type, through references and the pointers that hand their methods on.
// bounded: as deep as a type nests in a source file
fn type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Reference(reference) => type_name(&reference.elem),
        Type::Paren(inner) => type_name(&inner.elem),
        Type::Path(path) => {
            let last = path.path.segments.last()?;
            let pointer = ["Box", "Rc", "Arc"].iter().any(|name| last.ident == name);
            match &last.arguments {
                syn::PathArguments::AngleBracketed(arguments) if pointer => {
                    arguments.args.iter().find_map(|argument| match argument {
                        syn::GenericArgument::Type(inner) => type_name(inner),
                        _ => None,
                    })
                }
                _ => Some(last.ident.to_string()),
            }
        }
        _ => None,
    }
}

/// The types a type names that may be declared in this file: those it names with no module, or
/// with `crate`, `self` or `super`, and those among its type arguments.
fn names_in(ty: &Type) -> BTreeSet<String> {
    struct Names(BTreeSet<String>);
    impl<'a> Visit<'a> for Names {
        fn visit_path(&mut self, path: &'a syn::Path) {
            let local = path.segments.len() == 1
                || ["crate", "self", "super"]
                    .iter()
                    .any(|m| path.segments[0].ident == m);
            if local && let Some(last) = path.segments.last() {
                self.0.insert(last.ident.to_string());
            }
            visit::visit_path(self, path);
        }
    }
    let mut names = Names(BTreeSet::new());
    names.visit_type(ty);
    names.0
}

fn names_bound(pat: &Pat) -> Vec<String> {
    struct Names(Vec<String>);
    impl<'a> Visit<'a> for Names {
        fn visit_pat_ident(&mut self, ident: &'a syn::PatIdent) {
            self.0.push(ident.ident.to_string());
            visit::visit_pat_ident(self, ident);
        }
    }
    let mut names = Names(Vec::new());
    names.visit_pat(pat);
    names.0
}

/// What a value is, as far as the syntax says.
#[derive(Clone)]
enum Class {
    Of(String),
    /// Another type than the caller's: one that `self` holds, or one the syntax gives that has
    /// no name here.
    Held,
    /// An item of something, or a closure's parameter: taken to be the caller's own type.
    Item,
    /// What the syntax says nothing of.
    Unknown,
}

/// What a call may call: a free function, or a type's function or method, with whether it is
/// called on the caller's own `self` or by its path.
enum Callee {
    Free(String),
    Of {
        owner: String,
        name: String,
        direct: bool,
    },
}

/// The iterator and option adapters, whose closure's parameter is an item of what they're
/// called on.
const ADAPTERS: &[&str] = &[
    "all",
    "and_then",
    "any",
    "filter",
    "filter_map",
    "find",
    "find_map",
    "flat_map",
    "fold",
    "for_each",
    "inspect",
    "is_some_and",
    "map",
    "map_or",
    "map_or_else",
    "map_while",
    "max_by_key",
    "min_by_key",
    "partition",
    "position",
    "rposition",
    "scan",
    "skip_while",
    "take_while",
    "try_fold",
    "try_for_each",
];

struct Calls<'a, 'f> {
    caller: &'f Function<'a>,
    functions: &'f [Function<'a>],
    types: &'f Types,
    bindings: HashMap<String, Class>,
    /// What the parameters of the closure visited next are, when the call handing it says: one
    /// class for each, in order.
    closure: Option<Vec<Class>>,
    calls: Vec<Callee>,
    /// How many `grow` closures the expression visited is in.
    grown: usize,
}

impl Calls<'_, '_> {
    fn push(&mut self, callee: Callee) {
        if self.grown == 0 {
            self.calls.push(callee);
        }
    }

    /// The classes of the parameters of the closure handed as argument `arg` to `owner`'s method
    /// `name`, when the file declares that method with an `Fn` bound on the parameter.
    fn closure_inputs(&self, owner: &str, name: &str, arg: usize) -> Option<Vec<Class>> {
        let function = self
            .functions
            .iter()
            .find(|function| function.name == name && function.owner.as_deref() == Some(owner))?;
        let sig = function.sig;
        let offset = usize::from(sig.receiver().is_some());
        let syn::FnArg::Typed(typed) = sig.inputs.iter().nth(arg + offset)? else {
            return None;
        };
        let mut bounds: Vec<&syn::TypeParamBound> = Vec::new();
        match &*typed.ty {
            Type::ImplTrait(bound) => bounds.extend(&bound.bounds),
            Type::Path(path) => {
                let parameter = path.path.get_ident()?;
                for param in sig.generics.type_params() {
                    if param.ident == *parameter {
                        bounds.extend(&param.bounds);
                    }
                }
                let predicates = sig.generics.where_clause.iter().flat_map(|w| &w.predicates);
                for predicate in predicates {
                    if let syn::WherePredicate::Type(predicate) = predicate
                        && type_name(&predicate.bounded_ty).as_deref()
                            == Some(&*parameter.to_string())
                    {
                        bounds.extend(&predicate.bounds);
                    }
                }
            }
            _ => return None,
        }
        bounds.into_iter().find_map(|bound| {
            let syn::TypeParamBound::Trait(bound) = bound else {
                return None;
            };
            let last = bound.path.segments.last()?;
            let syn::PathArguments::Parenthesized(inputs) = &last.arguments else {
                return None;
            };
            let classes = inputs.inputs.iter().map(|ty| match type_name(ty) {
                Some(name) if name == "Self" => Class::Of(owner.to_owned()),
                Some(name) => Class::Of(name),
                None => Class::Held,
            });
            Some(classes.collect())
        })
    }

    fn own(&self) -> Class {
        self.caller.owner.clone().map_or(Class::Held, Class::Of)
    }

    fn is_own(&self, class: &Class) -> bool {
        matches!(class, Class::Of(of) if self.caller.owner.as_ref() == Some(of))
    }

    fn of_name(&self, name: String) -> Class {
        match name.as_str() {
            "Self" => self.own(),
            _ => Class::Of(name),
        }
    }

    /// A type the syntax gives but that has no name here, such as `impl Trait` or a slice, is
    /// taken to be another type.
    fn of_type(&self, ty: &Type) -> Class {
        type_name(ty).map_or(Class::Held, |name| self.of_name(name))
    }

    /// The class of a field of a value of type `of`: the caller's own type if the field's type
    /// names it, and another type if not.
    fn field(&self, of: &str, field: &str) -> Class {
        let owner = self.caller.owner.as_deref().unwrap_or("Self");
        let holds_own = self
            .types
            .get(of)
            .and_then(|fields| fields.iter().find(|(name, _)| name == field))
            .is_some_and(|(_, names)| names.contains(owner) || names.contains("Self"));
        match holds_own {
            true => self.own(),
            false => Class::Held,
        }
    }

    // bounded: as deep as type ascriptions nest in a pattern in a source file
    fn bind(&mut self, pat: &Pat, class: Class) {
        match pat {
            Pat::Type(typed) => {
                let class = self.of_type(&typed.ty);
                self.bind(&typed.pat, class);
            }
            _ => {
                for name in names_bound(pat) {
                    self.bindings.insert(name, class.clone());
                }
            }
        }
    }

    /// What an expression is, or with `items`, what an item of it is. What a method call gives
    /// is taken to be another type, but its items to be the caller's own, as children are.
    fn class(&self, expr: &Expr, items: bool) -> Class {
        let expr = unwrapped(expr);
        let class = match expr {
            Expr::Struct(literal) => match literal.path.segments.last() {
                Some(last) => self.of_name(last.ident.to_string()),
                None => Class::Unknown,
            },
            Expr::Call(call) => match &*call.func {
                Expr::Path(path) if path.path.segments.len() > 1 => {
                    let segments = &path.path.segments;
                    let owner = segments[segments.len() - 2].ident.to_string();
                    match owner.starts_with(char::is_uppercase) {
                        true => self.of_name(owner),
                        false => Class::Unknown,
                    }
                }
                _ => Class::Unknown,
            },
            _ => match chain(expr) {
                Some((root, steps)) => {
                    let mut class = match root.as_str() {
                        "self" => self.own(),
                        variable => self.bindings.get(variable).cloned().unwrap_or(Class::Item),
                    };
                    for step in &steps {
                        class = match (step, class) {
                            (Step::Field(field), Class::Of(of)) => self.field(&of, field),
                            (Step::Same, class) => class,
                            (Step::Field(_) | Step::Call, _) => Class::Held,
                        };
                    }
                    let last = steps.iter().rev().find(|step| !matches!(step, Step::Same));
                    let called = matches!(last, Some(Step::Call));
                    match (items, class) {
                        (true, Class::Held) if called => Class::Item,
                        (true, Class::Of(_)) => Class::Item,
                        (_, class) => class,
                    }
                }
                None => Class::Unknown,
            },
        };
        match (items, class) {
            (true, Class::Unknown) => Class::Item,
            (_, class) => class,
        }
    }

    /// Binds what a pattern matched against `scrutinee` binds.
    // bounded: as deep as a tuple nests in a source file
    fn bind_matched(&mut self, pat: &Pat, scrutinee: &Expr) {
        let pat = unwrapped_pat(pat);
        if let (Expr::Tuple(tuple), Pat::Tuple(pats)) = (unwrapped(scrutinee), pat)
            && tuple.elems.len() == pats.elems.len()
        {
            for (pat, scrutinee) in pats.elems.iter().zip(&tuple.elems) {
                self.bind_matched(pat, scrutinee);
            }
            return;
        }
        let class = self.class(scrutinee, false);
        if !self.is_own(&class) {
            return self.bind(pat, class);
        }
        for (name, class) in self.variant_fields(pat, &class) {
            self.bindings.insert(name, class);
        }
    }

    /// The names a pattern on a value of the caller's own type binds, each with its class: what
    /// the field of the enum variant or struct matched holds, or for `Some`, `Ok` and `Err`, the
    /// value matched.
    // bounded: as deep as a pattern nests in a source file
    fn variant_fields(&self, pat: &Pat, matched: &Class) -> Vec<(String, Class)> {
        let path_of = |path: &syn::Path| {
            let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
            let owner = self.caller.owner.clone().unwrap_or_default();
            match segments.as_slice() {
                [.., ty, variant] => {
                    let ty = if ty == "Self" { &owner } else { ty };
                    format!("{ty}::{variant}")
                }
                [name] if name == "Self" => owner,
                [name] => name.clone(),
                [] => String::new(),
            }
        };
        let fields = |of: String, fields: Vec<(String, &Pat)>| -> Vec<(String, Class)> {
            let wrapper = ["Some", "Ok", "Err"].contains(&of.as_str());
            fields
                .into_iter()
                .flat_map(|(field, pat)| {
                    let class = match wrapper {
                        true => matched.clone(),
                        false => self.field(&of, &field),
                    };
                    names_bound(pat)
                        .into_iter()
                        .map(move |name| (name, class.clone()))
                })
                .collect()
        };
        match unwrapped_pat(pat) {
            Pat::TupleStruct(tuple) => {
                let elems = tuple.elems.iter().enumerate();
                fields(
                    path_of(&tuple.path),
                    elems
                        .map(|(index, elem)| (index.to_string(), elem))
                        .collect(),
                )
            }
            Pat::Struct(record) => {
                let members = record.fields.iter().map(|field| {
                    let name = match &field.member {
                        syn::Member::Named(name) => name.to_string(),
                        syn::Member::Unnamed(index) => index.index.to_string(),
                    };
                    (name, &*field.pat)
                });
                fields(path_of(&record.path), members.collect())
            }
            Pat::Or(or) => or
                .cases
                .iter()
                .flat_map(|case| self.variant_fields(case, matched))
                .collect(),
            other => names_bound(other)
                .into_iter()
                .map(|name| (name, matched.clone()))
                .collect(),
        }
    }
}

// bounded: as deep as a pattern nests in a source file
fn unwrapped_pat(pat: &Pat) -> &Pat {
    match pat {
        Pat::Reference(reference) => unwrapped_pat(&reference.pat),
        Pat::Paren(paren) => unwrapped_pat(&paren.pat),
        other => other,
    }
}

impl<'a> Visit<'a> for Calls<'_, '_> {
    // A function declared in the body is a function of its own.
    fn visit_item(&mut self, _: &'a syn::Item) {}

    fn visit_local(&mut self, local: &'a syn::Local) {
        let Some(init) = &local.init else {
            return self.bind(&local.pat, Class::Unknown);
        };
        self.visit_expr(&init.expr);
        if let Some((_, diverge)) = &init.diverge {
            self.visit_expr(diverge);
        }
        match &local.pat {
            Pat::Ident(_) | Pat::Type(_) => {
                let class = self.class(&init.expr, false);
                self.bind(&local.pat, class);
            }
            pat => self.bind_matched(pat, &init.expr),
        }
    }

    fn visit_expr_closure(&mut self, closure: &'a syn::ExprClosure) {
        let classes = self.closure.take().unwrap_or_default();
        for (index, input) in closure.inputs.iter().enumerate() {
            let class = match classes.as_slice() {
                [class] => class.clone(),
                classes => classes.get(index).cloned().unwrap_or(Class::Item),
            };
            self.bind(input, class);
        }
        self.visit_expr(&closure.body);
    }

    fn visit_expr_for_loop(&mut self, for_loop: &'a syn::ExprForLoop) {
        self.visit_expr(&for_loop.expr);
        let class = self.class(&for_loop.expr, true);
        self.bind(&for_loop.pat, class);
        self.visit_block(&for_loop.body);
    }

    fn visit_expr_match(&mut self, matched: &'a syn::ExprMatch) {
        self.visit_expr(&matched.expr);
        for arm in &matched.arms {
            self.bind_matched(&arm.pat, &matched.expr);
            if let Some((_, guard)) = &arm.guard {
                self.visit_expr(guard);
            }
            self.visit_expr(&arm.body);
        }
    }

    fn visit_expr_let(&mut self, binding: &'a syn::ExprLet) {
        self.visit_expr(&binding.expr);
        self.bind_matched(&binding.pat, &binding.expr);
    }

    fn visit_expr_call(&mut self, call: &'a syn::ExprCall) {
        let mut grows = false;
        if let Expr::Path(path) = &*call.func {
            let segments: Vec<String> = path
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            match segments.as_slice() {
                // One segment may be a variable, so it counts only as a call's callee, and only
                // when no variable here has the name.
                [name] if !self.bindings.contains_key(name) => {
                    self.push(Callee::Free(name.clone()))
                }
                [module, name] if module == "self" => self.push(Callee::Free(name.clone())),
                _ => {}
            }
            grows = segments
                .last()
                .is_some_and(|name| name == "grow" || name == "maybe_grow");
        }
        self.visit_expr(&call.func);
        self.grown += usize::from(grows);
        for arg in &call.args {
            self.visit_expr(arg);
        }
        self.grown -= usize::from(grows);
    }

    fn visit_expr_path(&mut self, path: &'a syn::ExprPath) {
        let segments: Vec<String> = path
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        if let [.., owner, name] = segments.as_slice() {
            let owner = match owner.as_str() {
                "Self" => self.caller.owner.clone(),
                owner if owner.starts_with(char::is_uppercase) => Some(owner.to_owned()),
                _ => None,
            };
            if let Some(owner) = owner {
                let name = name.clone();
                self.push(Callee::Of {
                    owner,
                    name,
                    direct: true,
                });
            }
        }
        visit::visit_expr_path(self, path);
    }

    fn visit_expr_method_call(&mut self, call: &'a syn::ExprMethodCall) {
        let name = call.method.to_string();
        let receiver = unwrapped(&call.receiver);
        let on_self = matches!(receiver, Expr::Path(path) if path.path.is_ident("self"));
        let owner = match self.class(receiver, false) {
            Class::Of(owner) => Some(owner),
            Class::Item => self.caller.owner.clone(),
            Class::Held | Class::Unknown => None,
        };
        if let Some(owner) = &owner {
            self.push(Callee::Of {
                owner: owner.clone(),
                name: name.clone(),
                direct: on_self,
            });
        }

        self.visit_expr(&call.receiver);
        // A closure handed to a method of `self` is most often handed what `self` holds.
        let items = match ADAPTERS.contains(&name.as_str()) {
            true => self.class(receiver, true),
            false if from_self(receiver) => Class::Held,
            false => Class::Item,
        };
        for (index, arg) in call.args.iter().enumerate() {
            if let Expr::Closure(_) = arg {
                let declared = owner
                    .as_deref()
                    .and_then(|owner| self.closure_inputs(owner, &name, index));
                self.closure = Some(declared.unwrap_or_else(|| vec![items.clone()]));
            }
            self.visit_expr(arg);
            self.closure = None;
        }
    }

    // A macro's arguments, when they are expressions.
    fn visit_macro(&mut self, mac: &'a syn::Macro) {
        let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
        if let Ok(args) = mac.parse_body_with(parser) {
            for arg in &args {
                self.visit_expr(arg);
            }
        }
    }
}

// bounded: as deep as an expression nests in a source file
fn unwrapped(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(inner) => unwrapped(&inner.expr),
        Expr::Reference(inner) => unwrapped(&inner.expr),
        Expr::Unary(inner) if matches!(inner.op, syn::UnOp::Deref(_)) => unwrapped(&inner.expr),
        Expr::Try(inner) => unwrapped(&inner.expr),
        other => other,
    }
}

/// A step a chain takes from its variable: a field; a method call or an index, whose type the
/// syntax doesn't say; or a method that gives what it is called on, or a view of it.
enum Step {
    Field(String),
    Call,
    Same,
}

/// The methods that give what they are called on, or a view or an iterator of it.
const SAME: &[&str] = &[
    "as_deref",
    "as_deref_mut",
    "as_mut",
    "as_ref",
    "borrow",
    "borrow_mut",
    "by_ref",
    "clone",
    "cloned",
    "copied",
    "expect",
    "into_iter",
    "iter",
    "iter_mut",
    "peekable",
    "rev",
    "skip",
    "take",
    "unwrap",
];

/// The variable a chain of fields, indexes and method calls starts from, and its steps.
// bounded: as deep as an expression nests in a source file
fn chain(expr: &Expr) -> Option<(String, Vec<Step>)> {
    let (base, step) = match unwrapped(expr) {
        Expr::Path(path) => return Some((path.path.get_ident()?.to_string(), Vec::new())),
        Expr::Field(field) => {
            let name = match &field.member {
                syn::Member::Named(name) => name.to_string(),
                syn::Member::Unnamed(index) => index.index.to_string(),
            };
            (&*field.base, Step::Field(name))
        }
        Expr::Index(index) => (&*index.expr, Step::Call),
        Expr::MethodCall(call) if SAME.iter().any(|same| call.method == same) => {
            (&*call.receiver, Step::Same)
        }
        Expr::MethodCall(call) => (&*call.receiver, Step::Call),
        _ => return None,
    };
    let (root, mut steps) = chain(base)?;
    steps.push(step);
    Some((root, steps))
}

fn from_self(expr: &Expr) -> bool {
    chain(expr).is_some_and(|(root, _)| root == "self")
}

/// The functions here that a call from `functions[caller]` may call. A type's own method of a
/// name is called over a trait's, and a trait's method called directly by its own name is taken
/// to hand the call to the type's own method of that name, in another file if not here.
fn resolve(callee: &Callee, caller: usize, functions: &[Function]) -> Vec<usize> {
    let (owner, name, direct) = match callee {
        Callee::Free(name) => (None, name, false),
        Callee::Of {
            owner,
            name,
            direct,
        } => (Some(owner), name, *direct),
    };
    let candidates: Vec<usize> = (0..functions.len())
        .filter(|&index| functions[index].name == *name && functions[index].owner.as_ref() == owner)
        .collect();
    let inherent = candidates
        .iter()
        .any(|&index| functions[index].kind == ScopeKind::Inherent);
    let from_trait_impl = direct && functions[caller].kind == ScopeKind::TraitImpl;
    candidates
        .into_iter()
        .filter(|&index| !inherent || functions[index].kind == ScopeKind::Inherent)
        .filter(|&index| !(from_trait_impl && index == caller))
        .collect()
}

/// The cycles of the graph: each strongly connected component with more than one function, or
/// one that calls itself, in order of first function.
fn cycles(calls: &[BTreeSet<usize>]) -> Vec<Vec<usize>> {
    let mut tarjan = Tarjan {
        calls,
        index: vec![None; calls.len()],
        low: vec![0; calls.len()],
        on_stack: vec![false; calls.len()],
        stack: Vec::new(),
        next: 0,
        components: Vec::new(),
    };
    for function in 0..calls.len() {
        if tarjan.index[function].is_none() {
            tarjan.connect(function);
        }
    }
    let mut cycles: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for mut component in tarjan.components {
        component.sort();
        let first = component[0];
        if component.len() > 1 || calls[first].contains(&first) {
            cycles.insert(first, component);
        }
    }
    cycles.into_values().collect()
}

struct Tarjan<'a> {
    calls: &'a [BTreeSet<usize>],
    index: Vec<Option<usize>>,
    low: Vec<usize>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    next: usize,
    components: Vec<Vec<usize>>,
}

impl Tarjan<'_> {
    // bounded: as deep as a chain of calls in one file
    fn connect(&mut self, function: usize) {
        self.index[function] = Some(self.next);
        self.low[function] = self.next;
        self.next += 1;
        self.stack.push(function);
        self.on_stack[function] = true;
        for &callee in &self.calls[function] {
            match self.index[callee] {
                None => {
                    self.connect(callee);
                    self.low[function] = self.low[function].min(self.low[callee]);
                }
                Some(index) if self.on_stack[callee] => {
                    self.low[function] = self.low[function].min(index);
                }
                Some(_) => {}
            }
        }
        if Some(self.low[function]) == self.index[function] {
            let mut component = Vec::new();
            while let Some(member) = self.stack.pop() {
                self.on_stack[member] = false;
                component.push(member);
                if member == function {
                    break;
                }
            }
            self.components.push(component);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The functions of each cycle found in `source`.
    fn found(source: &str) -> Vec<Vec<String>> {
        let found = in_source(Path::new("source.rs"), source);
        let names = |cycle: Vec<(String, usize)>| cycle.into_iter().map(|(name, _)| name);
        found
            .into_iter()
            .map(|found| names(found.cycle).collect())
            .collect()
    }

    #[test]
    fn finds_a_function_calling_itself_unless_it_grows_or_is_bounded() {
        let walk = "fn walk(value: &Value) { for item in value.items() { walk(item) } }";
        assert_eq!(found(walk), [["walk"]]);
        let grown =
            "fn walk(value: &Value) { for item in value.items() { stack::grow(|| walk(item)) } }";
        assert!(found(grown).is_empty());
        let bounded = format!("// bounded: values here nest twice at most\n{walk}");
        assert!(found(&bounded).is_empty());
        let no_reason = format!("// bounded:\n{walk}");
        assert_eq!(found(&no_reason), [["walk"]]);
    }

    #[test]
    fn finds_methods_calling_each_other_on_self_and_on_children() {
        let source = "
            impl Reader {
                fn value(&mut self) { self.list() }
                fn list(&mut self) { self.value() }
            }
            impl Node {
                fn size(&self) -> usize { self.children().map(|child| child.size()).sum() }
                fn text(&self) { for child in self.children() { child.text() } }
            }";
        assert_eq!(
            found(source),
            [
                vec!["Reader::value", "Reader::list"],
                vec!["Node::size"],
                vec!["Node::text"]
            ]
        );
    }

    #[test]
    fn takes_a_method_of_what_self_holds_to_be_another_types() {
        let source = "
            struct Wrapper { inner: Inner }
            impl Wrapper {
                fn len(&self) -> usize { self.inner.len() }
                fn is_text(&self) -> bool { self.node_type().is_text() }
                fn maps(&self) -> Vec<Map> { self.with(|mapping| mapping.maps()) }
            }";
        assert!(found(source).is_empty());
    }

    #[test]
    fn follows_what_an_enum_or_struct_here_holds() {
        let source = "
            enum Step { Replace(ReplaceStep) }
            enum Tree { Leaf, Branch(Vec<Tree>) }
            struct List { next: Option<Box<List>> }
            impl Step {
                fn apply(&self) { match self { Step::Replace(step) => step.apply() } }
            }
            impl Tree {
                fn walk(&self) {
                    if let Tree::Branch(children) = self { for child in children { child.walk() } }
                }
            }
            impl List {
                fn len(&self) -> usize { self.next.as_ref().map_or(0, |next| next.len() + 1) }
            }";
        assert_eq!(found(source), [["Tree::walk"], ["List::len"]]);
    }

    #[test]
    fn types_a_closures_parameters_by_the_bound_of_the_method_it_is_handed_to() {
        let source = "
            impl Reader {
                fn below(&mut self, read: impl FnOnce(&mut Self)) { read(self) }
                fn value(&mut self) { self.below(|reader| reader.value()) }
            }";
        assert_eq!(found(source), [["Reader::value"]]);
    }

    #[test]
    fn takes_a_trait_method_calling_its_own_name_to_call_the_types_own() {
        let source = "
            impl JsonView for ValueRef {
                fn kind(self) -> Kind { ValueRef::kind(self) }
            }
            impl<T: ToValue> ToValue for &T {
                fn to_value(&self) -> Value { (**self).to_value() }
            }";
        assert!(found(source).is_empty());
    }
}
