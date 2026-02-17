use crate::{
    args::{ArgExprs, Kwarg},
    builtins::Builtins,
    fstring::FStringPart,
    intern::{BytesId, LongIntId, StringId},
    namespace::NamespaceId,
    parse::{CodeRange, ParsedSignature, Try},
    signature::Signature,
    value::{EitherStr, Marker, Value},
};

/// Indicates which namespace a variable reference belongs to.
///
/// This is determined at prepare time based on Python's scoping rules:
/// - Variables assigned in a function are Local (unless declared `global`)
/// - Variables only read (not assigned) that exist at module level are Global
/// - The `global` keyword explicitly marks a variable as Global
/// - Variables declared `nonlocal` or implicitly captured from enclosing scopes
///   are accessed through Cells
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum NameScope {
    /// Variable is in the current frame's local namespace (assigned somewhere in this function).
    ///
    /// If accessed before assignment, raises `UnboundLocalError`.
    #[default]
    Local,
    /// Variable reference that doesn't exist in any scope.
    ///
    /// A local slot is allocated but never assigned. Accessing raises `NameError`
    /// (not `UnboundLocalError`) because the name was never defined anywhere.
    LocalUnassigned,
    /// Variable is in the module-level global namespace
    Global,
    /// Variable accessed through a cell (heap-allocated container).
    ///
    /// Used for both:
    /// - Variables captured from enclosing scopes (free variables)
    /// - Variables in this function that are captured by nested functions (cell variables)
    ///
    /// The namespace slot contains `Value::Ref(cell_id)` pointing to a `HeapData::Cell`.
    /// Access requires dereferencing through the cell.
    Cell,
}

/// An identifier (variable or function name) with source location and scope information.
///
/// The name is stored as a `StringId` which indexes into the string interner.
/// To get the actual string, look it up in the `Interns` storage.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Identifier {
    pub position: CodeRange,
    /// Interned name ID - look up in Interns to get the actual string.
    pub name_id: StringId,
    opt_namespace_id: Option<NamespaceId>,
    /// Which namespace this identifier refers to (determined at prepare time)
    pub scope: NameScope,
}

impl Identifier {
    /// Creates a new identifier with unknown scope (to be resolved during prepare phase).
    pub fn new(name_id: StringId, position: CodeRange) -> Self {
        Self {
            name_id,
            position,
            opt_namespace_id: None,
            scope: NameScope::Local,
        }
    }

    /// Creates a new identifier with resolved namespace index and explicit scope.
    pub fn new_with_scope(name_id: StringId, position: CodeRange, namespace_id: NamespaceId, scope: NameScope) -> Self {
        Self {
            name_id,
            position,
            opt_namespace_id: Some(namespace_id),
            scope,
        }
    }

    pub fn namespace_id(&self) -> NamespaceId {
        self.opt_namespace_id
            .expect("Identifier not prepared with namespace_id")
    }
}

/// Target of a function call expression.
///
/// Represents a callable that can be either:
/// - A builtin function or exception resolved after scope analysis (`print`, `len`, `ValueError`, etc.)
/// - A name that will be looked up in the namespace at runtime (for callable variables)
///
/// Separate from Value to allow deriving Clone without Value's Clone restrictions.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum Callable {
    /// A builtin function like `print`, `len`, `str`, etc.
    Builtin(Builtins),
    /// A name to be looked up in the namespace at runtime (e.g., `x` in `x = len; x('abc')`).
    Name(Identifier),
}

/// One entry in a dict literal, preserving source order.
///
/// Python allows dict literals to mix normal key/value pairs and mapping unpacking
/// (`**mapping`) in the same expression. We keep this explicit representation so
/// compilation can preserve left-to-right overwrite behavior.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum DictLiteralItem {
    /// A regular `key: value` entry.
    Pair { key: ExprLoc, value: ExprLoc },
    /// A mapping unpack entry: `**mapping`.
    Unpack { mapping: ExprLoc },
}

/// An expression in the AST.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Expr {
    Literal(Literal),
    Builtin(Builtins),
    /// Python's `NotImplemented` singleton constant.
    ///
    /// Resolved at parse time from the name `NotImplemented`. Used as a return
    /// value from dunder methods to signal that the operation is not supported
    /// for the given operand types.
    NotImplemented,
    Name(Identifier),
    /// Function call expression.
    ///
    /// The `callable` can be a Builtin, ExcType (resolved after scope analysis), or a Name
    /// that will be looked up in the namespace at runtime.
    Call {
        callable: Callable,
        /// ArgExprs is relatively large and would require Box anyway since it uses ExprLoc, so keep Expr small
        /// by using a box here
        args: Box<ArgExprs>,
    },
    /// Method call on an object (e.g., `obj.method(args)`).
    ///
    /// The object expression is evaluated first, then the method is looked up
    /// and called with the given arguments. Supports chained attribute access
    /// like `a.b.c.method()`.
    AttrCall {
        object: Box<ExprLoc>,
        attr: EitherStr,
        /// same as above for Box
        args: Box<ArgExprs>,
    },
    /// Expression call (e.g., `(lambda x: x + 1)(5)` or `get_func()(args)`).
    ///
    /// Calls an arbitrary expression as a callable. The callable expression
    /// is evaluated first, then called with the given arguments.
    IndirectCall {
        /// The expression that evaluates to a callable.
        callable: Box<ExprLoc>,
        args: Box<ArgExprs>,
    },
    /// Attribute access expression (e.g., `point.x` or `a.b.c`).
    ///
    /// Retrieves the value of an attribute from an object. For dataclasses,
    /// this returns the field value. For other types, this may trigger
    /// special attribute handling. Supports chained attribute access.
    AttrGet {
        object: Box<ExprLoc>,
        attr: EitherStr,
    },
    Op {
        left: Box<ExprLoc>,
        op: Operator,
        right: Box<ExprLoc>,
    },
    CmpOp {
        left: Box<ExprLoc>,
        op: CmpOperator,
        right: Box<ExprLoc>,
    },
    /// Chain comparison expression: `a < b < c < d`
    ///
    /// Unlike single comparisons, chain comparisons evaluate intermediate values
    /// only once and short-circuit on the first false result. Compiled to bytecode
    /// that uses stack manipulation (Dup, Rot) rather than temporary variables,
    /// avoiding namespace pollution.
    ChainCmp {
        /// The leftmost operand in the chain.
        left: Box<ExprLoc>,
        /// Sequence of (operator, operand) pairs: `[(op1, b), (op2, c), ...]`
        comparisons: Vec<(CmpOperator, ExprLoc)>,
    },
    List(Vec<ExprLoc>),
    Tuple(Vec<ExprLoc>),
    Subscript {
        object: Box<ExprLoc>,
        index: Box<ExprLoc>,
    },
    /// Slice literal expression from `x[start:stop:step]` syntax.
    ///
    /// Each component is optional (None means use the default for that position).
    /// This expression creates a `slice` object when evaluated.
    Slice {
        lower: Option<Box<ExprLoc>>,
        upper: Option<Box<ExprLoc>>,
        step: Option<Box<ExprLoc>>,
    },
    /// Dict literal containing only explicit `key: value` entries.
    Dict(Vec<(ExprLoc, ExprLoc)>),
    /// Dict literal containing at least one `**mapping` unpack entry.
    ///
    /// This variant preserves full source ordering for mixed forms like
    /// `{1: 'a', **m, 2: 'b'}` where later entries overwrite earlier ones.
    DictUnpack(Vec<DictLiteralItem>),
    /// Set literal expression: `{1, 2, 3}`.
    ///
    /// Note: `{}` is always a dict, not an empty set. Use `set()` for empty sets.
    Set(Vec<ExprLoc>),
    /// Unary `not` expression - evaluates to the boolean negation of the operand's truthiness.
    Not(Box<ExprLoc>),
    /// Unary minus expression - negates a numeric value.
    UnaryMinus(Box<ExprLoc>),
    /// Unary plus expression - returns value as-is for numbers, converts bools to int.
    UnaryPlus(Box<ExprLoc>),
    /// Unary bitwise NOT expression - inverts all bits of an integer.
    UnaryInvert(Box<ExprLoc>),
    /// Await expression - suspends execution until the awaited value resolves.
    ///
    /// Can await `ExternalFuture`, `Coroutine`, or `GatherFuture` values.
    /// Raises `TypeError` for non-awaitable values.
    /// Unlike standard Python, `await` is allowed at module level (like Jupyter notebooks).
    Await(Box<ExprLoc>),
    /// A yield expression: `yield value` or bare `yield`.
    ///
    /// Used in generator functions to suspend execution and return a value.
    /// When the generator is resumed, execution continues after the yield.
    Yield {
        value: Option<Box<ExprLoc>>,
    },
    /// A yield from expression: `yield from iterable`.
    ///
    /// Delegates to another iterator, yielding all its values and evaluating to
    /// the delegated iterator's final return value.
    YieldFrom {
        value: Box<ExprLoc>,
    },
    /// F-string expression containing literal and interpolated parts.
    ///
    /// At evaluation time, each part is processed in sequence:
    /// - Literal parts are used directly
    /// - Interpolation parts have their expression evaluated, converted, and formatted
    ///
    /// The results are concatenated to produce the final string.
    FString(Vec<FStringPart>),
    /// Conditional expression (ternary operator): `body if test else orelse`
    ///
    /// Only one of body/orelse is evaluated based on the truthiness of test.
    /// This implements short-circuit evaluation - the branch not taken is never executed.
    IfElse {
        test: Box<ExprLoc>,
        body: Box<ExprLoc>,
        orelse: Box<ExprLoc>,
    },
    /// List comprehension: `[elt for target in iter if cond...]`
    ///
    /// Builds a new list by iterating and optionally filtering. Loop variables
    /// are scoped to the comprehension and do not leak to the enclosing scope.
    ListComp {
        elt: Box<ExprLoc>,
        generators: Vec<Comprehension>,
    },
    /// Set comprehension: `{elt for target in iter if cond...}`
    ///
    /// Builds a new set by iterating and optionally filtering. Duplicate values
    /// are deduplicated. Loop variables are scoped to the comprehension.
    SetComp {
        elt: Box<ExprLoc>,
        generators: Vec<Comprehension>,
    },
    /// Dict comprehension: `{key: value for target in iter if cond...}`
    ///
    /// Builds a new dict by iterating and optionally filtering. Later values
    /// overwrite earlier ones for duplicate keys. Loop variables are scoped
    /// to the comprehension.
    DictComp {
        key: Box<ExprLoc>,
        value: Box<ExprLoc>,
        generators: Vec<Comprehension>,
    },
    /// Raw generator expression from the parser: `(elt for target in iter if cond...)`.
    ///
    /// During prepare this is lowered into `Expr::GeneratorExp`, which stores:
    /// - The eagerly-evaluated outer iterator expression (in enclosing scope)
    /// - A prepared anonymous generator function capturing free variables
    ///
    /// This split mirrors `LambdaRaw` -> `Lambda` and keeps parse-time and
    /// prepare-time responsibilities clearly separated.
    GeneratorExpRaw {
        elt: Box<ExprLoc>,
        generators: Vec<Comprehension>,
        /// Interned hidden iterator parameter name for the lowered function (`.0`).
        iter_arg_name_id: StringId,
        /// Interned synthetic function name (`<genexpr>`).
        genexpr_name_id: StringId,
    },
    /// Prepared generator expression.
    ///
    /// The prepare phase rewrites `(elt for target in iter ...)` to the equivalent:
    ///
    /// ```python
    /// def <genexpr>(.0):
    ///     for target in .0:
    ///         ...
    ///         yield elt
    /// <genexpr>(iter(iterable))
    /// ```
    ///
    /// `outer_iter` is the first iterable expression prepared in the enclosing scope.
    /// `func_def` is the anonymous generator function with resolved names and closure metadata.
    GeneratorExp {
        outer_iter: Box<ExprLoc>,
        func_def: Box<PreparedFunctionDef>,
    },
    /// Raw lambda expression from the parser, before preparation.
    ///
    /// This variant is produced during parsing and contains unprepared data.
    /// During the prepare phase, it gets converted to `Expr::Lambda` with a
    /// fully prepared `PreparedFunctionDef`.
    LambdaRaw {
        /// The interned `<lambda>` name ID.
        name_id: StringId,
        /// The parsed lambda signature (parameters and defaults).
        signature: ParsedSignature,
        /// The lambda body expression (not yet prepared).
        body: Box<ExprLoc>,
    },
    /// Lambda expression: `lambda args: body` (prepared form).
    ///
    /// A lambda is an anonymous function that returns a single expression.
    /// It's compiled identically to a regular function, but with the name `<lambda>`
    /// and an implicit return of the body expression. The resulting function value
    /// stays on the stack as an expression result (not stored to a name).
    Lambda {
        /// The prepared function definition containing signature, body, and closure info.
        /// The body is wrapped as `[Node::Return(body_expr)]` during preparation.
        func_def: Box<PreparedFunctionDef>,
    },
    /// Named expression (walrus operator): `(target := value)`
    ///
    /// Evaluates `value`, assigns it to `target`, and returns the value as the
    /// expression result. The target is treated as an assignment for scope analysis,
    /// so it creates a local binding in the enclosing scope.
    ///
    /// Per PEP 572, in comprehensions the target binds in the enclosing scope,
    /// not the comprehension's implicit scope.
    Named {
        target: Identifier,
        value: Box<ExprLoc>,
    },
}

/// Target for tuple unpacking - can be a name, subscript, nested tuple, or starred target.
///
/// Supports recursive structures like `(a, b), c` or `a, (b, c)`.
/// Also supports starred targets like `first, *rest = [1, 2, 3, 4]`.
/// Used in assignment statements, for loop targets, and comprehension targets.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum UnpackTarget {
    /// Single identifier: `a`
    Name(Identifier),
    /// Subscript target: `obj[key]`.
    ///
    /// Used in unpack assignments like `a[0], b[1] = (x, y)` and in loop targets.
    /// The target object and index expressions are evaluated at assignment time.
    Subscript {
        /// Expression producing the container object.
        target: Box<ExprLoc>,
        /// Expression producing the key or index.
        index: Box<ExprLoc>,
        /// Source position covering the full subscript target.
        target_position: CodeRange,
    },
    /// Nested tuple: `(a, b)` - can contain further nested tuples
    Tuple {
        /// The targets to unpack into (can be names or nested tuples)
        targets: Vec<Self>,
        /// Source position covering all targets (for error caret placement)
        position: CodeRange,
    },
    /// Starred target: `*rest` - captures remaining values into a list.
    ///
    /// Only one starred target is allowed per unpacking level.
    Starred(Identifier),
}

/// A generator clause in a comprehension: `for target in iter [if cond1] [if cond2]...`
///
/// Represents one `for` clause with zero or more `if` filters. Multiple generators
/// create nested iteration (the rightmost varies fastest).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Comprehension {
    /// Loop variable - either single identifier or tuple unpacking pattern.
    pub target: UnpackTarget,
    /// Iterable expression to loop over.
    pub iter: ExprLoc,
    /// Zero or more filter conditions (all must be truthy for the element to be included).
    pub ifs: Vec<ExprLoc>,
}

impl Expr {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::Literal(Literal::None))
    }
}

/// Represents values that can be produced purely from the parser/prepare pipeline.
///
/// Const values are intentionally detached from the runtime heap so we can keep
/// parse-time transformations (constant folding, namespace seeding, etc.) free from
/// reference-count semantics. Only once execution begins are these literals turned
/// into real `Value`s that participate in the interpreter's runtime rules.
///
/// Note: unlike the AST `Constant` type, we store tuples only as expressions since they
/// can't always be recorded as constants.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum Literal {
    Ellipsis,
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// An interned string literal. The StringId references the string in the Interns table.
    Str(StringId),
    /// An interned bytes literal. The BytesId references the bytes in the Interns table.
    Bytes(BytesId),
    /// An interned long integer literal. The `LongIntId` references the value in the Interns table.
    /// Used for integer literals that exceed the i64 range.
    LongInt(LongIntId),
    /// A marker value (e.g., typing constructs like Any, Optional, etc.).
    Marker(Marker),
}

impl From<Literal> for Value {
    /// Converts the literal into its runtime `Value` counterpart.
    ///
    /// This is the only place parse-time data crosses the boundary into runtime
    /// semantics, ensuring every literal follows the same conversion path.
    fn from(literal: Literal) -> Self {
        match literal {
            Literal::Ellipsis => Self::Ellipsis,
            Literal::None => Self::None,
            Literal::Bool(b) => Self::Bool(b),
            Literal::Int(v) => Self::Int(v),
            Literal::Float(v) => Self::Float(v),
            Literal::Str(string_id) => Self::InternString(string_id),
            Literal::Bytes(bytes_id) => Self::InternBytes(bytes_id),
            Literal::LongInt(long_int_id) => Self::InternLongInt(long_int_id),
            Literal::Marker(marker) => Self::Marker(marker),
        }
    }
}

/// An expression with its source location.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExprLoc {
    pub position: CodeRange,
    pub expr: Expr,
}

impl ExprLoc {
    pub fn new(position: CodeRange, expr: Expr) -> Self {
        Self { position, expr }
    }
}

/// An AST node parameterized by the function definition type.
///
/// This generic enum represents statements in both parsed and prepared forms:
/// - `Node<RawFunctionDef>` (aka `ParseNode`): Output of the parser, contains unprepared function bodies
/// - `Node<PreparedFunctionDef>` (aka `PreparedNode`): Output of prepare phase, has resolved names
///
/// Some variants (`Pass`, `Global`, `Nonlocal`) only appear in parsed form and are filtered
/// out during the prepare phase.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Node<F> {
    /// No-op statement. Only present in parsed form, filtered out during prepare.
    Pass,
    Expr(ExprLoc),
    Return(ExprLoc),
    ReturnNone,
    /// Raise statement: `raise`, `raise exc`, or `raise exc from cause`.
    Raise(Option<ExprLoc>, Option<ExprLoc>),
    Assert {
        test: ExprLoc,
        msg: Option<ExprLoc>,
    },
    Assign {
        target: Identifier,
        object: ExprLoc,
    },
    /// Tuple unpacking assignment (e.g., `a, b = some_tuple` or `(a, b), c = nested`).
    ///
    /// The right-hand side is evaluated, then unpacked into the targets in order.
    /// Supports nested unpacking like `(a, b), c = ((1, 2), 'x')`.
    UnpackAssign {
        /// The targets to unpack into (can be names or nested tuples)
        targets: Vec<UnpackTarget>,
        /// Source position covering all targets (for error message caret placement)
        targets_position: CodeRange,
        object: ExprLoc,
    },
    /// Augmented assignment to a simple name: `x += value`
    ///
    /// Loads the name, performs the in-place operation, and stores back.
    OpAssign {
        target: Identifier,
        op: Operator,
        object: ExprLoc,
    },
    /// Augmented assignment to an attribute: `obj.attr += value`
    ///
    /// Evaluates `obj`, loads `obj.attr`, performs the in-place operation,
    /// and stores back with `StoreAttr`. The object expression is evaluated once.
    OpAssignAttr {
        object: ExprLoc,
        attr: EitherStr,
        op: Operator,
        value: ExprLoc,
        target_position: CodeRange,
    },
    /// Augmented assignment to a subscript: `obj[key] += value`
    ///
    /// Evaluates `obj` and `key`, loads `obj[key]`, performs the in-place operation,
    /// and stores back with `StoreSubscr`. Both `obj` and `key` are evaluated once.
    OpAssignSubscr {
        object: ExprLoc,
        index: ExprLoc,
        op: Operator,
        value: ExprLoc,
        target_position: CodeRange,
    },
    /// Subscript assignment: `obj[key] = value`
    ///
    /// The target object can be any expression (a name, attribute access like
    /// `self.data[key] = value`, or even a subscript like `matrix[i][j] = value`).
    SubscriptAssign {
        target: ExprLoc,
        index: ExprLoc,
        value: ExprLoc,
        /// Position of the subscript expression (e.g., `lst[10]`) for traceback carets.
        target_position: CodeRange,
    },
    /// Attribute assignment (e.g., `point.x = 5` or `a.b.c = 5`).
    ///
    /// Assigns a value to an attribute on an object. For mutable dataclasses,
    /// this sets the field value. Returns an error for immutable objects.
    /// Supports chained attribute access on the left-hand side.
    AttrAssign {
        object: ExprLoc,
        attr: EitherStr,
        target_position: CodeRange,
        value: ExprLoc,
    },
    /// Delete a local/global variable: `del x`
    ///
    /// After deletion, accessing the name raises `NameError` or `UnboundLocalError`.
    DeleteName(Identifier),
    /// Delete an attribute: `del obj.attr`
    ///
    /// Removes the attribute from the object. Raises `AttributeError` if the
    /// attribute doesn't exist or can't be deleted.
    DeleteAttr {
        object: ExprLoc,
        attr: EitherStr,
        position: CodeRange,
    },
    /// Delete a subscript: `del obj[key]`
    ///
    /// Removes the item at the given key. Calls `__delitem__` on the object.
    DeleteSubscr {
        object: ExprLoc,
        index: ExprLoc,
        position: CodeRange,
    },
    For {
        /// Loop target - either a single identifier or tuple unpacking pattern.
        target: UnpackTarget,
        iter: ExprLoc,
        body: Vec<Self>,
        or_else: Vec<Self>,
    },
    /// While loop statement: `while test: body [else: orelse]`
    ///
    /// Executes body repeatedly while test is truthy. If the loop exits normally
    /// (not via break), the else block runs.
    While {
        test: ExprLoc,
        body: Vec<Self>,
        or_else: Vec<Self>,
    },
    /// Break statement - exits the innermost loop.
    ///
    /// When executed, control flow jumps past the loop's else block (if any).
    /// Must be inside a loop, otherwise a `SyntaxError` is raised at compile time.
    Break {
        position: CodeRange,
    },
    /// Continue statement - jumps to the next iteration of the innermost loop.
    ///
    /// When executed, control flow jumps back to the loop's iterator advancement.
    /// Must be inside a loop, otherwise a `SyntaxError` is raised at compile time.
    Continue {
        position: CodeRange,
    },
    If {
        test: ExprLoc,
        body: Vec<Self>,
        or_else: Vec<Self>,
    },
    FunctionDef(F),
    /// Class definition statement.
    ///
    /// Like `FunctionDef`, this is parameterized by the function definition type.
    /// In parsed form (`RawClassDef`), it contains unprepared names.
    /// In prepared form (`PreparedClassDef`), names are resolved to namespace indices.
    ///
    /// The class body executes at definition time (like module-level code) and creates
    /// a class object. Methods defined in the class body are compiled as functions.
    ClassDef(Box<ClassDef<F>>),
    /// With statement (context manager): `with expr as var: body`
    ///
    /// Calls `__enter__` on the context expression, assigns the result to `var` (if present),
    /// executes the body, then calls `__exit__` with exception info (or None, None, None).
    /// If `__exit__` returns a truthy value, any exception raised in the body is suppressed.
    With {
        /// The context manager expression (e.g., `open('file')`)
        context_expr: ExprLoc,
        /// Optional `as` target (e.g., `f` in `with open('file') as f:`)
        var: Option<Identifier>,
        /// The body to execute inside the context
        body: Vec<Self>,
    },
    /// Global variable declaration. Only present in parsed form, consumed during prepare.
    ///
    /// Declares that the listed names refer to module-level (global) variables,
    /// allowing functions to read and write them instead of creating local variables.
    Global {
        position: CodeRange,
        names: Vec<StringId>,
    },
    /// Nonlocal variable declaration. Only present in parsed form, consumed during prepare.
    ///
    /// Declares that the listed names refer to variables in enclosing function scopes,
    /// allowing nested functions to read and write them instead of creating local variables.
    Nonlocal {
        position: CodeRange,
        names: Vec<StringId>,
    },
    /// Try/except/else/finally block.
    ///
    /// Executes body, catches matching exceptions with handlers, runs else if no exception,
    /// and always runs finally.
    Try(Try<Self>),
    /// Import statement (e.g., `import sys`, `import sys as s`).
    ///
    /// Loads a module and binds it to a name in the current namespace.
    Import {
        /// The module name to import (e.g., "sys", "typing").
        module_name: StringId,
        /// The binding target - contains the name (or alias), position, and namespace slot.
        /// After prepare phase, this includes the resolved namespace slot for storing the module.
        binding: Identifier,
        /// Whether this import used an explicit alias (`import mod as name`).
        ///
        /// This is used to preserve CPython's dotted-import binding semantics:
        /// `import os.path` binds `os`, while `import os.path as p` binds `p`.
        has_alias: bool,
    },
    /// From-import statement (e.g., `from typing import TYPE_CHECKING`).
    ///
    /// Imports specific names from a module into the current namespace.
    ImportFrom {
        /// The module name to import from (e.g., "typing").
        module_name: StringId,
        /// Names to import: (import_name, binding) pairs.
        /// The import_name is the name in the module, the binding is the local name
        /// (alias if provided, otherwise the import name) with resolved namespace slot.
        names: Vec<(StringId, Identifier)>,
        /// Source position for error reporting.
        position: CodeRange,
    },
}

/// A prepared function definition with resolved names and scope information.
///
/// This is created during the prepare phase and contains everything needed to
/// compile the function to bytecode. The function body has all names resolved
/// to namespace indices with proper scoping.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PreparedFunctionDef {
    /// The function name identifier with resolved namespace index.
    pub name: Identifier,
    /// The binding name used to store this function in the enclosing scope.
    ///
    /// In class bodies, this may be a mangled name (e.g., `_Class__private`),
    /// while `name` remains the original function name for metadata.
    pub binding_name: Identifier,
    /// Parsed type parameter names (PEP 695), stored as interned identifiers.
    pub type_params: Vec<StringId>,
    /// The function signature with parameter names and default counts.
    pub signature: Signature,
    /// The prepared function body with resolved names.
    pub body: Vec<Node<Self>>,
    /// Number of local variable slots needed in the namespace.
    pub namespace_size: usize,
    /// Enclosing namespace slots for variables captured from enclosing scopes.
    ///
    /// At definition time: look up cell HeapId from enclosing namespace at each slot.
    /// At call time: captured cells are pushed sequentially (our slots are implicit).
    pub free_var_enclosing_slots: Vec<NamespaceId>,
    /// Names of free variables captured from enclosing scopes.
    ///
    /// Stored in the same order as `free_var_enclosing_slots`, used for diagnostics
    /// and for resolving zero-arg `super()` via the `__class__` cell.
    #[serde(default)]
    pub free_var_names: Vec<StringId>,
    /// Number of cell variables (captured by nested functions).
    ///
    /// At call time, this many cells are created and pushed right after params.
    /// Their slots are implicitly params.len()..params.len()+cell_var_count.
    pub cell_var_count: usize,
    /// Maps cell variable indices to their corresponding parameter indices, if any.
    ///
    /// When a parameter is also captured by nested functions (cell variable), its value
    /// must be copied into the cell after binding. Each entry corresponds to a cell
    /// (index 0..cell_var_count), and contains `Some(param_index)` if that cell is for
    /// a parameter, or `None` otherwise.
    pub cell_param_indices: Vec<Option<usize>>,
    /// Prepared default value expressions, evaluated at function definition time.
    ///
    /// Layout: `[pos_defaults...][arg_defaults...][kwarg_defaults...]`
    /// Each group contains only the parameters that have defaults, in declaration order.
    /// The counts in `signature` indicate how many defaults exist for each group.
    pub default_exprs: Vec<ExprLoc>,
    /// Prepared annotation expressions, evaluated at function definition time.
    ///
    /// Entries are `(annotation_key, expression)` in definition order.
    /// Parameter annotations use the parameter name as key; return annotations
    /// use the key `"return"`.
    pub annotations: Vec<(StringId, ExprLoc)>,
    /// Whether this is an async function (`async def`).
    ///
    /// When true, calling this function creates a `Coroutine` object instead of
    /// immediately pushing a frame.
    pub is_async: bool,
    /// Whether this is a generator function (contains `yield` or `yield from`).
    ///
    /// When true, calling this function creates a `Generator` object instead of
    /// immediately pushing a frame.
    pub is_generator: bool,
    /// Decorator expressions applied to this function.
    ///
    /// These are evaluated at definition time and applied bottom-to-top.
    /// Inside class bodies, these enable `@staticmethod`, `@classmethod`, and `@property`.
    pub decorators: Vec<ExprLoc>,
}

/// A class definition parameterized by the function definition type.
///
/// This struct is generic over the function definition type `F` so it can hold
/// either `RawFunctionDef` (parsed, unprepared) or `PreparedFunctionDef` (resolved names).
/// The class body contains `Node<F>` statements which may include `FunctionDef(F)` for methods.
///
/// In parsed form, `namespace_size` is 0 (not yet computed).
/// In prepared form, `namespace_size` holds the number of local variable slots needed
/// for the class body namespace.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClassDef<F> {
    /// The class name identifier.
    pub name: Identifier,
    /// The binding name used to store this class in the enclosing scope.
    ///
    /// In class bodies, this may be a mangled name (e.g., `_Outer__Inner`).
    pub binding_name: Identifier,
    /// Parsed type parameter names (PEP 695), stored as interned identifiers.
    pub type_params: Vec<StringId>,
    /// Base class expressions (for inheritance). Empty means implicit `object` base.
    pub bases: Vec<ExprLoc>,
    /// Keyword arguments passed to the class definition (e.g., `metaclass=...`).
    pub keywords: Vec<Kwarg>,
    /// Optional `**kwargs` unpacking expression for class definition.
    ///
    /// This mirrors function call `**kwargs` support, allowing dynamic class
    /// keyword arguments to be merged into the class creation kwargs dict.
    pub var_kwargs: Option<ExprLoc>,
    /// The class body statements (assignments for class variables, function defs for methods).
    pub body: Vec<Node<F>>,
    /// Number of local variable slots needed for the class body namespace.
    /// Set to 0 in parsed form, filled in during prepare phase.
    pub namespace_size: usize,
    /// Namespace slot reserved for the `__class__` cell, if present.
    ///
    /// This is used to support zero-argument `super()` and `__class__` references
    /// inside methods by providing a closure cell owned by the class body frame.
    #[serde(default)]
    pub class_cell_slot: Option<NamespaceId>,
    /// Enclosing namespace slots that provide closure cells for class body execution.
    ///
    /// These cells are copied from the enclosing frame when entering the class body.
    /// Entries are ordered to match `class_free_var_target_slots`.
    #[serde(default)]
    pub class_free_var_enclosing_slots: Vec<NamespaceId>,
    /// Class-body namespace slots where copied closure cells are installed.
    ///
    /// Entries are ordered to match `class_free_var_enclosing_slots`.
    #[serde(default)]
    pub class_free_var_target_slots: Vec<NamespaceId>,
    /// Mapping of class body local variable names (StringId) to their namespace slot indices.
    /// Empty in parsed form, populated during prepare phase.
    /// Used by the compiler to emit code that builds the class namespace dict.
    pub local_names: Vec<(StringId, NamespaceId)>,
    /// Decorator expressions applied to the class itself (e.g., `@dataclass`).
    ///
    /// Applied bottom-to-top after the class is created by `BuildClass`.
    pub decorators: Vec<ExprLoc>,
}

/// Type alias for prepared AST nodes (output of prepare phase).
pub type PreparedNode = Node<PreparedFunctionDef>;

/// Binary operators for arithmetic, bitwise, and boolean operations.
///
/// Uses strum `Display` derive with per-variant serialization for operator symbols.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Operator {
    // `+`
    Add,
    // `-`
    Sub,
    // `*`
    Mult,
    // `@`
    MatMult,
    // `/`
    Div,
    // `%`
    Mod,
    // `**`
    Pow,
    // `<<`
    LShift,
    // `>>`
    RShift,
    // `|`
    BitOr,
    // `^`
    BitXor,
    // `&`
    BitAnd,
    // `//`
    FloorDiv,
    // bool operators
    // `and`
    And,
    // `or`
    Or,
}

/// Defined separately since these operators always return a bool
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CmpOperator {
    Eq,
    NotEq,
    Lt,
    LtE,
    Gt,
    GtE,
    Is,
    IsNot,
    In,
    NotIn,
    // we should support floats too, either via a Number type, or ModEqInt and ModEqFloat
    ModEq(i64),
}
