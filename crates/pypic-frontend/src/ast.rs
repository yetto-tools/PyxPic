//! AST del subconjunto PyPIC. Solo contiene lo que el lenguaje admite: todo lo
//! demas se rechaza en el parser con un diagnostico claro.

use crate::diag::Span;

pub type ExprId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div, // '//' (division entera, trunca hacia cero)
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "//",
            BinOp::Mod => "%",
            BinOp::BitAnd => "&",
            BinOp::BitOr => "|",
            BinOp::BitXor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CmpOp {
    pub fn symbol(self) -> &'static str {
        match self {
            CmpOp::Eq => "==",
            CmpOp::Ne => "!=",
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Pos,
    Invert,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOp {
    And,
    Or,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub id: ExprId,
    pub span: Span,
    pub kind: ExprKind,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Int(i128),
    Bool(bool),
    Name(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Compare(CmpOp, Box<Expr>, Box<Expr>),
    Bool_(BoolOp, Vec<Expr>),
    Call { func: String, func_span: Span, args: Vec<Expr> },
    /// `pin.metodo(args)`, p. ej. `RD0.toggle()`.
    MethodCall { obj: String, obj_span: Span, method: String, args: Vec<Expr> },
}

#[derive(Clone, Debug)]
pub enum TypeExprKind {
    Name(String),
    /// `Final[T]`
    Final(Box<TypeExpr>),
    /// `None` (solo como tipo de retorno)
    None,
}

#[derive(Clone, Debug)]
pub struct TypeExpr {
    pub span: Span,
    pub kind: TypeExprKind,
}

#[derive(Clone, Debug)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct VarDecl {
    pub span: Span,
    pub name: Ident,
    pub ann: TypeExpr,
    pub value: Option<Expr>,
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub span: Span,
    pub kind: StmtKind,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Decl(VarDecl),
    Assign { target: Ident, value: Expr },
    AugAssign { target: Ident, op: BinOp, value: Expr },
    If { test: Expr, body: Vec<Stmt>, orelse: Vec<Stmt> },
    While { test: Expr, body: Vec<Stmt> },
    /// `for var in range(args...)`
    For { var: Ident, iter_span: Span, args: Vec<Expr>, body: Vec<Stmt> },
    Return(Option<Expr>),
    Break,
    Continue,
    Pass,
    Global(Vec<Ident>),
    Expr(Expr),
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: Ident,
    pub ann: TypeExpr,
}

/// Cuerpo de una funcion `@asm_function`: el texto ensamblador.
#[derive(Clone, Debug)]
pub struct AsmSource {
    pub text: String,
    /// Posicion en bytes del primer caracter del contenido en el fuente.
    pub offset: usize,
    /// Linea (1-based) del fuente donde empieza el contenido; la rellena `analyze`.
    pub first_line: usize,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FuncDef {
    pub span: Span,
    pub name: Ident,
    pub decorators: Vec<Ident>,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>,
    pub body: Vec<Stmt>,
    /// Solo en funciones `@asm_function`: el ensamblador (y `body` queda vacio).
    pub asm: Option<AsmSource>,
}

#[derive(Clone, Debug)]
pub enum Item {
    Func(FuncDef),
    Var(VarDecl),
    /// `from pypic import a, b`
    Import { span: Span, module: String, names: Vec<Ident> },
}

#[derive(Clone, Debug, Default)]
pub struct Module {
    pub items: Vec<Item>,
    pub expr_count: u32,
}
