use std::{borrow::Cow, fmt};

use num_bigint::BigInt;
use ruff_python_ast::{
    self as ast, BoolOp, CmpOp, ConversionFlag as RuffConversionFlag, ElifElseClause, Expr as AstExpr,
    InterpolatedStringElement, Keyword, Number, Operator as AstOperator, ParameterWithDefault, Stmt, UnaryOp,
    name::Name,
};
use ruff_python_parser::parse_module;
use ruff_text_size::{Ranged, TextRange};

use crate::{
    StackFrame,
    args::{ArgExprs, Kwarg},
    builtins::{Builtins, BuiltinsFunctions},
    exception_private::ExcType,
    exception_public::{CodeLoc, Exception},
    expressions::{
        Callable, ClassDef, CmpOperator, Comprehension, DictLiteralItem, Expr, ExprLoc, Identifier, Literal, Node,
        Operator, UnpackTarget,
    },
    fstring::{ConversionFlag, FStringPart, FormatSpec},
    intern::{InternerBuilder, StringId},
    types::Type,
    value::EitherStr,
};

/// Maximum nesting depth for AST structures during parsing.
/// Matches CPython's limit of ~200 for nested parentheses.
/// This prevents stack overflow from deeply nested structures like `((((x,),),),)`.
#[cfg(not(debug_assertions))]
pub const MAX_NESTING_DEPTH: u16 = 200;
/// In debug builds, we use a lower limit because stack frames are much larger
/// (no inlining, debug info, etc.). The limit is set conservatively to prevent
/// stack overflow while still catching the error before the recursion limit.
#[cfg(debug_assertions)]
pub const MAX_NESTING_DEPTH: u16 = 35;

/// A parameter in a function signature with optional default value.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParsedParam {
    /// The parameter name.
    pub name: StringId,
    /// The default value expression (evaluated at definition time).
    pub default: Option<ExprLoc>,
    /// The annotation expression for this parameter, if present.
    pub annotation: Option<ExprLoc>,
}

/// A parsed function signature with all parameter types.
///
/// This intermediate representation captures the structure of Python function
/// parameters before name resolution. Default value expressions are stored
/// as unevaluated AST and will be evaluated during the prepare phase.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ParsedSignature {
    /// Positional-only parameters (before `/`).
    pub pos_args: Vec<ParsedParam>,
    /// Positional-or-keyword parameters.
    pub args: Vec<ParsedParam>,
    /// Variable positional parameter (`*args`).
    pub var_args: Option<StringId>,
    /// Keyword-only parameters (after `*` or `*args`).
    pub kwargs: Vec<ParsedParam>,
    /// Variable keyword parameter (`**kwargs`).
    pub var_kwargs: Option<StringId>,
}

impl ParsedSignature {
    /// Returns an iterator over all parameter names in the signature.
    ///
    /// Order: pos_args, args, var_args, kwargs, var_kwargs
    pub fn param_names(&self) -> impl Iterator<Item = StringId> + '_ {
        self.pos_args
            .iter()
            .map(|p| p.name)
            .chain(self.args.iter().map(|p| p.name))
            .chain(self.var_args.iter().copied())
            .chain(self.kwargs.iter().map(|p| p.name))
            .chain(self.var_kwargs.iter().copied())
    }
}

/// A raw (unprepared) function definition from the parser.
///
/// Contains the function name, signature, and body as parsed AST nodes.
/// During the prepare phase, this is transformed into `PreparedFunctionDef`
/// with resolved names and scope information.
#[derive(Debug, Clone)]
pub struct RawFunctionDef {
    /// The function name identifier (not yet resolved to a namespace index).
    pub name: Identifier,
    /// The binding name in the enclosing scope.
    ///
    /// In class bodies, this may be a mangled name (e.g., `_Class__private`)
    /// while `name` retains the original function name for metadata like `__name__`.
    pub binding_name: Identifier,
    /// Parsed type parameter names for PEP 695 syntax (`def f[T]`).
    ///
    /// Stored as interned identifiers; resolved semantics are handled at runtime.
    pub type_params: Vec<StringId>,
    /// The parsed function signature with parameter names and default expressions.
    pub signature: ParsedSignature,
    /// The unprepared function body (names not yet resolved).
    pub body: Vec<ParseNode>,
    /// Optional return annotation expression.
    pub return_annotation: Option<ExprLoc>,
    /// Whether this is an async function (`async def`).
    pub is_async: bool,
    /// Decorator expressions applied to this function (e.g., `@staticmethod`, `@property`).
    ///
    /// Parsed from the `decorator_list` in the ruff AST. These are evaluated
    /// at definition time and applied bottom-to-top (last decorator applied first).
    /// Inside class bodies, these enable `@staticmethod`, `@classmethod`, and `@property`.
    pub decorators: Vec<ExprLoc>,
}

/// Type alias for parsed AST nodes (output of the parser).
///
/// This uses `Node<RawFunctionDef>` where function definitions contain their
/// full unprepared body. After the prepare phase, this becomes `PreparedNode`
/// (aka `Node<PreparedFunctionDef>`).
pub type ParseNode = Node<RawFunctionDef>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Try<N> {
    pub body: Vec<N>,
    pub handlers: Vec<ExceptHandler<N>>,
    pub or_else: Vec<N>,
    pub finally: Vec<N>,
}

/// A parsed exception handler (except clause).
///
/// Represents `except ExcType as name:` or bare `except:` clauses.
/// The exception type and variable binding are both optional.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExceptHandler<N> {
    /// Exception type(s) to catch. None = bare except (catches all).
    pub exc_type: Option<ExprLoc>,
    /// Variable name for `except X as e:`. None = no binding.
    pub name: Option<Identifier>,
    /// Handler body statements.
    pub body: Vec<N>,
}

/// Parsed representation of a match-pattern check and the bindings it introduces.
///
/// Pattern matching is lowered to existing control-flow nodes (`if`, assignments, guards)
/// during parsing. This helper carries the generated boolean test expression and the
/// capture assignments that should run only after the test succeeds.
#[derive(Debug, Clone)]
struct MatchPatternPlan {
    /// Boolean expression that evaluates whether the pattern matches.
    test: ExprLoc,
    /// Name-binding assignments performed on successful matches.
    bindings: Vec<ParseNode>,
}

/// Result of parsing: the AST nodes and the string interner with all interned names.
#[derive(Debug)]
pub struct ParseResult {
    pub nodes: Vec<ParseNode>,
    pub interner: InternerBuilder,
}

pub(crate) fn parse(code: &str, filename: &str) -> Result<ParseResult, ParseError> {
    let mut parser = Parser::new(code, filename);
    let parsed = parse_module(code).map_err(|e| ParseError::syntax(e.to_string(), parser.convert_range(e.range())))?;
    let module = parsed.into_syntax();
    let nodes = parser.parse_statements(module.body)?;
    Ok(ParseResult {
        nodes,
        interner: parser.interner,
    })
}

/// Parses code using an existing interner state.
///
/// This is used by REPL execution to parse subsequent snippets with a clone of
/// the session interner, preserving StringId stability across lines while keeping
/// the original session state unchanged on parse failures.
pub(crate) fn parse_with_interner(
    code: &str,
    filename: &str,
    interner: InternerBuilder,
) -> Result<ParseResult, ParseError> {
    let mut parser = Parser::new_with_interner(code, filename, interner);
    let parsed = parse_module(code).map_err(|e| ParseError::syntax(e.to_string(), parser.convert_range(e.range())))?;
    let module = parsed.into_syntax();
    let nodes = parser.parse_statements(module.body)?;
    Ok(ParseResult {
        nodes,
        interner: parser.interner,
    })
}

/// Parser for converting ruff AST to Ouros's intermediate ParseNode representation.
///
/// Holds references to the source code and owns a string interner for names.
/// The filename is interned once at construction and reused for all CodeRanges.
pub struct Parser<'a> {
    line_ends: Vec<usize>,
    code: &'a str,
    /// Interned filename ID, used for all CodeRanges created by this parser.
    filename_id: StringId,
    /// String interner for names (variables, functions, etc).
    pub interner: InternerBuilder,
    /// Stack of enclosing class names for name mangling.
    ///
    /// Python mangles `__private` identifiers inside class bodies using the
    /// current class name. Nested classes push a new name while parsing their
    /// bodies, so names inside the nested body are mangled with the inner class.
    class_stack: Vec<StringId>,
    /// Remaining nesting depth budget for recursive structures.
    /// Starts at MAX_NESTING_DEPTH and decrements on each nested level.
    /// When it reaches zero, we return a "too many nested parentheses" error.
    depth_remaining: u16,
    /// Number of nested function bodies currently being parsed.
    ///
    /// Used to distinguish class-scope statements from statements inside
    /// methods nested under a class body.
    function_depth: usize,
    /// Counter used to generate synthetic local names for parser desugarings.
    ///
    /// Names use a dotted prefix (e.g. `.try_star_0`) so they cannot collide
    /// with user-defined identifiers from source code.
    synthetic_name_counter: u32,
}

impl<'a> Parser<'a> {
    /// Creates a parser with a fresh interner.
    ///
    /// This is the standard entrypoint for one-shot execution where interned
    /// values do not need to persist across parses.
    fn new(code: &'a str, filename: &'a str) -> Self {
        // Position of each line in the source code, to convert indexes to line number and column number
        let mut line_ends = vec![];
        for (i, c) in code.chars().enumerate() {
            if c == '\n' {
                line_ends.push(i);
            }
        }
        let mut interner = InternerBuilder::new(code);
        let filename_id = interner.intern(filename);
        Self {
            line_ends,
            code,
            filename_id,
            interner,
            class_stack: Vec::new(),
            depth_remaining: MAX_NESTING_DEPTH,
            function_depth: 0,
            synthetic_name_counter: 0,
        }
    }

    /// Creates a parser that reuses an existing interner state.
    ///
    /// New strings discovered during parsing are interned into the provided
    /// builder, preserving existing IDs and append-only growth semantics.
    fn new_with_interner(code: &'a str, filename: &'a str, mut interner: InternerBuilder) -> Self {
        let mut line_ends = vec![];
        for (i, c) in code.chars().enumerate() {
            if c == '\n' {
                line_ends.push(i);
            }
        }
        let filename_id = interner.intern(filename);
        Self {
            line_ends,
            code,
            filename_id,
            interner,
            class_stack: Vec::new(),
            depth_remaining: MAX_NESTING_DEPTH,
            function_depth: 0,
            synthetic_name_counter: 0,
        }
    }

    fn parse_statements(&mut self, statements: Vec<Stmt>) -> Result<Vec<ParseNode>, ParseError> {
        statements.into_iter().map(|f| self.parse_statement(f)).collect()
    }

    fn parse_elif_else_clauses(&mut self, clauses: Vec<ElifElseClause>) -> Result<Vec<ParseNode>, ParseError> {
        let mut tail: Vec<ParseNode> = Vec::new();
        for clause in clauses.into_iter().rev() {
            match clause.test {
                Some(test) => {
                    let test = self.parse_expression(test)?;
                    let body = self.parse_statements(clause.body)?;
                    let or_else = tail;
                    let nested = Node::If { test, body, or_else };
                    tail = vec![nested];
                }
                None => {
                    tail = self.parse_statements(clause.body)?;
                }
            }
        }
        Ok(tail)
    }

    /// Parses an exception handler (except clause).
    ///
    /// Handles `except:`, `except ExcType:`, and `except ExcType as name:` forms.
    fn parse_except_handler(
        &mut self,
        handler: ruff_python_ast::ExceptHandler,
    ) -> Result<ExceptHandler<ParseNode>, ParseError> {
        let ruff_python_ast::ExceptHandler::ExceptHandler(h) = handler;
        let exc_type = match h.type_ {
            Some(expr) => Some(self.parse_expression(*expr)?),
            None => None,
        };
        let name = h.name.map(|n| self.identifier(&n.id, n.range));
        let body = self.parse_statements(h.body)?;
        Ok(ExceptHandler { exc_type, name, body })
    }

    fn parse_statement(&mut self, statement: Stmt) -> Result<ParseNode, ParseError> {
        self.decr_depth_remaining(|| statement.range())?;
        let result = self.parse_statement_impl(statement);
        self.depth_remaining += 1;
        result
    }

    fn parse_statement_impl(&mut self, statement: Stmt) -> Result<ParseNode, ParseError> {
        match statement {
            Stmt::FunctionDef(function) => {
                let params = &function.parameters;
                let type_params = self.parse_type_params(function.type_params);

                // Parse positional-only parameters (before /)
                let pos_args = self.parse_params_with_defaults(&params.posonlyargs)?;

                // Parse positional-or-keyword parameters
                let args = self.parse_params_with_defaults(&params.args)?;

                // Parse *args
                let var_args = params
                    .vararg
                    .as_ref()
                    .map(|p| self.intern_name_maybe_mangled(&p.name.id));

                // Parse keyword-only parameters (after * or *args)
                let kwargs = self.parse_params_with_defaults(&params.kwonlyargs)?;

                // Parse **kwargs
                let var_kwargs = params
                    .kwarg
                    .as_ref()
                    .map(|p| self.intern_name_maybe_mangled(&p.name.id));

                let signature = ParsedSignature {
                    pos_args,
                    args,
                    var_args,
                    kwargs,
                    var_kwargs,
                };

                let name = self.identifier_raw(&function.name.id, function.name.range);
                let binding_name = self.identifier(&function.name.id, function.name.range);
                let return_annotation = function.returns.map(|expr| self.parse_expression(*expr)).transpose()?;
                // Parse function body recursively under function scope.
                self.function_depth += 1;
                let body = self.parse_statements(function.body)?;
                self.function_depth -= 1;
                let is_async = function.is_async;

                // Parse decorators (e.g., @staticmethod, @property)
                let decorators = function
                    .decorator_list
                    .into_iter()
                    .map(|d| self.parse_expression(d.expression))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(Node::FunctionDef(RawFunctionDef {
                    name,
                    binding_name,
                    type_params,
                    signature,
                    body,
                    return_annotation,
                    is_async,
                    decorators,
                }))
            }
            Stmt::ClassDef(c) => {
                let name = self.identifier_raw(&c.name.id, c.name.range);
                let binding_name = self.identifier(&c.name.id, c.name.range);
                let type_params = self.parse_type_params(c.type_params);

                // Parse base class expressions and class keyword arguments
                let (bases, keywords, var_kwargs) = if let Some(ref arguments) = c.arguments {
                    let bases = arguments
                        .args
                        .iter()
                        .map(|arg| self.parse_expression(arg.clone()))
                        .collect::<Result<Vec<_>, _>>()?;
                    let (keywords, var_kwargs) = self.parse_keywords(arguments.keywords.clone().into_vec())?;
                    (bases, keywords, var_kwargs)
                } else {
                    (Vec::new(), Vec::new(), None)
                };

                // Parse class decorators (e.g., @tracker1, @tracker2)
                let decorators = c
                    .decorator_list
                    .into_iter()
                    .map(|d| self.parse_expression(d.expression))
                    .collect::<Result<Vec<_>, _>>()?;

                // Parse class body with class name on the stack for name mangling
                self.class_stack.push(name.name_id);
                let mut body = self.parse_statements(c.body)?;
                self.class_stack.pop();

                // Preserve class docstrings as `__doc__` like CPython.
                if let Some(Node::Expr(ExprLoc {
                    expr: Expr::Literal(Literal::Str(_)),
                    ..
                })) = body.first()
                {
                    let doc_expr = match &body[0] {
                        Node::Expr(expr) => expr.clone(),
                        _ => unreachable!("checked first node is Node::Expr"),
                    };
                    body.insert(
                        0,
                        Node::Assign {
                            target: Identifier::new(self.interner.intern("__doc__"), self.convert_range(c.range)),
                            object: doc_expr,
                        },
                    );
                }

                // Ensure class bodies always start with an `__annotations__` dict so
                // annotated assignments can update it directly during execution.
                body.insert(
                    0,
                    Node::Assign {
                        target: Identifier::new(self.interner.intern("__annotations__"), self.convert_range(c.range)),
                        object: ExprLoc::new(self.convert_range(c.range), Expr::DictUnpack(Vec::new())),
                    },
                );

                Ok(Node::ClassDef(Box::new(ClassDef {
                    name,
                    binding_name,
                    type_params,
                    bases,
                    keywords,
                    var_kwargs,
                    body,
                    namespace_size: 0,     // Will be computed during prepare phase
                    class_cell_slot: None, // Filled during prepare when needed
                    class_free_var_enclosing_slots: Vec::new(),
                    class_free_var_target_slots: Vec::new(),
                    local_names: Vec::new(), // Will be populated during prepare phase
                    decorators,
                })))
            }
            Stmt::Return(ast::StmtReturn { value, .. }) => match value {
                Some(value) => Ok(Node::Return(self.parse_expression(*value)?)),
                None => Ok(Node::ReturnNone),
            },
            Stmt::Delete(ast::StmtDelete { targets, range, .. }) => {
                // del can have multiple targets: `del x, y, z`
                // Each target can be a name, attribute, or subscript
                let mut nodes = Vec::new();
                for target in targets {
                    match target {
                        AstExpr::Name(ast::ExprName {
                            id, range: name_range, ..
                        }) => {
                            nodes.push(Node::DeleteName(self.identifier(&id, name_range)));
                        }
                        AstExpr::Attribute(ast::ExprAttribute {
                            value,
                            attr,
                            range: attr_range,
                            ..
                        }) => {
                            nodes.push(Node::DeleteAttr {
                                object: self.parse_expression(*value)?,
                                attr: EitherStr::Interned(self.maybe_mangle_name(attr.id())),
                                position: self.convert_range(attr_range),
                            });
                        }
                        AstExpr::Subscript(ast::ExprSubscript {
                            value,
                            slice,
                            range: sub_range,
                            ..
                        }) => {
                            nodes.push(Node::DeleteSubscr {
                                object: self.parse_expression(*value)?,
                                index: self.parse_expression(*slice)?,
                                position: self.convert_range(sub_range),
                            });
                        }
                        other => {
                            return Err(ParseError::syntax(
                                format!("Invalid del target: {other:?}"),
                                self.convert_range(range),
                            ));
                        }
                    }
                }
                // If single target, return the single node; otherwise wrap in a sequence
                // (we don't have a multi-node variant, so emit them individually)
                if nodes.len() == 1 {
                    Ok(nodes.into_iter().next().unwrap())
                } else {
                    // Multiple del targets: flatten into individual statements
                    // We need to return just one node, so we'll use the first
                    // and handle multi-target del by returning a list...
                    // Actually, the parse_statement returns a single node.
                    // For now, we only support single-target del.
                    // Multi-target del (del x, y) is uncommon.
                    // Let's handle it properly by adding support.
                    Err(ParseError::not_implemented(
                        "multi-target del statements (del x, y)",
                        self.convert_range(range),
                    ))
                }
            }
            Stmt::TypeAlias(t) => self.parse_type_alias_statement(t),
            Stmt::Assign(ast::StmtAssign {
                targets, value, range, ..
            }) => self.parse_assign_statement(targets, *value, self.convert_range(range)),
            Stmt::AugAssign(ast::StmtAugAssign {
                target,
                op,
                value,
                range,
                ..
            }) => {
                let op = convert_op(op);
                let rhs = self.parse_expression(*value)?;
                match *target {
                    // Simple name: x += value
                    AstExpr::Name(ast::ExprName {
                        id, range: name_range, ..
                    }) => Ok(Node::OpAssign {
                        target: self.identifier(&id, name_range),
                        op,
                        object: rhs,
                    }),
                    // Attribute target: obj.attr += value
                    AstExpr::Attribute(ast::ExprAttribute {
                        value: obj,
                        attr,
                        range: attr_range,
                        ..
                    }) => Ok(Node::OpAssignAttr {
                        object: self.parse_expression(*obj)?,
                        attr: EitherStr::Interned(self.maybe_mangle_name(attr.id())),
                        op,
                        value: rhs,
                        target_position: self.convert_range(attr_range),
                    }),
                    // Subscript target: obj[key] += value
                    AstExpr::Subscript(ast::ExprSubscript {
                        value: obj,
                        slice,
                        range: sub_range,
                        ..
                    }) => Ok(Node::OpAssignSubscr {
                        object: self.parse_expression(*obj)?,
                        index: self.parse_expression(*slice)?,
                        op,
                        value: rhs,
                        target_position: self.convert_range(sub_range),
                    }),
                    other => Err(ParseError::syntax(
                        format!("Invalid augmented assignment target: {other:?}"),
                        self.convert_range(range),
                    )),
                }
            }
            Stmt::AnnAssign(ast::StmtAnnAssign {
                target,
                annotation,
                value,
                range,
                ..
            }) => {
                // Class-scope annotated names are lowered into two runtime effects:
                // 1) Optional value assignment (`x: T = v` or placeholder `x = None`)
                // 2) `__annotations__['x'] = T`
                //
                // We encode both statements in a single always-true `if` node so the
                // parser can continue returning one node per source statement.
                if self.function_depth == 0
                    && !self.class_stack.is_empty()
                    && let AstExpr::Name(ast::ExprName {
                        id, range: name_range, ..
                    }) = *target
                {
                    let mut body = Vec::with_capacity(2);
                    if let Some(value_expr) = value {
                        body.push(Node::Assign {
                            target: self.identifier(&id, name_range),
                            object: self.parse_expression(*value_expr)?,
                        });
                    } else {
                        body.push(Node::Assign {
                            target: self.identifier(&id, name_range),
                            object: ExprLoc {
                                position: self.convert_range(range),
                                expr: Expr::Literal(Literal::None),
                            },
                        });
                    }

                    let annotation_target = ExprLoc::new(
                        self.convert_range(range),
                        Expr::Name(Identifier::new(
                            self.interner.intern("__annotations__"),
                            self.convert_range(range),
                        )),
                    );
                    let annotation_key = ExprLoc::new(
                        self.convert_range(name_range),
                        Expr::Literal(Literal::Str(self.maybe_mangle_name(id.as_str()))),
                    );
                    body.push(Node::SubscriptAssign {
                        target: annotation_target,
                        index: annotation_key,
                        value: self.parse_expression(*annotation)?,
                        target_position: self.convert_range(range),
                    });

                    return Ok(Node::If {
                        test: ExprLoc::new(self.convert_range(range), Expr::Literal(Literal::Bool(true))),
                        body,
                        or_else: Vec::new(),
                    });
                }

                // Non-class annotations keep existing lowering:
                // assignment when a value exists, otherwise no runtime effect.
                if let Some(value_expr) = value {
                    self.parse_assignment(*target, *value_expr)
                } else {
                    Ok(Node::Pass)
                }
            }
            Stmt::For(ast::StmtFor {
                is_async,
                target,
                iter,
                body,
                orelse,
                range,
                ..
            }) => {
                if is_async {
                    return self.lower_async_for_statement(*target, *iter, body, orelse, self.convert_range(range));
                }
                Ok(Node::For {
                    target: self.parse_unpack_target(*target)?,
                    iter: self.parse_expression(*iter)?,
                    body: self.parse_statements(body)?,
                    or_else: self.parse_statements(orelse)?,
                })
            }
            Stmt::While(ast::StmtWhile { test, body, orelse, .. }) => Ok(Node::While {
                test: self.parse_expression(*test)?,
                body: self.parse_statements(body)?,
                or_else: self.parse_statements(orelse)?,
            }),
            Stmt::If(ast::StmtIf {
                test,
                body,
                elif_else_clauses,
                ..
            }) => {
                let test = self.parse_expression(*test)?;
                let body = self.parse_statements(body)?;
                let or_else = self.parse_elif_else_clauses(elif_else_clauses)?;
                Ok(Node::If { test, body, or_else })
            }
            Stmt::With(ast::StmtWith {
                is_async,
                items,
                body,
                range,
                ..
            }) => {
                let position = self.convert_range(range);
                if items.len() != 1 {
                    return Err(ParseError::not_implemented("multi-item with statements", position));
                }
                let item = items.into_iter().next().unwrap();
                let context_expr = self.parse_expression(item.context_expr)?;
                let (var, mut target_preamble) = self.parse_with_as_target(item.optional_vars, position)?;
                let mut body = self.parse_statements(body)?;
                if !target_preamble.is_empty() {
                    target_preamble.append(&mut body);
                    body = target_preamble;
                }
                if is_async {
                    return self.lower_async_with_statement(context_expr, var, body, position);
                }
                Ok(Node::With {
                    context_expr,
                    var,
                    body,
                })
            }
            Stmt::Match(m) => self.parse_match_statement(m),
            Stmt::Raise(ast::StmtRaise { exc, cause, .. }) => {
                let expr = match exc {
                    Some(expr) => Some(self.parse_expression(*expr)?),
                    None => None,
                };
                let cause = match cause {
                    Some(expr) => Some(self.parse_expression(*expr)?),
                    None => None,
                };
                Ok(Node::Raise(expr, cause))
            }
            Stmt::Try(ast::StmtTry {
                body,
                handlers,
                orelse,
                finalbody,
                is_star,
                range,
                ..
            }) => {
                if is_star {
                    self.parse_try_star_statement(body, handlers, orelse, finalbody, range)
                } else {
                    let body = self.parse_statements(body)?;
                    let handlers = handlers
                        .into_iter()
                        .map(|h| self.parse_except_handler(h))
                        .collect::<Result<Vec<_>, _>>()?;
                    let or_else = self.parse_statements(orelse)?;
                    let finally = self.parse_statements(finalbody)?;
                    Ok(Node::Try(Try {
                        body,
                        handlers,
                        or_else,
                        finally,
                    }))
                }
            }
            Stmt::Assert(ast::StmtAssert { test, msg, .. }) => {
                let test = self.parse_expression(*test)?;
                let msg = match msg {
                    Some(m) => Some(self.parse_expression(*m)?),
                    None => None,
                };
                Ok(Node::Assert { test, msg })
            }
            Stmt::Import(ast::StmtImport { names, range, .. }) => {
                // We only support single module imports (e.g., `import sys`)
                // Multi-module imports (e.g., `import sys, os`) are not supported
                let position = self.convert_range(range);
                if names.len() != 1 {
                    return Err(ParseError::not_implemented("multi-module import statements", position));
                }
                let alias_node = &names[0];
                let module_name = self.interner.intern(&alias_node.name);
                let has_alias = alias_node.asname.is_some();
                // The binding name is:
                // - alias target when present (`import pkg.mod as alias` -> `alias`)
                // - otherwise the top-level package (`import pkg.mod` -> `pkg`)
                // This matches CPython import binding behavior for dotted imports.
                let binding_name = if let Some(asname) = alias_node.asname.as_ref() {
                    self.maybe_mangle_name(asname.as_str())
                } else {
                    let top_level = alias_node
                        .name
                        .as_str()
                        .split('.')
                        .next()
                        .expect("import target should never be empty");
                    self.maybe_mangle_name(top_level)
                };
                // Create an unresolved identifier (namespace slot will be set during prepare)
                let binding = Identifier::new(binding_name, position);
                Ok(Node::Import {
                    module_name,
                    binding,
                    has_alias,
                })
            }
            Stmt::ImportFrom(ast::StmtImportFrom {
                module,
                names,
                level,
                range,
                ..
            }) => {
                let position = self.convert_range(range);
                // We only support absolute imports (level 0)
                if level != 0 {
                    return Err(ParseError::import_error(
                        "attempted relative import with no known parent package",
                        position,
                    ));
                }
                // Module name is required for absolute imports
                let module_name = match module {
                    Some(m) => self.interner.intern(&m),
                    None => {
                        return Err(ParseError::import_error(
                            "attempted relative import with no known parent package",
                            position,
                        ));
                    }
                };
                // Parse the imported names
                let names = names
                    .iter()
                    .map(|alias| {
                        // Check for star import which is not supported
                        if alias.name.as_str() == "*" {
                            return Err(ParseError::not_supported(
                                "Wildcard imports (`from ... import *`) are not supported",
                                position,
                            ));
                        }
                        let name = self.interner.intern(&alias.name);
                        // The binding name is the alias if provided, otherwise the import name
                        let binding_name = if let Some(asname) = alias.asname.as_ref() {
                            self.maybe_mangle_name(asname.as_str())
                        } else {
                            self.maybe_mangle_name(alias.name.as_str())
                        };
                        // Create an unresolved identifier (namespace slot will be set during prepare)
                        let binding = Identifier::new(binding_name, position);
                        Ok((name, binding))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Node::ImportFrom {
                    module_name,
                    names,
                    position,
                })
            }
            Stmt::Global(ast::StmtGlobal { names, range, .. }) => {
                let names = names
                    .iter()
                    .map(|id| self.maybe_mangle_name(&self.code[id.range]))
                    .collect();
                Ok(Node::Global {
                    position: self.convert_range(range),
                    names,
                })
            }
            Stmt::Nonlocal(ast::StmtNonlocal { names, range, .. }) => {
                let names = names
                    .iter()
                    .map(|id| self.maybe_mangle_name(&self.code[id.range]))
                    .collect();
                Ok(Node::Nonlocal {
                    position: self.convert_range(range),
                    names,
                })
            }
            Stmt::Expr(ast::StmtExpr { value, .. }) => self.parse_expression(*value).map(Node::Expr),
            Stmt::Pass(_) => Ok(Node::Pass),
            Stmt::Break(b) => Ok(Node::Break {
                position: self.convert_range(b.range),
            }),
            Stmt::Continue(c) => Ok(Node::Continue {
                position: self.convert_range(c.range),
            }),
            Stmt::IpyEscapeCommand(i) => Err(ParseError::not_implemented(
                "IPython escape commands",
                self.convert_range(i.range),
            )),
        }
    }

    /// Lowers a `match` statement into nested `if` nodes.
    ///
    /// This parser-side lowering lets the existing compiler and VM execute the
    /// supported subset of structural pattern matching without introducing new
    /// bytecode instructions. The subject expression is currently re-used in each
    /// case test rather than stored in a temporary.
    fn parse_match_statement(&mut self, match_stmt: ast::StmtMatch) -> Result<ParseNode, ParseError> {
        let subject = self.parse_expression(*match_stmt.subject)?;
        self.parse_match_case_chain(&subject, &match_stmt.cases)
    }

    /// Recursively builds a nested `if/else` chain for match cases.
    fn parse_match_case_chain(&mut self, subject: &ExprLoc, cases: &[ast::MatchCase]) -> Result<ParseNode, ParseError> {
        let Some((case, rest)) = cases.split_first() else {
            return Ok(Node::Pass);
        };

        let next = self.parse_match_case_chain(subject, rest)?;
        let plan = self.parse_match_pattern(subject.clone(), case.pattern.clone())?;

        let mut case_body = plan.bindings;
        let parsed_case_body = self.parse_statements(case.body.clone())?;

        if let Some(guard) = &case.guard {
            let guard_expr = self.parse_expression((**guard).clone())?;
            case_body.push(Node::If {
                test: guard_expr,
                body: parsed_case_body,
                or_else: vec![next.clone()],
            });
        } else {
            case_body.extend(parsed_case_body);
        }

        Ok(Node::If {
            test: plan.test,
            body: case_body,
            or_else: vec![next],
        })
    }

    /// Converts a single pattern node into a boolean match test and capture bindings.
    ///
    /// Supported subset:
    /// - value patterns (`case 1`, `case 'x'`)
    /// - singleton patterns (`case None`, `case True`, `case False`)
    /// - capture and wildcard patterns (`case name`, `case _`)
    /// - fixed-size sequence patterns without starred targets (`case (x, y)`)
    /// - or-patterns without captures (`case 1 | 2`)
    fn parse_match_pattern(&mut self, subject: ExprLoc, pattern: ast::Pattern) -> Result<MatchPatternPlan, ParseError> {
        let position = self.convert_range(pattern.range());
        match pattern {
            ast::Pattern::MatchValue(value_pat) => {
                let rhs = self.parse_expression(*value_pat.value)?;
                Ok(MatchPatternPlan {
                    test: Self::make_cmp_expr(subject, CmpOperator::Eq, rhs, position),
                    bindings: Vec::new(),
                })
            }
            ast::Pattern::MatchSingleton(singleton_pat) => {
                let literal = match singleton_pat.value {
                    ast::Singleton::None => Literal::None,
                    ast::Singleton::True => Literal::Bool(true),
                    ast::Singleton::False => Literal::Bool(false),
                };
                let rhs = ExprLoc::new(position, Expr::Literal(literal));
                Ok(MatchPatternPlan {
                    test: Self::make_cmp_expr(subject, CmpOperator::Is, rhs, position),
                    bindings: Vec::new(),
                })
            }
            ast::Pattern::MatchAs(as_pat) => {
                // Wildcard (`_`) and bare wildcard (`case _`) are both irrefutable without binding.
                if as_pat.pattern.is_none() && as_pat.name.as_ref().is_none_or(|name| name.id.as_str() == "_") {
                    return Ok(MatchPatternPlan {
                        test: ExprLoc::new(position, Expr::Literal(Literal::Bool(true))),
                        bindings: Vec::new(),
                    });
                }

                if let Some(inner_pattern) = as_pat.pattern {
                    let mut inner_plan = self.parse_match_pattern(subject.clone(), *inner_pattern)?;
                    if let Some(name) = as_pat.name
                        && name.id.as_str() != "_"
                    {
                        inner_plan.bindings.push(Node::Assign {
                            target: self.identifier(&name.id, name.range),
                            object: subject,
                        });
                    }
                    return Ok(inner_plan);
                }

                // Capture pattern: `case name`
                let Some(name) = as_pat.name else {
                    return Err(ParseError::syntax("invalid capture pattern", position));
                };
                Ok(MatchPatternPlan {
                    test: ExprLoc::new(position, Expr::Literal(Literal::Bool(true))),
                    bindings: vec![Node::Assign {
                        target: self.identifier(&name.id, name.range),
                        object: subject,
                    }],
                })
            }
            ast::Pattern::MatchSequence(seq_pat) => {
                if seq_pat
                    .patterns
                    .iter()
                    .any(|pattern| matches!(pattern, ast::Pattern::MatchStar(_)))
                {
                    return Err(ParseError::not_implemented(
                        "starred sequence patterns in match",
                        position,
                    ));
                }

                let tuple_type = ExprLoc::new(position, Expr::Builtin(Builtins::Type(crate::types::Type::Tuple)));
                let isinstance_test = Self::make_builtin_call(
                    BuiltinsFunctions::Isinstance,
                    ArgExprs::Two(subject.clone(), tuple_type),
                    position,
                );
                let len_call =
                    Self::make_builtin_call(BuiltinsFunctions::Len, ArgExprs::One(subject.clone()), position);
                let pattern_len = i64::try_from(seq_pat.patterns.len()).expect("pattern length exceeds i64");
                let expected_len = ExprLoc::new(position, Expr::Literal(Literal::Int(pattern_len)));
                let len_test = Self::make_cmp_expr(len_call, CmpOperator::Eq, expected_len, position);

                let mut test = Self::make_bool_op_expr(isinstance_test, Operator::And, len_test, position);
                let mut bindings = Vec::new();

                for (index, sub_pattern) in seq_pat.patterns.into_iter().enumerate() {
                    let index = i64::try_from(index).expect("pattern index exceeds i64");
                    let element = ExprLoc::new(
                        position,
                        Expr::Subscript {
                            object: Box::new(subject.clone()),
                            index: Box::new(ExprLoc::new(position, Expr::Literal(Literal::Int(index)))),
                        },
                    );
                    let sub_plan = self.parse_match_pattern(element, sub_pattern)?;
                    test = Self::make_bool_op_expr(test, Operator::And, sub_plan.test, position);
                    bindings.extend(sub_plan.bindings);
                }

                Ok(MatchPatternPlan { test, bindings })
            }
            ast::Pattern::MatchOr(or_pat) => {
                let mut alternatives = or_pat.patterns.into_iter();
                let Some(first_pattern) = alternatives.next() else {
                    return Err(ParseError::syntax("empty or-pattern", position));
                };

                let first = self.parse_match_pattern(subject.clone(), first_pattern)?;
                if !first.bindings.is_empty() {
                    return Err(ParseError::not_implemented("or-pattern captures in match", position));
                }
                let mut test = first.test;
                for alt in alternatives {
                    let plan = self.parse_match_pattern(subject.clone(), alt)?;
                    if !plan.bindings.is_empty() {
                        return Err(ParseError::not_implemented("or-pattern captures in match", position));
                    }
                    test = Self::make_bool_op_expr(test, Operator::Or, plan.test, position);
                }
                Ok(MatchPatternPlan {
                    test,
                    bindings: Vec::new(),
                })
            }
            ast::Pattern::MatchClass(_) => Err(ParseError::not_implemented("class patterns in match", position)),
            ast::Pattern::MatchMapping(_) => Err(ParseError::not_implemented("mapping patterns in match", position)),
            ast::Pattern::MatchStar(_) => Err(ParseError::not_implemented("star patterns in match", position)),
        }
    }

    /// Lowers `try*/except*` into a regular `try/except ExceptionGroup` block.
    ///
    /// The lowered handler splits the caught group's `.exceptions` by each handler type,
    /// runs matching handler bodies with subgroup bindings, and re-raises any unmatched
    /// exceptions as a new `ExceptionGroup`.
    fn parse_try_star_statement(
        &mut self,
        body: Vec<Stmt>,
        handlers: Vec<ruff_python_ast::ExceptHandler>,
        orelse: Vec<Stmt>,
        finalbody: Vec<Stmt>,
        range: TextRange,
    ) -> Result<ParseNode, ParseError> {
        let position = self.convert_range(range);
        let body = self.parse_statements(body)?;
        let or_else = self.parse_statements(orelse)?;
        let finally = self.parse_statements(finalbody)?;

        let group_ident = self.synthetic_identifier(".try_star_group", position);
        let lowered_handler_body = self.parse_try_star_handlers(&group_ident, handlers, position)?;
        let exception_group_expr = ExprLoc::new(position, Expr::Builtin(Builtins::ExcType(ExcType::ExceptionGroup)));
        let handler = ExceptHandler {
            exc_type: Some(exception_group_expr),
            name: Some(group_ident),
            body: lowered_handler_body,
        };

        Ok(Node::Try(Try {
            body,
            handlers: vec![handler],
            or_else,
            finally,
        }))
    }

    /// Builds the body of the lowered `except ExceptionGroup as group` handler.
    fn parse_try_star_handlers(
        &mut self,
        group_ident: &Identifier,
        handlers: Vec<ruff_python_ast::ExceptHandler>,
        position: CodeRange,
    ) -> Result<Vec<ParseNode>, ParseError> {
        let mut lowered: Vec<ParseNode> = Vec::new();
        let remaining_ident = self.synthetic_identifier(".try_star_remaining", position);
        let message_ident = self.synthetic_identifier(".try_star_message", position);
        let exceptions_attr = self.interner.intern("exceptions");
        let message_attr = self.interner.intern("message");
        let append_attr = self.interner.intern("append");

        lowered.push(Node::Assign {
            target: remaining_ident,
            object: ExprLoc::new(
                position,
                Expr::AttrGet {
                    object: Box::new(Self::make_name_expr(*group_ident)),
                    attr: EitherStr::Interned(exceptions_attr),
                },
            ),
        });
        lowered.push(Node::Assign {
            target: message_ident,
            object: ExprLoc::new(
                position,
                Expr::AttrGet {
                    object: Box::new(Self::make_name_expr(*group_ident)),
                    attr: EitherStr::Interned(message_attr),
                },
            ),
        });

        for handler in handlers {
            let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
            let Some(exc_type) = handler.type_ else {
                return Err(ParseError::syntax("except* requires an exception type", position));
            };
            let exc_type = self.parse_expression(*exc_type)?;
            let handler_body = self.parse_statements(handler.body)?;
            let matched_ident = self.synthetic_identifier(".try_star_matched", position);
            let rest_ident = self.synthetic_identifier(".try_star_rest", position);
            let item_ident = self.synthetic_identifier(".try_star_item", position);

            lowered.push(Node::Assign {
                target: matched_ident,
                object: ExprLoc::new(position, Expr::List(Vec::new())),
            });
            lowered.push(Node::Assign {
                target: rest_ident,
                object: ExprLoc::new(position, Expr::List(Vec::new())),
            });

            let item_expr = Self::make_name_expr(item_ident);
            let append_matched = Node::Expr(ExprLoc::new(
                position,
                Expr::AttrCall {
                    object: Box::new(Self::make_name_expr(matched_ident)),
                    attr: EitherStr::Interned(append_attr),
                    args: Box::new(ArgExprs::One(item_expr.clone())),
                },
            ));
            let append_rest = Node::Expr(ExprLoc::new(
                position,
                Expr::AttrCall {
                    object: Box::new(Self::make_name_expr(rest_ident)),
                    attr: EitherStr::Interned(append_attr),
                    args: Box::new(ArgExprs::One(item_expr.clone())),
                },
            ));
            let is_match = Self::make_builtin_call(
                BuiltinsFunctions::Isinstance,
                ArgExprs::Two(item_expr, exc_type),
                position,
            );
            lowered.push(Node::For {
                target: UnpackTarget::Name(item_ident),
                iter: Self::make_name_expr(remaining_ident),
                body: vec![Node::If {
                    test: is_match,
                    body: vec![append_matched],
                    or_else: vec![append_rest],
                }],
                or_else: Vec::new(),
            });
            lowered.push(Node::Assign {
                target: remaining_ident,
                object: Self::make_name_expr(rest_ident),
            });

            let mut when_matched: Vec<ParseNode> = Vec::new();
            if let Some(name) = handler.name
                && name.id.as_str() != "_"
            {
                when_matched.push(Node::Assign {
                    target: self.identifier(&name.id, name.range),
                    object: Self::make_exception_group_expr(
                        Self::make_name_expr(message_ident),
                        Self::make_name_expr(matched_ident),
                        position,
                    ),
                });
            }
            when_matched.extend(handler_body);

            lowered.push(Node::If {
                test: Self::make_builtin_call(
                    BuiltinsFunctions::Len,
                    ArgExprs::One(Self::make_name_expr(matched_ident)),
                    position,
                ),
                body: when_matched,
                or_else: Vec::new(),
            });
        }

        lowered.push(Node::If {
            test: Self::make_builtin_call(
                BuiltinsFunctions::Len,
                ArgExprs::One(Self::make_name_expr(remaining_ident)),
                position,
            ),
            body: vec![Node::Raise(
                Some(Self::make_exception_group_expr(
                    Self::make_name_expr(message_ident),
                    Self::make_name_expr(remaining_ident),
                    position,
                )),
                None,
            )],
            or_else: Vec::new(),
        });

        Ok(lowered)
    }

    /// Creates a synthetic identifier that cannot be shadowed by user source code.
    ///
    /// Names use a dotted prefix (e.g. `.try_star_0`) so they cannot collide
    /// with user-defined identifiers from source code.
    fn synthetic_identifier(&mut self, prefix: &str, position: CodeRange) -> Identifier {
        let name = format!("{prefix}_{}", self.synthetic_name_counter);
        self.synthetic_name_counter = self
            .synthetic_name_counter
            .checked_add(1)
            .expect("synthetic name counter overflow");
        Identifier::new(self.interner.intern(&name), position)
    }

    /// Builds a simple name expression from a prepared identifier.
    fn make_name_expr(ident: Identifier) -> ExprLoc {
        ExprLoc::new(ident.position, Expr::Name(ident))
    }

    /// Builds a binary comparison expression.
    fn make_cmp_expr(left: ExprLoc, op: CmpOperator, right: ExprLoc, position: CodeRange) -> ExprLoc {
        ExprLoc::new(
            position,
            Expr::CmpOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            },
        )
    }

    /// Builds a boolean operation (`and` / `or`) expression.
    fn make_bool_op_expr(left: ExprLoc, op: Operator, right: ExprLoc, position: CodeRange) -> ExprLoc {
        ExprLoc::new(
            position,
            Expr::Op {
                left: Box::new(left),
                op,
                right: Box::new(right),
            },
        )
    }

    /// Builds a builtin-call expression.
    fn make_builtin_call(builtin: BuiltinsFunctions, args: ArgExprs, position: CodeRange) -> ExprLoc {
        ExprLoc::new(
            position,
            Expr::Call {
                callable: Callable::Builtin(Builtins::Function(builtin)),
                args: Box::new(args),
            },
        )
    }

    /// Builds an `ExceptionGroup(message, exceptions)` constructor call expression.
    fn make_exception_group_expr(message: ExprLoc, exceptions: ExprLoc, position: CodeRange) -> ExprLoc {
        ExprLoc::new(
            position,
            Expr::Call {
                callable: Callable::Builtin(Builtins::ExcType(ExcType::ExceptionGroup)),
                args: Box::new(ArgExprs::Two(message, exceptions)),
            },
        )
    }

    /// Lowers a `type Alias = value` statement into a runtime assignment.
    ///
    /// This keeps runtime behavior simple while allowing the parser to accept
    /// PEP 695 type alias syntax used by stdlib typing tests.
    fn parse_type_alias_statement(&mut self, alias: ast::StmtTypeAlias) -> Result<ParseNode, ParseError> {
        let target = self.parse_identifier(*alias.name)?;
        let object = self.parse_expression(*alias.value)?;
        Ok(Node::Assign { target, object })
    }

    /// Parses assignment statements and supports chained targets (`a = b = expr`).
    ///
    /// For chained assignments the right-hand side must be evaluated exactly once.
    /// We lower to a synthetic temporary assignment and then assign each target
    /// from that temporary.
    fn parse_assign_statement(
        &mut self,
        targets: Vec<AstExpr>,
        rhs: AstExpr,
        position: CodeRange,
    ) -> Result<ParseNode, ParseError> {
        if targets.len() == 1 {
            let target = first(targets, position)?;
            return self.parse_assignment(target, rhs);
        }

        let temp_ident = self.synthetic_identifier("<assign_chain>", position);
        let temp_expr = Self::make_name_expr(temp_ident);
        let mut lowered = Vec::with_capacity(targets.len() + 1);

        lowered.push(Node::Assign {
            target: temp_ident,
            object: self.parse_expression(rhs)?,
        });

        for target in targets {
            lowered.push(self.parse_assignment_from_expr(target, temp_expr.clone())?);
        }

        Ok(Node::If {
            test: ExprLoc::new(position, Expr::Literal(Literal::Bool(true))),
            body: lowered,
            or_else: Vec::new(),
        })
    }

    /// Parses an assignment target when the RHS is already parsed.
    ///
    /// This is used by chained-assignment lowering so the RHS expression can be
    /// evaluated once and reused via a synthetic temporary name.
    fn parse_assignment_from_expr(&mut self, lhs: AstExpr, rhs: ExprLoc) -> Result<ParseNode, ParseError> {
        match lhs {
            AstExpr::Subscript(ast::ExprSubscript {
                value, slice, range, ..
            }) => Ok(Node::SubscriptAssign {
                target: self.parse_expression(*value)?,
                index: self.parse_expression(*slice)?,
                value: rhs,
                target_position: self.convert_range(range),
            }),
            AstExpr::Attribute(ast::ExprAttribute { value, attr, range, .. }) => Ok(Node::AttrAssign {
                object: self.parse_expression(*value)?,
                attr: EitherStr::Interned(self.maybe_mangle_name(attr.id())),
                target_position: self.convert_range(range),
                value: rhs,
            }),
            AstExpr::Tuple(ast::ExprTuple { elts, range, .. }) => {
                let targets_position = self.convert_range(range);
                let targets = elts
                    .into_iter()
                    .map(|e| self.parse_unpack_target(e))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Node::UnpackAssign {
                    targets,
                    targets_position,
                    object: rhs,
                })
            }
            AstExpr::List(ast::ExprList { elts, range, .. }) => {
                let targets_position = self.convert_range(range);
                let targets = elts
                    .into_iter()
                    .map(|e| self.parse_unpack_target(e))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Node::UnpackAssign {
                    targets,
                    targets_position,
                    object: rhs,
                })
            }
            _ => Ok(Node::Assign {
                target: self.parse_identifier(lhs)?,
                object: rhs,
            }),
        }
    }

    /// Parses `with ... as target` target syntax.
    ///
    /// `Node::With` and the async-with lowering keep a single optional identifier
    /// target. For tuple/list unpacking we bind to a synthetic temp name and prepend
    /// an unpack assignment to the with-body.
    fn parse_with_as_target(
        &mut self,
        optional_vars: Option<Box<AstExpr>>,
        position: CodeRange,
    ) -> Result<(Option<Identifier>, Vec<ParseNode>), ParseError> {
        match optional_vars {
            Some(v) => match *v {
                AstExpr::Name(ast::ExprName {
                    id, range: name_range, ..
                }) => Ok((Some(self.identifier(&id, name_range)), Vec::new())),
                AstExpr::Tuple(ast::ExprTuple { elts, range, .. }) => {
                    let target = self.synthetic_identifier("<with_as>", position);
                    let targets_position = self.convert_range(range);
                    let targets = elts
                        .into_iter()
                        .map(|expr| self.parse_unpack_target(expr))
                        .collect::<Result<Vec<_>, _>>()?;
                    let unpack = Node::UnpackAssign {
                        targets,
                        targets_position,
                        object: Self::make_name_expr(target),
                    };
                    Ok((Some(target), vec![unpack]))
                }
                AstExpr::List(ast::ExprList { elts, range, .. }) => {
                    let target = self.synthetic_identifier("<with_as>", position);
                    let targets_position = self.convert_range(range);
                    let targets = elts
                        .into_iter()
                        .map(|expr| self.parse_unpack_target(expr))
                        .collect::<Result<Vec<_>, _>>()?;
                    let unpack = Node::UnpackAssign {
                        targets,
                        targets_position,
                        object: Self::make_name_expr(target),
                    };
                    Ok((Some(target), vec![unpack]))
                }
                other => Err(ParseError::not_implemented(
                    "complex with targets",
                    self.convert_range(other.range()),
                )),
            },
            None => Ok((None, Vec::new())),
        }
    }

    /// Lowers `async with` into explicit `await __aenter__` / `await __aexit__` calls.
    ///
    /// The lowering preserves exception suppression semantics and ensures `__aexit__`
    /// runs for normal completion and exceptional exit paths.
    fn lower_async_with_statement(
        &mut self,
        context_expr: ExprLoc,
        var: Option<Identifier>,
        body: Vec<ParseNode>,
        position: CodeRange,
    ) -> Result<ParseNode, ParseError> {
        let manager_ident = self.synthetic_identifier("<async_with_manager>", position);
        let enter_ident = self.synthetic_identifier("<async_with_enter>", position);
        let saw_exception_ident = self.synthetic_identifier("<async_with_saw_exception>", position);
        let exception_ident = self.synthetic_identifier("<async_with_exception>", position);
        let suppress_ident = self.synthetic_identifier("<async_with_suppress>", position);

        let manager_name_expr = Self::make_name_expr(manager_ident);

        let mut lowered = Vec::with_capacity(6);
        lowered.push(Node::Assign {
            target: manager_ident,
            object: context_expr,
        });

        let aenter_call = ExprLoc::new(
            position,
            Expr::AttrCall {
                object: Box::new(manager_name_expr),
                attr: EitherStr::Interned(self.interner.intern("__aenter__")),
                args: Box::new(ArgExprs::Empty),
            },
        );
        lowered.push(Node::Assign {
            target: enter_ident,
            object: ExprLoc::new(position, Expr::Await(Box::new(aenter_call))),
        });

        if let Some(var_target) = var {
            lowered.push(Node::Assign {
                target: var_target,
                object: Self::make_name_expr(enter_ident),
            });
        }

        lowered.push(Node::Assign {
            target: saw_exception_ident,
            object: ExprLoc::new(position, Expr::Literal(Literal::Bool(false))),
        });

        let exception_name_expr = Self::make_name_expr(exception_ident);
        let type_exc_expr = Self::make_builtin_call(
            BuiltinsFunctions::Type,
            ArgExprs::One(exception_name_expr.clone()),
            position,
        );

        let exit_on_exception = ExprLoc::new(
            position,
            Expr::Await(Box::new(ExprLoc::new(
                position,
                Expr::AttrCall {
                    object: Box::new(Self::make_name_expr(manager_ident)),
                    attr: EitherStr::Interned(self.interner.intern("__aexit__")),
                    args: Box::new(ArgExprs::Args(vec![
                        type_exc_expr,
                        exception_name_expr.clone(),
                        ExprLoc::new(position, Expr::Literal(Literal::None)),
                    ])),
                },
            ))),
        );

        let handler_body = vec![
            Node::Assign {
                target: saw_exception_ident,
                object: ExprLoc::new(position, Expr::Literal(Literal::Bool(true))),
            },
            Node::Assign {
                target: suppress_ident,
                object: exit_on_exception,
            },
            Node::If {
                test: ExprLoc::new(position, Expr::Not(Box::new(Self::make_name_expr(suppress_ident)))),
                body: vec![Node::Raise(None, None)],
                or_else: Vec::new(),
            },
        ];

        let exit_on_success = ExprLoc::new(
            position,
            Expr::Await(Box::new(ExprLoc::new(
                position,
                Expr::AttrCall {
                    object: Box::new(Self::make_name_expr(manager_ident)),
                    attr: EitherStr::Interned(self.interner.intern("__aexit__")),
                    args: Box::new(ArgExprs::Args(vec![
                        ExprLoc::new(position, Expr::Literal(Literal::None)),
                        ExprLoc::new(position, Expr::Literal(Literal::None)),
                        ExprLoc::new(position, Expr::Literal(Literal::None)),
                    ])),
                },
            ))),
        );

        lowered.push(Node::Try(Try {
            body,
            handlers: vec![ExceptHandler {
                exc_type: None,
                name: Some(exception_ident),
                body: handler_body,
            }],
            or_else: Vec::new(),
            finally: vec![Node::If {
                test: ExprLoc::new(position, Expr::Not(Box::new(Self::make_name_expr(saw_exception_ident)))),
                body: vec![Node::Expr(exit_on_success)],
                or_else: Vec::new(),
            }],
        }));

        Ok(Node::If {
            test: ExprLoc::new(position, Expr::Literal(Literal::Bool(true))),
            body: lowered,
            or_else: Vec::new(),
        })
    }

    /// Lowers `async for` to the current `for` bytecode path.
    ///
    /// This keeps parsing permissive for parity tests while sharing the same
    /// runtime iteration semantics as regular `for` loops.
    fn lower_async_for_statement(
        &mut self,
        target: AstExpr,
        iter: AstExpr,
        body: Vec<Stmt>,
        orelse: Vec<Stmt>,
        _position: CodeRange,
    ) -> Result<ParseNode, ParseError> {
        Ok(Node::For {
            target: self.parse_unpack_target(target)?,
            iter: self.parse_expression(iter)?,
            body: self.parse_statements(body)?,
            or_else: self.parse_statements(orelse)?,
        })
    }

    /// `lhs = rhs` -> `lhs, rhs`
    /// Handles simple assignments (x = value), subscript assignments (dict[key] = value),
    /// attribute assignments (obj.attr = value), and tuple unpacking (a, b = value)
    fn parse_assignment(&mut self, lhs: AstExpr, rhs: AstExpr) -> Result<ParseNode, ParseError> {
        match lhs {
            // Subscript assignment like dict[key] = value or self.data[key] = value
            AstExpr::Subscript(ast::ExprSubscript {
                value, slice, range, ..
            }) => Ok(Node::SubscriptAssign {
                target: self.parse_expression(*value)?,
                index: self.parse_expression(*slice)?,
                value: self.parse_expression(rhs)?,
                target_position: self.convert_range(range),
            }),
            // Attribute assignment like obj.attr = value (supports chained like n.b.c = value)
            AstExpr::Attribute(ast::ExprAttribute { value, attr, range, .. }) => Ok(Node::AttrAssign {
                object: self.parse_expression(*value)?,
                attr: EitherStr::Interned(self.maybe_mangle_name(attr.id())),
                target_position: self.convert_range(range),
                value: self.parse_expression(rhs)?,
            }),
            // Tuple unpacking like a, b = value or (a, b), c = nested
            AstExpr::Tuple(ast::ExprTuple { elts, range, .. }) => {
                let targets_position = self.convert_range(range);
                let targets = elts
                    .into_iter()
                    .map(|e| self.parse_unpack_target(e)) // Use parse_unpack_target for recursion
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Node::UnpackAssign {
                    targets,
                    targets_position,
                    object: self.parse_expression(rhs)?,
                })
            }
            // List unpacking like [a, b] = value or [a, *rest] = value
            AstExpr::List(ast::ExprList { elts, range, .. }) => {
                let targets_position = self.convert_range(range);
                let targets = elts
                    .into_iter()
                    .map(|e| self.parse_unpack_target(e))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Node::UnpackAssign {
                    targets,
                    targets_position,
                    object: self.parse_expression(rhs)?,
                })
            }
            // Simple identifier assignment like x = value
            _ => Ok(Node::Assign {
                target: self.parse_identifier(lhs)?,
                object: self.parse_expression(rhs)?,
            }),
        }
    }

    /// Parses an expression from the ruff AST into Ouros's ExprLoc representation.
    ///
    /// Includes depth tracking to prevent stack overflow from deeply nested structures.
    /// Matches CPython's limit of 200 for nested parentheses.
    fn parse_expression(&mut self, expression: AstExpr) -> Result<ExprLoc, ParseError> {
        self.decr_depth_remaining(|| expression.range())?;
        let result = self.parse_expression_impl(expression);
        self.depth_remaining += 1;
        result
    }

    fn parse_expression_impl(&mut self, expression: AstExpr) -> Result<ExprLoc, ParseError> {
        match expression {
            AstExpr::BoolOp(ast::ExprBoolOp { op, values, range, .. }) => {
                // Handle chained boolean operations like `a and b and c` by right-folding
                // into nested binary operations: `a and (b and c)`
                let rust_op = convert_bool_op(op);
                let position = self.convert_range(range);
                let mut values_iter = values.into_iter().rev();

                // Start with the rightmost value
                let last_value = values_iter.next().expect("Expected at least one value in boolean op");
                let mut result = self.parse_expression(last_value)?;

                // Fold from right to left
                for value in values_iter {
                    let left = Box::new(self.parse_expression(value)?);
                    result = ExprLoc::new(
                        position,
                        Expr::Op {
                            left,
                            op: rust_op.clone(),
                            right: Box::new(result),
                        },
                    );
                }
                Ok(result)
            }
            AstExpr::Named(ast::ExprNamed {
                target, value, range, ..
            }) => {
                let target_ident = self.parse_identifier(*target)?;
                let value_expr = self.parse_expression(*value)?;
                Ok(ExprLoc::new(
                    self.convert_range(range),
                    Expr::Named {
                        target: target_ident,
                        value: Box::new(value_expr),
                    },
                ))
            }
            AstExpr::BinOp(ast::ExprBinOp {
                left, op, right, range, ..
            }) => {
                let position = self.convert_range(range);
                let left = self.parse_expression(*left)?;
                let right = self.parse_expression(*right)?;
                let op = convert_op(op);

                // Lower `1+2j`/`1-2j` style expressions to a direct complex constructor
                // call so literal forms map to one canonical runtime representation.
                if let Some((real, imag)) = Self::complex_parts_from_real_imag_binop(&left, &op, &right) {
                    return Ok(Self::make_type_call_expr(
                        Type::Complex,
                        ArgExprs::Two(
                            ExprLoc::new(position, Expr::Literal(Literal::Float(real))),
                            ExprLoc::new(position, Expr::Literal(Literal::Float(imag))),
                        ),
                        position,
                    ));
                }

                Ok(ExprLoc {
                    position,
                    expr: Expr::Op {
                        left: Box::new(left),
                        op,
                        right: Box::new(right),
                    },
                })
            }
            AstExpr::UnaryOp(ast::ExprUnaryOp { op, operand, range, .. }) => match op {
                UnaryOp::Not => {
                    let operand = Box::new(self.parse_expression(*operand)?);
                    Ok(ExprLoc::new(self.convert_range(range), Expr::Not(operand)))
                }
                UnaryOp::USub => {
                    let operand = Box::new(self.parse_expression(*operand)?);
                    Ok(ExprLoc::new(self.convert_range(range), Expr::UnaryMinus(operand)))
                }
                UnaryOp::UAdd => {
                    let operand = Box::new(self.parse_expression(*operand)?);
                    Ok(ExprLoc::new(self.convert_range(range), Expr::UnaryPlus(operand)))
                }
                UnaryOp::Invert => {
                    let operand = Box::new(self.parse_expression(*operand)?);
                    Ok(ExprLoc::new(self.convert_range(range), Expr::UnaryInvert(operand)))
                }
            },
            AstExpr::Lambda(ast::ExprLambda {
                parameters,
                body,
                range,
                ..
            }) => {
                let position = self.convert_range(range);

                // Intern the lambda name
                let name_id = self.interner.intern("<lambda>");

                // Parse lambda parameters (similar to function parameters)
                let signature = if let Some(params) = parameters {
                    // Parse positional-only parameters (before /)
                    let pos_args = self.parse_params_with_defaults(&params.posonlyargs)?;

                    // Parse positional-or-keyword parameters
                    let args = self.parse_params_with_defaults(&params.args)?;

                    // Parse *args
                    let var_args = params
                        .vararg
                        .as_ref()
                        .map(|p| self.intern_name_maybe_mangled(&p.name.id));

                    // Parse keyword-only parameters (after * or *args)
                    let kwargs = self.parse_params_with_defaults(&params.kwonlyargs)?;

                    // Parse **kwargs
                    let var_kwargs = params
                        .kwarg
                        .as_ref()
                        .map(|p| self.intern_name_maybe_mangled(&p.name.id));

                    ParsedSignature {
                        pos_args,
                        args,
                        var_args,
                        kwargs,
                        var_kwargs,
                    }
                } else {
                    // No parameters (e.g., `lambda: 42`)
                    ParsedSignature::default()
                };

                // Parse the body expression
                let body = Box::new(self.parse_expression(*body)?);

                Ok(ExprLoc::new(
                    position,
                    Expr::LambdaRaw {
                        name_id,
                        signature,
                        body,
                    },
                ))
            }
            AstExpr::If(ast::ExprIf {
                test,
                body,
                orelse,
                range,
                ..
            }) => Ok(ExprLoc::new(
                self.convert_range(range),
                Expr::IfElse {
                    test: Box::new(self.parse_expression(*test)?),
                    body: Box::new(self.parse_expression(*body)?),
                    orelse: Box::new(self.parse_expression(*orelse)?),
                },
            )),
            AstExpr::Dict(ast::ExprDict { items, range, .. }) => {
                let position = self.convert_range(range);
                let mut dict_items = Vec::new();
                let mut has_unpack = false;
                for ast::DictItem { key, value } in items {
                    if let Some(key_expr_ast) = key {
                        let key_expr = self.parse_expression(key_expr_ast)?;
                        let value_expr = self.parse_expression(value)?;
                        dict_items.push(DictLiteralItem::Pair {
                            key: key_expr,
                            value: value_expr,
                        });
                    } else {
                        has_unpack = true;
                        dict_items.push(DictLiteralItem::Unpack {
                            mapping: self.parse_expression(value)?,
                        });
                    }
                }
                if has_unpack {
                    Ok(ExprLoc::new(position, Expr::DictUnpack(dict_items)))
                } else {
                    let pairs = dict_items
                        .into_iter()
                        .map(|item| match item {
                            DictLiteralItem::Pair { key, value } => (key, value),
                            DictLiteralItem::Unpack { .. } => unreachable!("has_unpack is false"),
                        })
                        .collect();
                    Ok(ExprLoc::new(position, Expr::Dict(pairs)))
                }
            }
            AstExpr::Set(ast::ExprSet { elts, range, .. }) => {
                let position = self.convert_range(range);
                if elts.iter().any(|e| matches!(e, AstExpr::Starred(_))) {
                    self.parse_set_literal_with_unpack(elts, position)
                } else {
                    let elements: Result<Vec<_>, _> = elts.into_iter().map(|e| self.parse_expression(e)).collect();
                    Ok(ExprLoc::new(position, Expr::Set(elements?)))
                }
            }
            AstExpr::ListComp(ast::ExprListComp {
                elt, generators, range, ..
            }) => {
                let elt = Box::new(self.parse_expression(*elt)?);
                let generators = self.parse_comprehension_generators(generators)?;
                Ok(ExprLoc::new(
                    self.convert_range(range),
                    Expr::ListComp { elt, generators },
                ))
            }
            AstExpr::SetComp(ast::ExprSetComp {
                elt, generators, range, ..
            }) => {
                let elt = Box::new(self.parse_expression(*elt)?);
                let generators = self.parse_comprehension_generators(generators)?;
                Ok(ExprLoc::new(
                    self.convert_range(range),
                    Expr::SetComp { elt, generators },
                ))
            }
            AstExpr::DictComp(ast::ExprDictComp {
                key,
                value,
                generators,
                range,
                ..
            }) => {
                let key = Box::new(self.parse_expression(*key)?);
                let value = Box::new(self.parse_expression(*value)?);
                let generators = self.parse_comprehension_generators(generators)?;
                Ok(ExprLoc::new(
                    self.convert_range(range),
                    Expr::DictComp { key, value, generators },
                ))
            }
            AstExpr::Generator(ast::ExprGenerator {
                elt, generators, range, ..
            }) => {
                let elt = Box::new(self.parse_expression(*elt)?);
                let generators = self.parse_comprehension_generators(generators)?;
                let iter_arg_name_id = self.interner.intern(".0");
                let genexpr_name_id = self.interner.intern("<genexpr>");
                Ok(ExprLoc::new(
                    self.convert_range(range),
                    Expr::GeneratorExpRaw {
                        elt,
                        generators,
                        iter_arg_name_id,
                        genexpr_name_id,
                    },
                ))
            }
            AstExpr::Await(a) => {
                let value = self.parse_expression(*a.value)?;
                Ok(ExprLoc::new(self.convert_range(a.range), Expr::Await(Box::new(value))))
            }
            AstExpr::Yield(y) => {
                let value = match y.value {
                    Some(v) => Some(Box::new(self.parse_expression(*v)?)),
                    None => None,
                };
                Ok(ExprLoc::new(self.convert_range(y.range), Expr::Yield { value }))
            }
            AstExpr::YieldFrom(y) => {
                let value = Box::new(self.parse_expression(*y.value)?);
                Ok(ExprLoc::new(self.convert_range(y.range), Expr::YieldFrom { value }))
            }
            AstExpr::Compare(ast::ExprCompare {
                left,
                ops,
                comparators,
                range,
                ..
            }) => {
                let position = self.convert_range(range);
                let ops_vec = ops.into_vec();
                let comparators_vec = comparators.into_vec();

                // Simple case: single comparison (most common)
                if ops_vec.len() == 1 {
                    return Ok(ExprLoc::new(
                        position,
                        Expr::CmpOp {
                            left: Box::new(self.parse_expression(*left)?),
                            op: convert_compare_op(ops_vec.into_iter().next().unwrap()),
                            right: Box::new(self.parse_expression(comparators_vec.into_iter().next().unwrap())?),
                        },
                    ));
                }

                // Chain comparison: transform to nested And expressions
                self.parse_chain_comparison(*left, ops_vec, comparators_vec, position)
            }
            AstExpr::Call(ast::ExprCall {
                func, arguments, range, ..
            }) => {
                let position = self.convert_range(range);
                let ast::Arguments { args, keywords, .. } = arguments;
                let mut positional_args = Vec::new();
                let mut trailing_positional_args = Vec::new();
                let mut var_args_expr: Option<ExprLoc> = None;
                let mut seen_star = false;

                for arg_expr in args.into_vec() {
                    match arg_expr {
                        AstExpr::Starred(ast::ExprStarred { value, .. }) => {
                            if var_args_expr.is_some() {
                                return Err(ParseError::not_implemented("multiple *args unpacking", position));
                            }
                            var_args_expr = Some(self.parse_expression(*value)?);
                            seen_star = true;
                        }
                        other => {
                            if seen_star {
                                trailing_positional_args.push(self.parse_expression(other)?);
                            } else {
                                positional_args.push(self.parse_expression(other)?);
                            }
                        }
                    }
                }
                if let Some(var_args) = var_args_expr.take() {
                    if trailing_positional_args.is_empty() {
                        var_args_expr = Some(var_args);
                    } else {
                        let unpacked_tuple = Self::make_type_call_expr(Type::Tuple, ArgExprs::One(var_args), position);
                        let trailing_tuple = ExprLoc::new(position, Expr::Tuple(trailing_positional_args));
                        var_args_expr = Some(ExprLoc::new(
                            position,
                            Expr::Op {
                                left: Box::new(unpacked_tuple),
                                op: Operator::Add,
                                right: Box::new(trailing_tuple),
                            },
                        ));
                    }
                } else if !trailing_positional_args.is_empty() {
                    positional_args.extend(trailing_positional_args);
                }
                // Separate regular kwargs (key=value) from var_kwargs (**expr)
                let (kwargs, var_kwargs) = self.parse_keywords(keywords.into_vec())?;
                let args = ArgExprs::new_with_var_kwargs(positional_args, var_args_expr, kwargs, var_kwargs);
                match *func {
                    AstExpr::Name(ast::ExprName { id, range, .. }) => {
                        // Name is resolved in prepare phase so local/global bindings
                        // can shadow builtins following LEGB lookup order.
                        let ident = self.identifier(&id, range);
                        let callable = Callable::Name(ident);
                        Ok(ExprLoc::new(
                            position,
                            Expr::Call {
                                callable,
                                args: Box::new(args),
                            },
                        ))
                    }
                    AstExpr::Attribute(ast::ExprAttribute { value, attr, .. }) => {
                        let object = Box::new(self.parse_expression(*value)?);
                        Ok(ExprLoc::new(
                            position,
                            Expr::AttrCall {
                                object,
                                attr: EitherStr::Interned(self.maybe_mangle_name(attr.id())),
                                args: Box::new(args),
                            },
                        ))
                    }
                    other => {
                        // Handle arbitrary expression as callable (e.g., lambda calls)
                        let callable = Box::new(self.parse_expression(other)?);
                        Ok(ExprLoc::new(
                            position,
                            Expr::IndirectCall {
                                callable,
                                args: Box::new(args),
                            },
                        ))
                    }
                }
            }
            AstExpr::FString(ast::ExprFString { value, range, .. }) => self.parse_fstring(&value, range),
            AstExpr::TString(t) => Err(ParseError::not_implemented(
                "template strings (t-strings)",
                self.convert_range(t.range),
            )),
            AstExpr::StringLiteral(ast::ExprStringLiteral { value, range, .. }) => {
                let string_id = self.interner.intern(&value.to_string());
                Ok(ExprLoc::new(
                    self.convert_range(range),
                    Expr::Literal(Literal::Str(string_id)),
                ))
            }
            AstExpr::BytesLiteral(ast::ExprBytesLiteral { value, range, .. }) => {
                let bytes: Cow<'_, [u8]> = Cow::from(&value);
                let bytes_id = self.interner.intern_bytes(&bytes);
                Ok(ExprLoc::new(
                    self.convert_range(range),
                    Expr::Literal(Literal::Bytes(bytes_id)),
                ))
            }
            AstExpr::NumberLiteral(ast::ExprNumberLiteral { value, range, .. }) => {
                let position = self.convert_range(range);
                let const_value = match value {
                    Number::Int(i) => {
                        if let Some(i) = i.as_i64() {
                            Literal::Int(i)
                        } else {
                            // Integer too large for i64, parse string representation as BigInt
                            // Handles radix prefixes (0x, 0o, 0b) and underscores
                            let bi = parse_int_literal(&i.to_string())
                                .ok_or_else(|| ParseError::syntax(format!("invalid integer literal: {i}"), position))?;
                            let long_int_id = self.interner.intern_long_int(bi);
                            Literal::LongInt(long_int_id)
                        }
                    }
                    Number::Float(f) => Literal::Float(f),
                    Number::Complex { real, imag } => {
                        return Ok(Self::make_type_call_expr(
                            Type::Complex,
                            ArgExprs::Two(
                                ExprLoc::new(position, Expr::Literal(Literal::Float(real))),
                                ExprLoc::new(position, Expr::Literal(Literal::Float(imag))),
                            ),
                            position,
                        ));
                    }
                };
                Ok(ExprLoc::new(position, Expr::Literal(const_value)))
            }
            AstExpr::BooleanLiteral(ast::ExprBooleanLiteral { value, range, .. }) => Ok(ExprLoc::new(
                self.convert_range(range),
                Expr::Literal(Literal::Bool(value)),
            )),
            AstExpr::NoneLiteral(ast::ExprNoneLiteral { range, .. }) => {
                Ok(ExprLoc::new(self.convert_range(range), Expr::Literal(Literal::None)))
            }
            AstExpr::EllipsisLiteral(ast::ExprEllipsisLiteral { range, .. }) => Ok(ExprLoc::new(
                self.convert_range(range),
                Expr::Literal(Literal::Ellipsis),
            )),
            AstExpr::Attribute(ast::ExprAttribute { value, attr, range, .. }) => {
                let object = Box::new(self.parse_expression(*value)?);
                let position = self.convert_range(range);
                Ok(ExprLoc::new(
                    position,
                    Expr::AttrGet {
                        object,
                        attr: EitherStr::Interned(self.maybe_mangle_name(attr.id())),
                    },
                ))
            }
            AstExpr::Subscript(ast::ExprSubscript {
                value, slice, range, ..
            }) => {
                let object = Box::new(self.parse_expression(*value)?);
                let index = Box::new(self.parse_expression(*slice)?);
                Ok(ExprLoc::new(
                    self.convert_range(range),
                    Expr::Subscript { object, index },
                ))
            }
            AstExpr::Starred(s) => Err(ParseError::not_implemented(
                "starred expressions (*expr)",
                self.convert_range(s.range),
            )),
            AstExpr::Name(ast::ExprName { id, range, .. }) => {
                let name = id.to_string();
                let position = self.convert_range(range);
                // Keep NotImplemented as a dedicated constant expression.
                let expr = if name == "NotImplemented" {
                    Expr::NotImplemented
                } else {
                    Expr::Name(self.identifier(&id, range))
                };
                Ok(ExprLoc::new(position, expr))
            }
            AstExpr::List(ast::ExprList { elts, range, .. }) => {
                let position = self.convert_range(range);
                if elts.iter().any(|e| matches!(e, AstExpr::Starred(_))) {
                    self.parse_list_literal_with_unpack(elts, position)
                } else {
                    let items = elts
                        .into_iter()
                        .map(|f| self.parse_expression(f))
                        .collect::<Result<_, ParseError>>()?;
                    Ok(ExprLoc::new(position, Expr::List(items)))
                }
            }
            AstExpr::Tuple(ast::ExprTuple { elts, range, .. }) => {
                let position = self.convert_range(range);
                if elts.iter().any(|e| matches!(e, AstExpr::Starred(_))) {
                    self.parse_tuple_literal_with_unpack(elts, position)
                } else {
                    let items = elts
                        .into_iter()
                        .map(|f| self.parse_expression(f))
                        .collect::<Result<_, ParseError>>()?;
                    Ok(ExprLoc::new(position, Expr::Tuple(items)))
                }
            }
            AstExpr::Slice(ast::ExprSlice {
                lower,
                upper,
                step,
                range,
                ..
            }) => {
                let lower = lower.map(|e| self.parse_expression(*e)).transpose()?;
                let upper = upper.map(|e| self.parse_expression(*e)).transpose()?;
                let step = step.map(|e| self.parse_expression(*e)).transpose()?;
                Ok(ExprLoc::new(
                    self.convert_range(range),
                    Expr::Slice {
                        lower: lower.map(Box::new),
                        upper: upper.map(Box::new),
                        step: step.map(Box::new),
                    },
                ))
            }
            AstExpr::IpyEscapeCommand(i) => Err(ParseError::not_implemented(
                "IPython escape commands",
                self.convert_range(i.range),
            )),
        }
    }

    /// Parses keyword arguments, separating regular kwargs from var_kwargs (`**expr`).
    ///
    /// Returns `(kwargs, var_kwargs)` where kwargs is a vec of named keyword arguments
    /// and var_kwargs is an optional expression for `**expr` unpacking.
    fn parse_keywords(&mut self, keywords: Vec<Keyword>) -> Result<(Vec<Kwarg>, Option<ExprLoc>), ParseError> {
        let mut kwargs = Vec::new();
        let mut var_kwargs = None;

        for kwarg in keywords {
            if let Some(key) = kwarg.arg {
                // Regular kwarg: key=value
                let key = self.identifier_raw(&key.id, key.range);
                let value = self.parse_expression(kwarg.value)?;
                kwargs.push(Kwarg { key, value });
            } else {
                // Var kwargs: **expr
                if var_kwargs.is_some() {
                    return Err(ParseError::not_implemented(
                        "multiple **kwargs unpacking",
                        self.convert_range(kwarg.range),
                    ));
                }
                var_kwargs = Some(self.parse_expression(kwarg.value)?);
            }
        }

        Ok((kwargs, var_kwargs))
    }

    /// Builds a call expression for a builtin type constructor.
    fn make_type_call_expr(ty: Type, args: ArgExprs, position: CodeRange) -> ExprLoc {
        ExprLoc::new(
            position,
            Expr::Call {
                callable: Callable::Builtin(Builtins::Type(ty)),
                args: Box::new(args),
            },
        )
    }

    /// Extracts `(real, imag)` parts from minimal `real ± imag*j` binops.
    ///
    /// This recognizes the internal lowered form where imaginary literals become
    /// `complex(0.0, imag)` calls and rewrites:
    /// - `real + imag*j` -> `complex(real, imag)`
    /// - `real - imag*j` -> `complex(real, -imag)`
    /// - `imag*j + real` -> `complex(real, imag)`
    /// - `imag*j - real` -> `complex(-real, imag)`
    fn complex_parts_from_real_imag_binop(left: &ExprLoc, op: &Operator, right: &ExprLoc) -> Option<(f64, f64)> {
        if matches!(op, Operator::Add | Operator::Sub) {
            if let Some(real) = Self::extract_real_literal_value(left)
                && let Some((imag_real, imag)) = Self::extract_complex_call_parts(right)
                && imag_real == 0.0
            {
                let imag = if matches!(op, Operator::Add) { imag } else { -imag };
                return Some((real, imag));
            }
            if let Some((imag_real, imag)) = Self::extract_complex_call_parts(left)
                && imag_real == 0.0
                && let Some(real) = Self::extract_real_literal_value(right)
            {
                return if matches!(op, Operator::Add) {
                    Some((real, imag))
                } else {
                    Some((-real, imag))
                };
            }
        }
        None
    }

    /// Extracts a numeric literal from an expression as `f64`.
    fn extract_real_literal_value(expr: &ExprLoc) -> Option<f64> {
        match &expr.expr {
            Expr::Literal(Literal::Int(v)) => Some(*v as f64),
            Expr::Literal(Literal::Float(v)) => Some(*v),
            _ => None,
        }
    }

    /// Extracts `(real, imag)` from lowered `complex(real, imag)` calls.
    fn extract_complex_call_parts(expr: &ExprLoc) -> Option<(f64, f64)> {
        let Expr::Call { callable, args } = &expr.expr else {
            return None;
        };
        let Callable::Builtin(Builtins::Type(Type::Complex)) = callable else {
            return None;
        };
        let ArgExprs::Two(real, imag) = args.as_ref() else {
            return None;
        };
        let real = Self::extract_real_literal_value(real)?;
        let imag = Self::extract_real_literal_value(imag)?;
        Some((real, imag))
    }

    /// Folds a sequence of expression terms into left-associative binary operations.
    fn fold_expr_terms(terms: Vec<ExprLoc>, op: Operator, empty: ExprLoc, position: CodeRange) -> ExprLoc {
        let mut terms_iter = terms.into_iter();
        let Some(mut acc) = terms_iter.next() else {
            return empty;
        };
        for term in terms_iter {
            acc = ExprLoc::new(
                position,
                Expr::Op {
                    left: Box::new(acc),
                    op: op.clone(),
                    right: Box::new(term),
                },
            );
        }
        acc
    }

    /// Lowers list literals with unpacking (`[a, *it, b]`) into list concatenations.
    fn parse_list_literal_with_unpack(
        &mut self,
        elts: Vec<AstExpr>,
        position: CodeRange,
    ) -> Result<ExprLoc, ParseError> {
        let mut terms = Vec::with_capacity(elts.len());
        for elt in elts {
            match elt {
                AstExpr::Starred(ast::ExprStarred { value, .. }) => {
                    let iterable = self.parse_expression(*value)?;
                    terms.push(Self::make_type_call_expr(Type::List, ArgExprs::One(iterable), position));
                }
                other => {
                    let element = self.parse_expression(other)?;
                    terms.push(ExprLoc::new(position, Expr::List(vec![element])));
                }
            }
        }

        Ok(Self::fold_expr_terms(
            terms,
            Operator::Add,
            ExprLoc::new(position, Expr::List(Vec::new())),
            position,
        ))
    }

    /// Lowers tuple literals with unpacking (`(*a, 1, *b)`) via list lowering + tuple conversion.
    fn parse_tuple_literal_with_unpack(
        &mut self,
        elts: Vec<AstExpr>,
        position: CodeRange,
    ) -> Result<ExprLoc, ParseError> {
        let list_expr = self.parse_list_literal_with_unpack(elts, position)?;
        Ok(Self::make_type_call_expr(
            Type::Tuple,
            ArgExprs::One(list_expr),
            position,
        ))
    }

    /// Lowers set literals with unpacking (`{1, *a, 2, *b}`) into set unions.
    fn parse_set_literal_with_unpack(
        &mut self,
        elts: Vec<AstExpr>,
        position: CodeRange,
    ) -> Result<ExprLoc, ParseError> {
        let mut terms = Vec::with_capacity(elts.len());
        for elt in elts {
            match elt {
                AstExpr::Starred(ast::ExprStarred { value, .. }) => {
                    let iterable = self.parse_expression(*value)?;
                    terms.push(Self::make_type_call_expr(Type::Set, ArgExprs::One(iterable), position));
                }
                other => {
                    let element = self.parse_expression(other)?;
                    terms.push(ExprLoc::new(position, Expr::Set(vec![element])));
                }
            }
        }

        Ok(Self::fold_expr_terms(
            terms,
            Operator::BitOr,
            ExprLoc::new(position, Expr::Set(Vec::new())),
            position,
        ))
    }

    fn parse_identifier(&mut self, ast: AstExpr) -> Result<Identifier, ParseError> {
        match ast {
            AstExpr::Name(ast::ExprName { id, range, .. }) => Ok(self.identifier(&id, range)),
            other => Err(ParseError::syntax(
                format!("Expected name, got {other:?}"),
                self.convert_range(other.range()),
            )),
        }
    }

    /// Parses PEP 695 type parameters into a list of interned names.
    ///
    /// Type parameters are stored as names only; bounds/defaults are not evaluated yet.
    fn parse_type_params(&mut self, type_params: Option<Box<ast::TypeParams>>) -> Vec<StringId> {
        let Some(type_params) = type_params else {
            return Vec::new();
        };
        type_params
            .type_params
            .iter()
            .map(|param| self.interner.intern(param.name().id()))
            .collect()
    }

    /// Parses a chain comparison expression like `a < b < c < d`.
    ///
    /// Chain comparisons evaluate each intermediate value only once and short-circuit
    /// on the first false result. This creates an `Expr::ChainCmp` node which is
    /// compiled to bytecode using stack manipulation (Dup, Rot) rather than
    /// temporary variables, avoiding namespace pollution.
    fn parse_chain_comparison(
        &mut self,
        left: AstExpr,
        ops: Vec<CmpOp>,
        comparators: Vec<AstExpr>,
        position: CodeRange,
    ) -> Result<ExprLoc, ParseError> {
        let left_expr = self.parse_expression(left)?;
        let comparisons = ops
            .into_iter()
            .zip(comparators)
            .map(|(op, cmp)| Ok((convert_compare_op(op), self.parse_expression(cmp)?)))
            .collect::<Result<Vec<_>, ParseError>>()?;

        Ok(ExprLoc::new(
            position,
            Expr::ChainCmp {
                left: Box::new(left_expr),
                comparisons,
            },
        ))
    }

    /// Parses an unpack target - a name, subscript, or nested tuple/list.
    ///
    /// Handles patterns like `a` (single variable), `a[i]` (subscript target),
    /// `a, b` (flat tuple), or `(a, b), c` (nested).
    /// Includes depth tracking to prevent stack overflow from deeply nested structures.
    fn parse_unpack_target(&mut self, ast: AstExpr) -> Result<UnpackTarget, ParseError> {
        self.decr_depth_remaining(|| ast.range())?;
        let result = self.parse_unpack_target_impl(ast);
        self.depth_remaining += 1;
        result
    }

    fn parse_unpack_target_impl(&mut self, ast: AstExpr) -> Result<UnpackTarget, ParseError> {
        match ast {
            AstExpr::Name(ast::ExprName { id, range, .. }) => Ok(UnpackTarget::Name(self.identifier(&id, range))),
            AstExpr::Subscript(ast::ExprSubscript {
                value, slice, range, ..
            }) => Ok(UnpackTarget::Subscript {
                target: Box::new(self.parse_expression(*value)?),
                index: Box::new(self.parse_expression(*slice)?),
                target_position: self.convert_range(range),
            }),
            AstExpr::Tuple(ast::ExprTuple { elts, range, .. }) => {
                let position = self.convert_range(range);
                let targets = elts
                    .into_iter()
                    .map(|e| self.parse_unpack_target(e))
                    .collect::<Result<Vec<_>, _>>()?;
                if targets.is_empty() {
                    return Err(ParseError::syntax("empty tuple in unpack target", position));
                }
                // Validate at most one starred target
                let starred_count = targets.iter().filter(|t| matches!(t, UnpackTarget::Starred(_))).count();
                if starred_count > 1 {
                    return Err(ParseError::syntax(
                        "multiple starred expressions in assignment",
                        position,
                    ));
                }
                Ok(UnpackTarget::Tuple { targets, position })
            }
            AstExpr::Starred(ast::ExprStarred { value, range, .. }) => {
                // Starred target must be a simple name
                match *value {
                    AstExpr::Name(ast::ExprName { id, range, .. }) => {
                        Ok(UnpackTarget::Starred(self.identifier(&id, range)))
                    }
                    _ => Err(ParseError::syntax(
                        "starred assignment target must be a name",
                        self.convert_range(range),
                    )),
                }
            }
            AstExpr::List(ast::ExprList { elts, range, .. }) => {
                // List unpacking target [a, b, *rest] - same as tuple
                let position = self.convert_range(range);
                let targets = elts
                    .into_iter()
                    .map(|e| self.parse_unpack_target(e))
                    .collect::<Result<Vec<_>, _>>()?;
                if targets.is_empty() {
                    return Err(ParseError::syntax("empty list in unpack target", position));
                }
                // Validate at most one starred target
                let starred_count = targets.iter().filter(|t| matches!(t, UnpackTarget::Starred(_))).count();
                if starred_count > 1 {
                    return Err(ParseError::syntax(
                        "multiple starred expressions in assignment",
                        position,
                    ));
                }
                Ok(UnpackTarget::Tuple { targets, position })
            }
            other => Err(ParseError::syntax(
                format!("invalid unpacking target: {other:?}"),
                self.convert_range(other.range()),
            )),
        }
    }

    fn identifier(&mut self, id: &Name, range: TextRange) -> Identifier {
        let string_id = self.intern_name_maybe_mangled(id);
        Identifier::new(string_id, self.convert_range(range))
    }

    /// Builds an identifier without applying name mangling.
    ///
    /// This is used when we need the raw source name (e.g. for error messages
    /// or internal labels) rather than the class-private mangled form.
    fn identifier_raw(&mut self, id: &Name, range: TextRange) -> Identifier {
        let string_id = self.interner.intern(id);
        Identifier::new(string_id, self.convert_range(range))
    }

    /// Interns a name, applying class-private mangling when appropriate.
    ///
    /// This mirrors CPython's behavior for `__private` names inside class bodies.
    fn intern_name_maybe_mangled(&mut self, id: &Name) -> StringId {
        self.maybe_mangle_name(id.as_str())
    }

    /// Returns the interned name, with class-private mangling if applicable.
    ///
    /// Mangling only applies inside class bodies and only for `__name`-style
    /// identifiers that are not dunder magic methods.
    fn maybe_mangle_name(&mut self, name: &str) -> StringId {
        let Some(class_name_id) = self.class_stack.last().copied() else {
            return self.interner.intern(name);
        };

        if !is_mangling_candidate(name) {
            return self.interner.intern(name);
        }

        let class_name = self.interner.get_str(class_name_id);
        let stripped = class_name.trim_start_matches('_');
        if stripped.is_empty() {
            return self.interner.intern(name);
        }

        let mut mangled = String::with_capacity(1 + stripped.len() + name.len());
        mangled.push('_');
        mangled.push_str(stripped);
        mangled.push_str(name);
        self.interner.intern(&mangled)
    }

    /// Parses function parameters with optional default values.
    ///
    /// Handles parameters like `a`, `b=10`, `c=None` by extracting the parameter
    /// name and parsing any default expression. Default expressions are stored
    /// as unevaluated AST and will be evaluated during the prepare phase.
    fn parse_params_with_defaults(&mut self, params: &[ParameterWithDefault]) -> Result<Vec<ParsedParam>, ParseError> {
        params
            .iter()
            .map(|p| {
                let name = self.intern_name_maybe_mangled(&p.parameter.name.id);
                let default = match &p.default {
                    Some(expr) => Some(self.parse_expression((**expr).clone())?),
                    None => None,
                };
                let annotation = match &p.parameter.annotation {
                    Some(expr) => Some(self.parse_expression((**expr).clone())?),
                    None => None,
                };
                Ok(ParsedParam {
                    name,
                    default,
                    annotation,
                })
            })
            .collect()
    }

    /// Parses comprehension generators (the `for ... in ... if ...` clauses).
    ///
    /// Each generator represents one `for` clause with zero or more `if` filters.
    /// Multiple generators create nested iteration. Supports both single identifiers
    /// (`for x in ...`) and tuple unpacking (`for x, y in ...`).
    fn parse_comprehension_generators(
        &mut self,
        generators: Vec<ast::Comprehension>,
    ) -> Result<Vec<Comprehension>, ParseError> {
        generators
            .into_iter()
            .map(|comp| {
                if comp.is_async {
                    return Err(ParseError::not_implemented(
                        "async comprehensions",
                        self.convert_range(comp.range),
                    ));
                }
                let target = self.parse_unpack_target(comp.target)?;
                let iter = self.parse_expression(comp.iter)?;
                let ifs = comp
                    .ifs
                    .into_iter()
                    .map(|cond| self.parse_expression(cond))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Comprehension { target, iter, ifs })
            })
            .collect()
    }

    /// Parses an f-string value into expression parts.
    ///
    /// F-strings in ruff AST are represented as `FStringValue` containing
    /// `FStringPart`s, which can be either literal strings or `FString`
    /// interpolated sections. Each `FString` contains `InterpolatedStringElements`.
    fn parse_fstring(&mut self, value: &ast::FStringValue, range: TextRange) -> Result<ExprLoc, ParseError> {
        let mut parts = Vec::new();

        for fstring_part in value {
            match fstring_part {
                ast::FStringPart::Literal(lit) => {
                    // Literal string segment - intern for use at runtime
                    let processed = lit.value.to_string();
                    if !processed.is_empty() {
                        let string_id = self.interner.intern(&processed);
                        parts.push(FStringPart::Literal(string_id));
                    }
                }
                ast::FStringPart::FString(fstring) => {
                    // Interpolated f-string section
                    for element in &fstring.elements {
                        let part = self.parse_fstring_element(element)?;
                        parts.push(part);
                    }
                }
            }
        }

        // Optimization: if only one literal part, return as simple string literal
        if parts.len() == 1
            && let FStringPart::Literal(string_id) = parts[0]
        {
            return Ok(ExprLoc::new(
                self.convert_range(range),
                Expr::Literal(Literal::Str(string_id)),
            ));
        }

        Ok(ExprLoc::new(self.convert_range(range), Expr::FString(parts)))
    }

    /// Parses a single f-string element (literal or interpolation).
    fn parse_fstring_element(&mut self, element: &InterpolatedStringElement) -> Result<FStringPart, ParseError> {
        match element {
            InterpolatedStringElement::Literal(lit) => {
                // Intern the literal string
                let processed = lit.value.to_string();
                let string_id = self.interner.intern(&processed);
                Ok(FStringPart::Literal(string_id))
            }
            InterpolatedStringElement::Interpolation(interp) => {
                let expr = Box::new(self.parse_expression((*interp.expression).clone())?);
                let conversion = convert_conversion_flag(interp.conversion);
                // Format specs within format specs are not allowed in Python,
                // and debug_prefix doesn't apply to nested interpolations
                let format_spec = match &interp.format_spec {
                    Some(spec) => Some(self.parse_format_spec(spec)?),
                    None => None,
                };
                // Extract debug prefix for `=` specifier (e.g., f'{a=}' -> "a=")
                let debug_prefix = interp.debug_text.as_ref().map(|dt| {
                    let expr_text = &self.code[interp.expression.range()];
                    self.interner
                        .intern(&format!("{}{}{}", dt.leading, expr_text, dt.trailing))
                });
                Ok(FStringPart::Interpolation {
                    expr,
                    conversion,
                    format_spec,
                    debug_prefix,
                })
            }
        }
    }

    /// Parses a format specification, which may contain nested interpolations.
    ///
    /// For static specs (no interpolations), parses the format string into a
    /// `ParsedFormatSpec` at parse time to avoid runtime parsing overhead.
    fn parse_format_spec(&mut self, spec: &ast::InterpolatedStringFormatSpec) -> Result<FormatSpec, ParseError> {
        let mut parts = Vec::new();
        let mut has_interpolation = false;

        for element in &spec.elements {
            match element {
                InterpolatedStringElement::Literal(lit) => {
                    // Intern the literal string
                    let processed = lit.value.to_string();
                    let string_id = self.interner.intern(&processed);
                    parts.push(FStringPart::Literal(string_id));
                }
                InterpolatedStringElement::Interpolation(interp) => {
                    has_interpolation = true;
                    let expr = Box::new(self.parse_expression((*interp.expression).clone())?);
                    let conversion = convert_conversion_flag(interp.conversion);
                    // Format specs within format specs are not allowed in Python,
                    // and debug_prefix doesn't apply to nested interpolations
                    parts.push(FStringPart::Interpolation {
                        expr,
                        conversion,
                        format_spec: None,
                        debug_prefix: None,
                    });
                }
            }
        }

        if has_interpolation {
            Ok(FormatSpec::Dynamic(parts))
        } else {
            // Combine all literal parts into a single static string and parse at parse time
            let static_spec: String = parts
                .into_iter()
                .filter_map(|p| {
                    if let FStringPart::Literal(string_id) = p {
                        Some(self.interner.get_str(string_id).to_owned())
                    } else {
                        None
                    }
                })
                .collect();
            let parsed = static_spec.parse().map_err(|spec_str| {
                ParseError::syntax(
                    format!("Invalid format specifier '{spec_str}'"),
                    self.convert_range(spec.range),
                )
            })?;
            let raw = self.interner.intern(&static_spec);
            Ok(FormatSpec::Static { parsed, raw })
        }
    }

    fn convert_range(&self, range: TextRange) -> CodeRange {
        let start = range.start().into();
        let (start_line_no, start_line_start, _) = self.index_to_position(start);
        let start = CodeLoc::new(start_line_no, start - start_line_start);

        let end = range.end().into();
        let (end_line_no, end_line_start, _) = self.index_to_position(end);
        let end = CodeLoc::new(end_line_no, end - end_line_start);

        // Store line number for single-line ranges, None for multi-line
        let preview_line = if start_line_no == end_line_no {
            Some(u32::try_from(start_line_no).expect("line number exceeds u32"))
        } else {
            None
        };

        CodeRange::new(self.filename_id, start, end, preview_line)
    }

    fn index_to_position(&self, index: usize) -> (usize, usize, Option<usize>) {
        let mut line_start = 0;
        for (line_no, line_end) in self.line_ends.iter().enumerate() {
            if index <= *line_end {
                return (line_no, line_start, Some(*line_end));
            }
            line_start = *line_end + 1;
        }
        // Content after the last newline (file without trailing newline)
        // line_ends.len() gives the correct 0-indexed line number
        (self.line_ends.len(), line_start, None)
    }

    /// Decrements the depth remaining for nested parentheses.
    /// Returns an error if the depth remaining goes to zero.
    fn decr_depth_remaining(&mut self, get_range: impl FnOnce() -> TextRange) -> Result<(), ParseError> {
        if let Some(depth_remaining) = self.depth_remaining.checked_sub(1) {
            self.depth_remaining = depth_remaining;
            Ok(())
        } else {
            let position = self.convert_range(get_range());
            Err(ParseError::syntax("too many nested parentheses", position))
        }
    }
}

fn first<T: fmt::Debug>(v: Vec<T>, position: CodeRange) -> Result<T, ParseError> {
    if v.len() == 1 {
        v.into_iter()
            .next()
            .ok_or_else(|| ParseError::syntax("Expected 1 element, got 0", position))
    } else {
        Err(ParseError::syntax(
            format!("Expected 1 element, got {} (raw: {v:?})", v.len()),
            position,
        ))
    }
}

fn convert_op(op: AstOperator) -> Operator {
    match op {
        AstOperator::Add => Operator::Add,
        AstOperator::Sub => Operator::Sub,
        AstOperator::Mult => Operator::Mult,
        AstOperator::MatMult => Operator::MatMult,
        AstOperator::Div => Operator::Div,
        AstOperator::Mod => Operator::Mod,
        AstOperator::Pow => Operator::Pow,
        AstOperator::LShift => Operator::LShift,
        AstOperator::RShift => Operator::RShift,
        AstOperator::BitOr => Operator::BitOr,
        AstOperator::BitXor => Operator::BitXor,
        AstOperator::BitAnd => Operator::BitAnd,
        AstOperator::FloorDiv => Operator::FloorDiv,
    }
}

fn convert_bool_op(op: BoolOp) -> Operator {
    match op {
        BoolOp::And => Operator::And,
        BoolOp::Or => Operator::Or,
    }
}

fn convert_compare_op(op: CmpOp) -> CmpOperator {
    match op {
        CmpOp::Eq => CmpOperator::Eq,
        CmpOp::NotEq => CmpOperator::NotEq,
        CmpOp::Lt => CmpOperator::Lt,
        CmpOp::LtE => CmpOperator::LtE,
        CmpOp::Gt => CmpOperator::Gt,
        CmpOp::GtE => CmpOperator::GtE,
        CmpOp::Is => CmpOperator::Is,
        CmpOp::IsNot => CmpOperator::IsNot,
        CmpOp::In => CmpOperator::In,
        CmpOp::NotIn => CmpOperator::NotIn,
    }
}

/// Converts ruff's ConversionFlag to our ConversionFlag.
fn convert_conversion_flag(flag: RuffConversionFlag) -> ConversionFlag {
    match flag {
        RuffConversionFlag::None => ConversionFlag::None,
        RuffConversionFlag::Str => ConversionFlag::Str,
        RuffConversionFlag::Repr => ConversionFlag::Repr,
        RuffConversionFlag::Ascii => ConversionFlag::Ascii,
    }
}

/// Source code location information for error reporting.
///
/// Contains filename (as StringId), line/column positions, and optionally a line number for
/// extracting the preview line from source during traceback formatting.
///
/// To display the filename, the caller must provide access to the string storage.
#[derive(Clone, Copy, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CodeRange {
    /// Interned filename ID - look up in Interns to get the actual string.
    pub filename: StringId,
    /// Line number (0-indexed) for extracting preview from source. None if range spans multiple lines.
    preview_line: Option<u32>,
    start: CodeLoc,
    end: CodeLoc,
}

/// Custom Debug implementation to make displaying code much less verbose.
impl fmt::Debug for CodeRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CodeRange{{filename: {:?}, start: {:?}, end: {:?}}}",
            self.filename, self.start, self.end
        )
    }
}

impl CodeRange {
    /// Creates a new code range from filename and start/end locations.
    #[must_use]
    pub const fn new(filename: StringId, start: CodeLoc, end: CodeLoc, preview_line: Option<u32>) -> Self {
        Self {
            filename,
            preview_line,
            start,
            end,
        }
    }

    /// Returns the start position.
    #[must_use]
    pub fn start(&self) -> CodeLoc {
        self.start
    }

    /// Returns the end position.
    #[must_use]
    pub fn end(&self) -> CodeLoc {
        self.end
    }

    /// Returns the preview line number (0-indexed) if available.
    #[must_use]
    pub fn preview_line_number(&self) -> Option<u32> {
        self.preview_line
    }

    /// Returns a new `CodeRange` with an updated end location.
    ///
    /// Clears the preview line when the range spans multiple lines.
    #[must_use]
    pub(crate) fn with_end(self, end: CodeLoc) -> Self {
        let preview_line = if self.start.line == end.line {
            self.preview_line
        } else {
            None
        };
        Self {
            filename: self.filename,
            preview_line,
            start: self.start,
            end,
        }
    }
}

/// Errors that can occur during parsing or preparation of Python code.
#[derive(Debug, Clone)]
pub enum ParseError {
    /// Error in syntax
    Syntax {
        msg: Cow<'static, str>,
        position: CodeRange,
    },
    /// Missing feature from Ouros, we hope to implement in the future.
    /// Message gets prefixed with "The ouros syntax parser does not yet support ".
    NotImplemented {
        msg: Cow<'static, str>,
        position: CodeRange,
    },
    /// Missing feature with a custom full message (no prefix added).
    NotSupported {
        msg: Cow<'static, str>,
        position: CodeRange,
    },
    /// Import error (e.g., relative imports without a package).
    Import {
        msg: Cow<'static, str>,
        position: CodeRange,
    },
}

impl ParseError {
    pub(crate) fn not_implemented(msg: impl Into<Cow<'static, str>>, position: CodeRange) -> Self {
        Self::NotImplemented {
            msg: msg.into(),
            position,
        }
    }

    fn not_supported(msg: impl Into<Cow<'static, str>>, position: CodeRange) -> Self {
        Self::NotSupported {
            msg: msg.into(),
            position,
        }
    }

    fn import_error(msg: impl Into<Cow<'static, str>>, position: CodeRange) -> Self {
        Self::Import {
            msg: msg.into(),
            position,
        }
    }

    pub(crate) fn syntax(msg: impl Into<Cow<'static, str>>, position: CodeRange) -> Self {
        Self::Syntax {
            msg: msg.into(),
            position,
        }
    }

    /// Converts this parser error into a Python exception with source location.
    pub fn into_python_exc(self, filename: &str, source: &str) -> Exception {
        let (exc_type, message, position) = match self {
            Self::Syntax { msg, position } => (ExcType::SyntaxError, msg.into_owned(), position),
            Self::NotImplemented { msg, position } => (
                ExcType::NotImplementedError,
                format!("The ouros syntax parser does not yet support {msg}"),
                position,
            ),
            Self::NotSupported { msg, position } => (ExcType::NotImplementedError, msg.into_owned(), position),
            Self::Import { msg, position } => (ExcType::ImportError, msg.into_owned(), position),
        };

        let mut frame = if exc_type == ExcType::SyntaxError {
            StackFrame::from_position_syntax_error(position, filename, source)
        } else {
            StackFrame::from_position(position, filename, source)
        };

        if exc_type == ExcType::ImportError {
            frame.hide_caret = true;
        }

        Exception::new_full(exc_type, Some(message), vec![frame])
    }
}

/// Returns true if the identifier should be name-mangled in a class body.
///
/// Python mangles names that:
/// - start with `__`
/// - do NOT end with `__`
fn is_mangling_candidate(name: &str) -> bool {
    if !name.starts_with("__") {
        return false;
    }
    if name.ends_with("__") {
        return false;
    }
    true
}

/// Parses an integer literal string into a `BigInt`, handling radix prefixes and underscores.
///
/// Supports Python integer literal formats:
/// - Decimal: `123`, `1_000_000`
/// - Hexadecimal: `0x1a2b`, `0X1A2B`
/// - Octal: `0o777`, `0O777`
/// - Binary: `0b1010`, `0B1010`
///
/// Returns `None` if the string cannot be parsed.
fn parse_int_literal(s: &str) -> Option<BigInt> {
    // Remove underscores (Python allows them as digit separators)
    let cleaned: String = s.chars().filter(|c| *c != '_').collect();
    let cleaned = cleaned.as_str();

    // Detect radix from prefix
    if cleaned.len() >= 2 {
        let prefix = &cleaned[..2];
        let digits = &cleaned[2..];
        match prefix.to_ascii_lowercase().as_str() {
            "0x" => return BigInt::parse_bytes(digits.as_bytes(), 16),
            "0o" => return BigInt::parse_bytes(digits.as_bytes(), 8),
            "0b" => return BigInt::parse_bytes(digits.as_bytes(), 2),
            _ => {}
        }
    }

    // Default to decimal
    cleaned.parse::<BigInt>().ok()
}
