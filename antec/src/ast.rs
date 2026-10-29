//! Abstract syntax tree (spec §5). Surface ornament such as case markers stays
//! in the tree; later passes erase it when binding calls (§11.2).

use crate::source::Span;
use crate::token::IntLit;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

/// `a.b.c`; the first segment may be `naa` or a built-in type word such as `varusa`.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub segments: Vec<Ident>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Ni,
    Ki,
    Tho,
    Lo,
    Nunchi,
    Kosam,
    Batti,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Ni => "ni",
            Role::Ki => "ki",
            Role::Tho => "tho",
            Role::Lo => "lo",
            Role::Nunchi => "nunchi",
            Role::Kosam => "kosam",
            Role::Batti => "batti",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub annotations: Vec<Annotation>,
    pub public: bool,
    pub kind: ItemKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    pub name: Ident,
    pub arg: AnnotationArg,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AnnotationArg {
    None,
    Str(String),
    Names(Vec<Ident>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind {
    Import(Import),
    Fn(FnDecl),
    Const { name: Ident, ty: Type, value: Expr },
    Record(RecordDecl),
    Enum(EnumDecl),
    Methods(MethodsDecl),
    Trait(TraitDecl),
    Impl(ImplDecl),
    Extern(ExternBlock),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Import {
    pub module: Path,
    pub kind: ImportKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImportKind {
    /// `m thechuko`
    Module,
    /// `m nunchi a, b thechuko`
    Items(Vec<Ident>),
    /// `m ni k ga thechuko`
    Alias(Ident),
}

#[derive(Clone, Debug, PartialEq)]
pub struct GenericParam {
    pub name: Ident,
    /// Trait bounds such as `Polchochu + Okkokkati[I]`.
    pub bounds: Vec<Type>,
    /// `N: ankam`
    pub is_const: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamKind {
    Receiver,
    Normal,
    /// `migathavi`
    Variadic,
    /// `migatha_perlu`
    NamedCollector,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub kind: ParamKind,
    pub name: Ident,
    pub ty: Type,
    pub role: Option<Role>,
    pub keyword_only: bool,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FnSig {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub ret: Type,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FnDecl {
    pub sig: FnSig,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub annotations: Vec<Annotation>,
    pub public: bool,
    pub name: Ident,
    pub ty: Type,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecordDecl {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub annotations: Vec<Annotation>,
    pub name: Ident,
    pub fields: Vec<Type>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnumDecl {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub variants: Vec<Variant>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MethodsDecl {
    pub generics: Vec<GenericParam>,
    pub self_ty: Type,
    /// Each item is an `ItemKind::Fn`.
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraitItem {
    pub sig: FnSig,
    pub default_body: Option<Block>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraitDecl {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub items: Vec<TraitItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImplDecl {
    pub generics: Vec<GenericParam>,
    pub self_ty: Type,
    pub trait_ref: Type,
    pub items: Vec<FnDecl>,
    /// Written inside a module-level `naa_risk:` block (§22.2).
    pub naa_risk: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExternBlock {
    pub abi: String,
    pub fns: Vec<FnSig>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Type {
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeKind {
    /// Named or built-in type with optional arguments: `ankam32`,
    /// `aasajeevi ankam`, `varusa[T]`, `torch.Tensor`.
    Named {
        path: Path,
        args: Vec<Type>,
    },
    Ref {
        mutable: bool,
        inner: Box<Type>,
    },
    Ptr {
        mutable: bool,
        inner: Box<Type>,
    },
    Array {
        elem: Box<Type>,
        len: Box<Expr>,
    },
    Fn {
        params: Vec<(Type, Option<Role>)>,
        ret: Box<Type>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BindTarget {
    Name(Ident),
    Tuple(Vec<Ident>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Arm {
    pub pattern: Pattern,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    Binding { mutable: bool, target: BindTarget, ty: Option<Type>, value: Expr },
    Assign { place: Expr, value: Expr },
    Compound { place: Expr, op: BinOp, value: Expr },
    Return(Option<Expr>),
    If { branches: Vec<(Expr, Block)>, else_block: Option<Block> },
    While { cond: Expr, body: Block },
    For { iter: Expr, pattern: Pattern, body: Block },
    Match { scrutinee: Expr, arms: Vec<Arm> },
    Break,
    Continue,
    Defer(Block),
    Unsafe(Block),
    Expr(Expr),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Mul,
    Div,
    Rem,
    MatMul,
    Add,
    Sub,
    Shl,
    Shr,
    BitAnd,
    BitXor,
    BitOr,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

impl BinOp {
    pub fn as_str(self) -> &'static str {
        use BinOp::*;
        match self {
            Mul => "*",
            Div => "/",
            Rem => "%",
            MatMul => "@",
            Add => "+",
            Sub => "-",
            Shl => "<<",
            Shr => ">>",
            BitAnd => "&",
            BitXor => "^",
            BitOr => "|",
            Lt => "<",
            Le => "<=",
            Gt => ">",
            Ge => ">=",
            Eq => "==",
            Ne => "!=",
            And => "&&",
            Or => "||",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    BitNot,
    Deref,
    Ref,
    RefMut,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

impl Expr {
    /// The direct sub-expressions.
    pub fn children(&self) -> Vec<&Expr> {
        match &self.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Str(_)
            | ExprKind::Char(_)
            | ExprKind::Bool(_)
            | ExprKind::Ledhu
            | ExprKind::BareSare
            | ExprKind::Unit
            | ExprKind::Hole
            | ExprKind::Path(_) => Vec::new(),
            ExprKind::FStr(parts) | ExprKind::Sql(parts) => parts
                .iter()
                .filter_map(|p| match p {
                    FStrPart::Expr(e) => Some(e),
                    FStrPart::Text(_) => None,
                })
                .collect(),
            ExprKind::Call { callee, phrases, .. } => {
                let mut out = vec![&**callee];
                for p in phrases {
                    if let PhraseKind::Value(e) | PhraseKind::Spread(e) = &p.kind {
                        out.push(e);
                    }
                }
                out
            }
            ExprKind::Try(e) => vec![&**e],
            ExprKind::Pipe { input, stage } => vec![&**input, &**stage],
            ExprKind::Binary { lhs, rhs, .. } => vec![&**lhs, &**rhs],
            ExprKind::Unary { operand, .. } => vec![&**operand],
            ExprKind::Field { base, .. } | ExprKind::TupleIndex { base, .. } => vec![&**base],
            ExprKind::Index { base, indices } => std::iter::once(&**base).chain(indices).collect(),
            ExprKind::Range { lo, hi, .. } => vec![&**lo, &**hi],
            ExprKind::Closure { body, .. } => vec![&**body],
            ExprKind::Ctor { args, .. } => args.iter().collect(),
            ExprKind::Record { fields, .. } => fields.iter().map(|(_, e)| e).collect(),
            ExprKind::Tuple(items) | ExprKind::List(items) => items.iter().collect(),
            ExprKind::Map(pairs) => pairs.iter().flat_map(|(k, v)| [k, v]).collect(),
        }
    }

    /// The height of the syntax tree, computed without recursion so that
    /// it's safe on any input.
    pub fn depth(&self) -> usize {
        let mut max = 0;
        let mut stack = vec![(self, 1)];
        while let Some((e, d)) = stack.pop() {
            max = max.max(d);
            stack.extend(e.children().into_iter().map(|c| (c, d + 1)));
        }
        max
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PhraseKind {
    Value(Expr),
    Spread(Expr),
    Hole,
}

/// One argument phrase of a call: `expr role? (NAME ga)?` (spec §5.3).
#[derive(Clone, Debug, PartialEq)]
pub struct Phrase {
    pub kind: PhraseKind,
    pub role: Option<Role>,
    pub label: Option<Ident>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClosureParam {
    pub name: Ident,
    pub ty: Option<Type>,
    pub role: Option<Role>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FStrPart {
    Text(String),
    Expr(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Int(IntLit),
    Float(String),
    Str(String),
    Char(char),
    FStr(Vec<FStrPart>),
    /// `sql"…"` (Services, §29.3.1).
    Sql(Vec<FStrPart>),
    Bool(bool),
    /// The absence literal `ledhu`.
    Ledhu,
    /// Bare `sare`, meaning `sare(())` (§6.7).
    BareSare,
    Unit,
    /// `_` inside a multi-index (tensors).
    Hole,
    Path(Path),
    /// Every call, with or without phrases. `callee` is a path for phrase
    /// calls; a zero-argument call may have any callee, as in `(f[0])()`.
    Call {
        callee: Box<Expr>,
        type_args: Vec<Type>,
        phrases: Vec<Phrase>,
    },
    Try(Box<Expr>),
    Pipe {
        input: Box<Expr>,
        stage: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Unary {
        op: UnOp,
        operand: Box<Expr>,
    },
    Field {
        base: Box<Expr>,
        name: Ident,
    },
    TupleIndex {
        base: Box<Expr>,
        index: u32,
    },
    Index {
        base: Box<Expr>,
        indices: Vec<Expr>,
    },
    Range {
        lo: Box<Expr>,
        hi: Box<Expr>,
        inclusive: bool,
    },
    Closure {
        moves: bool,
        params: Vec<ClosureParam>,
        body: Box<Expr>,
    },
    /// `undhi(v)`, `sare(v)`, `thappu(e)`, `Enum.Variant(a, b)`.
    Ctor {
        path: Path,
        args: Vec<Expr>,
    },
    Record {
        path: Path,
        fields: Vec<(Ident, Expr)>,
    },
    Tuple(Vec<Expr>),
    List(Vec<Expr>),
    Map(Vec<(Expr, Expr)>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PatternKind {
    Wild,
    Literal(Expr),
    Range {
        lo: Expr,
        hi: Expr,
        inclusive: bool,
    },
    Bind {
        mutable: bool,
        name: Ident,
    },
    /// Prelude constructor (`undhi(p)`, `ledhu`, …) or qualified variant.
    Ctor {
        path: Path,
        args: Option<Vec<Pattern>>,
    },
    Record {
        path: Path,
        fields: Vec<(Ident, Option<Pattern>)>,
        rest: bool,
    },
    Tuple(Vec<Pattern>),
    Or(Vec<Pattern>),
}
