//! Árbol de sintaxis abstracta (AST) de OpenS.
//!
//! Cada nodo guarda su [`Span`] para que los diagnósticos de tipos y de
//! capacidades (hitos siguientes) puedan señalar la ubicación exacta.
//!
//! El AST cubre los tres paradigmas del lenguaje:
//! - **Imperativo**: `var`, bucles, asignación, `unsafe`.
//! - **Funcional**: closures, tipos función, inmutabilidad, `|>`.
//! - **Orientado a objetos**: `type` + `impl` (métodos) y `trait` (interfaces).
//!
//! Y el modelo de memoria estilo C++: referencias `&T` / `&var T`, punteros
//! crudos `*T` y reservas en heap con `box T`.

use opens_syntax::Span;

/// Un archivo `.ops` completo.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub items: Vec<Item>,
}

// ---------------------------------------------------------------------------
// Ítems de nivel superior
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// Comentarios `///` previos (documentación; se usan para el `inputSchema`
    /// de las herramientas MCP y para el LSP).
    pub doc: Option<String>,
    /// `pub` o `export`.
    pub is_pub: bool,
    pub kind: ItemKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    Import(Import),
    Function(Function),
    TypeDef(TypeDef),
    EnumDef(EnumDef),
    Trait(TraitDef),
    Impl(ImplBlock),
    Prompt(Prompt),
    Server(ServerDef),
    Extern(ExternFn),
    Test(TestDef),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub path: Vec<String>,
    pub alias: Option<String>,
}

/// `fn` corriente o `tool` (función exponible como herramienta MCP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FnFlavor {
    Fn,
    Tool,
}

/// Receptor de un método: `self`, `&self` o `&var self`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfKind {
    Value,
    Ref,
    RefVar,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub flavor: FnFlavor,
    pub name: String,
    pub generics: Vec<String>,
    /// Receptor cuando es un método dentro de `impl`/`trait`.
    pub self_kind: Option<SelfKind>,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    /// Capacidades declaradas: `uses net, llm`.
    pub uses: Vec<String>,
    /// `None` en firmas de `trait` sin cuerpo y en `extern`.
    pub body: Option<Block>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub attrs: Vec<Attr>,
    pub name: String,
    pub ty: Type,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDef {
    pub name: String,
    pub generics: Vec<String>,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub doc: Option<String>,
    pub attrs: Vec<Attr>,
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<Variant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitDef {
    pub name: String,
    pub generics: Vec<String>,
    pub methods: Vec<Function>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImplBlock {
    /// `impl Trait for Tipo` → `Some("Trait")`; `impl Tipo` → `None`.
    pub trait_name: Option<String>,
    pub type_name: String,
    pub generics: Vec<String>,
    pub methods: Vec<Function>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Prompt {
    pub name: String,
    pub params: Vec<Param>,
    /// Plantilla: una expresión string (normalmente multilínea).
    pub template: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerDef {
    pub name: String,
    pub exposes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternFn {
    pub abi: String,
    pub func: Function,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TestDef {
    pub name: String,
    pub body: Block,
}

/// Anotación `@nombre(args)` sobre un campo o parámetro.
#[derive(Debug, Clone, PartialEq)]
pub struct Attr {
    pub name: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Tipos
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Type {
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeKind {
    /// `Int`, `Usuario`, `List<T>`, `Result<T, E>`…
    Named { name: String, args: Vec<Type> },
    /// `&T` / `&var T`.
    Ref { mutable: bool, inner: Box<Type> },
    /// `*T` — puntero crudo estilo C++ (solo en `unsafe`).
    Ptr { inner: Box<Type> },
    /// `box T` — valor en heap de propiedad única.
    Box { inner: Box<Type> },
    /// `fn(T, U) -> R`.
    Func { params: Vec<Type>, ret: Box<Type> },
    /// `enum { a, b }` anónimo dentro del tipo de un campo.
    InlineEnum(Vec<String>),
    /// `()` — unidad.
    Unit,
}

// ---------------------------------------------------------------------------
// Sentencias
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    /// `let`/`var name (: Type)? = expr`.
    Let {
        mutable: bool,
        name: String,
        ty: Option<Type>,
        value: Expr,
    },
    /// `target op= value` (incluye `=`).
    Assign {
        target: Expr,
        op: AssignOp,
        value: Expr,
    },
    Return(Option<Expr>),
    If(IfStmt),
    While {
        cond: Expr,
        body: Block,
    },
    For {
        /// `for await x in ...` (consumir un stream).
        is_await: bool,
        binder: String,
        iter: Expr,
        body: Block,
    },
    Match(MatchExpr),
    Break,
    Continue,
    Unsafe(Block),
    Assert(Expr),
    Expr(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IfStmt {
    pub cond: Expr,
    pub then: Block,
    pub els: Option<Else>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Else {
    Block(Block),
    If(Box<IfStmt>),
}

// ---------------------------------------------------------------------------
// Expresiones
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    /// `none`.
    NoneLit,
    /// `self`.
    SelfExpr,
    Str(Vec<StrSeg>),
    Ident(String),
    List(Vec<Expr>),
    Map(Vec<(Expr, Expr)>),
    /// `(a, b, c)`; de longitud 1 es solo un paréntesis de agrupación.
    Tuple(Vec<Expr>),
    Unit,
    /// `Nombre { campo: valor, ... }`.
    StructLit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    /// `.variante` (forma corta de un valor de enum).
    EnumShorthand(String),
    Call {
        callee: Box<Expr>,
        generics: Vec<Type>,
        args: Vec<Arg>,
    },
    MethodCall {
        recv: Box<Expr>,
        method: String,
        generics: Vec<Type>,
        args: Vec<Arg>,
    },
    Field {
        recv: Box<Expr>,
        name: String,
    },
    Index {
        recv: Box<Expr>,
        index: Box<Expr>,
    },
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
    },
    /// `lhs |> rhs`.
    Pipe {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// `expr?` (propagar error).
    Try(Box<Expr>),
    Await(Box<Expr>),
    Yield(Box<Expr>),
    /// `&expr` / `&var expr` (tomar referencia).
    Ref {
        mutable: bool,
        expr: Box<Expr>,
    },
    /// `*expr` (desreferenciar un puntero/una referencia).
    Deref(Box<Expr>),
    /// `box expr` (mover el valor al heap).
    BoxExpr(Box<Expr>),
    Closure {
        params: Vec<ClosureParam>,
        body: Box<ClosureBody>,
    },
    Match(Box<MatchExpr>),
    /// Marcador de recuperación tras un error de sintaxis.
    Error,
}

/// Segmento de un string: texto literal o una interpolación `{expr}`.
#[derive(Debug, Clone, PartialEq)]
pub enum StrSeg {
    Text(String),
    Interp(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    /// Argumento con nombre: `f(x: 2)`.
    pub name: Option<String>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClosureParam {
    pub name: String,
    pub ty: Option<Type>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClosureBody {
    Expr(Expr),
    Block(Block),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchExpr {
    pub scrutinee: Box<Expr>,
    pub arms: Vec<MatchArm>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pat: Pattern,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// `_`.
    Wildcard,
    /// `.variante`.
    EnumVariant(String),
    /// Ligadura a un nombre.
    Binding(String),
    /// Un literal (`42`, `"x"`, `true`).
    Literal(Expr),
}
