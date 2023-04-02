use super::{Declaration, Located, Type};
use crate::ast::QualifiedName;
use crate::symbol::Symbol;

pub type Expr = Located<ExprKind>;

#[derive(Debug)]
pub enum ExprKind {
    Literal(Literal<Expr>),

    /// Infix operator sequence with unknown precedence
    Infix(Box<Expr>, Vec<(InfixOp, Expr)>),

    /// Record field accessor
    Accessor(Box<Expr>, Symbol),

    RecordUpdate(Box<Expr>, RecordUpdate),

    Var(QualifiedName),

    /// Standalone operator
    Operator(InfixOp),

    DataConstructor(QualifiedName),

    App(Box<Expr>, Vec<Expr>),

    Lam(Vec<Pat>, Box<Expr>),

    Case {
        expr: Box<Expr>,
        branches: Vec<CaseBranch>,
    },

    If {
        cond: Box<Expr>,
        then_: Box<Expr>,
        else_: Box<Expr>,
    },

    Typed(Box<Expr>, Box<Type>),

    Let {
        decls: Vec<Declaration>,
        body: Box<Expr>,
    },

    Wildcard,

    // Pseudo-expression, used only as an intermediate value during parsing.
    RecordUpdateSuffix(RecordUpdate),

    // Pseudo-expression, used only as an intermediate value during parsing.
    NamedPat(Symbol, Box<Expr>),

    Do(Vec<DoItem>),

    Ado(Vec<DoItem>, Box<Expr>),

    Negate(Box<Expr>),
}

#[derive(Debug)]
pub enum InfixOp {
    Symbol(Symbol),
    Backtick(Box<Expr>),
}

#[derive(Debug)]
pub enum DoItem {
    Let(Vec<Declaration>),
    Expr(Expr),
    Bind(Pat, Expr),
}

type RecordUpdate = Vec<(Symbol, Expr)>;

#[derive(Debug)]
pub enum RecordLiteralOrUpdate {
    Literal(Vec<(Symbol, Expr)>),
    Update(Vec<(Symbol, Expr)>),
}

#[derive(Debug)]
pub struct CaseBranch {
    pub pat: Pat,
    pub expr: PossiblyGuardedExpr,
}

#[derive(Debug)]
pub enum PossiblyGuardedExpr {
    Unconditional(Expr),
    Guarded(Vec<GuardedExpr>),
}

#[derive(Debug)]
pub struct GuardedExpr {
    pub guards: Vec<Guard>,
    pub expr: Expr,
}

#[derive(Debug)]
pub enum Guard {
    Expr(Expr),
    Bind(Pat, Expr),
}

pub type Pat = Located<PatKind>;

#[derive(Debug)]
pub enum PatKind {
    Literal(Literal<Pat>),

    /// Infix operator sequence with unknown precedence
    Infix(Box<Pat>, Vec<(Symbol, Pat)>),

    Var(Symbol),

    DataConstructorApp(QualifiedName, Vec<Pat>),

    Wildcard,

    Named(Symbol, Box<Pat>),

    Typed(Box<Pat>, Box<Type>),
}

#[derive(Debug)]
pub enum Literal<T> {
    Integer(u64),
    Float(f64),
    String(String),
    Char(char),
    Boolean(bool),
    Array(Vec<T>),
    Object(Vec<(Symbol, T)>),
}

#[test]
fn test_size() {
    assert_eq!(std::mem::size_of::<Expr>(), 56);
}
