//! AST del subconjunto PyxPic. Solo contiene lo que el lenguaje admite: todo lo
//! demas se rechaza en el parser con un diagnostico claro.

use crate::diag::Span;

pub type ExprId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div, // '//' (division entera, trunca hacia cero)
    /// '/': division exacta, solo entre decimales al compilar (`1.0 / 3`); con enteros es un error.
    TrueDiv,
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
            BinOp::TrueDiv => "/",
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
    /// `8MHz`, `500ms`, `3.3V`: un literal con unidad; el valor es un entero exacto en la unidad base de su magnitud.
    Quantity(i128, crate::units::Dim),
    /// `0.5`, `2.54`, `1.5e-3`: un decimal exacto `num/den` (normalizado); solo existe al compilar.
    Decimal(i128, i128),
    /// `Fixed[int16, 2](x)`: convierte un entero, un decimal constante o otro punto fijo al tipo indicado.
    FixedCast(Box<TypeExpr>, Box<Expr>),
    Bool(bool),
    Name(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Compare(CmpOp, Box<Expr>, Box<Expr>),
    Bool_(BoolOp, Vec<Expr>),
    Call {
        func: String,
        func_span: Span,
        args: Vec<Expr>,
    },
    /// `pin.metodo(args)`, p. ej. `RD0.toggle()`.
    /// Con `attr`, es `REG.BIT.metodo(args)` (p. ej. `STATUS.RP0.set()`).
    /// `[a, b, c]`: solo como valor de una tabla `Rom[u8]`.
    List(Vec<Expr>),
    /// `"texto"`: solo como argumento de `UART.write_text`; el contenido va sin decodificar (con sus escapes).
    Str(String),
    /// `f"texto {nombre:formato}"`: solo como argumento de `UART.write_text`.
    FStr(Vec<FPart>),
    /// `TABLA[indice]`.
    Index {
        obj: String,
        obj_span: Span,
        index: Box<Expr>,
    },
    MethodCall {
        obj: String,
        obj_span: Span,
        attr: Option<Ident>,
        method: String,
        args: Vec<Expr>,
        kwargs: Vec<Kwarg>,
    },
}

/// Una parte de una f-string: texto literal (con sus escapes sin decodificar) o un campo `{nombre:formato}`.
#[derive(Clone, Debug)]
pub enum FPart {
    Lit(String),
    Field { expr: Box<Expr>, spec: FSpec },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FKind {
    /// `{v}` o `{v:d}`: decimal sin ceros a la izquierda.
    Dec,
    /// `{v:X}`: hexadecimal en mayusculas.
    HexUpper,
    /// `{v:x}`: hexadecimal en minusculas.
    HexLower,
    /// `{v:b}`: binario.
    Bin,
}

/// El formato de un campo: `[0][ancho][d|X|x|b]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FSpec {
    pub kind: FKind,
    /// 0 si no se pidio ancho.
    pub width: u8,
    /// `{v:05}`: relleno con ceros en lugar de espacios (los formatos X, x y b siempre rellenan con ceros).
    pub zero: bool,
}

#[derive(Clone, Debug)]
pub enum TypeExprKind {
    /// `Rom[u8]`: datos constantes en la memoria de programa (nombre interno: Rom).
    Rom(Box<TypeExpr>),
    Name(String),
    /// `Const[T]`
    Const(Box<TypeExpr>),
    /// `Fixed[int16, 2]` (escala decimal 10^n) o `QFixed[int16, 8]` (escala binaria 2^n): punto fijo sobre el entero `base`.
    Fixed { binary: bool, base: String, scale: i128 },
    /// `Const` sin tipo (`FACTOR: Const = 0.5`): el valor es una constante de compilacion y el tipo lo da el valor.
    ConstAuto,
    /// `uint8[16]`: arreglo de capacidad fija en RAM (solo a nivel de modulo).
    Array(Box<TypeExpr>, i128),
    /// `CLOCK: Internal`: la fuente del reloj (`Internal`, `Crystal`, `External`, `Resonator`, `RC`, `Secondary`).
    Clock(String),
    /// `FREQUENCY: 8MHz`: la frecuencia de la fuente, una expresion constante.
    Frequency(Box<Expr>),
    /// `OSC_MODE: HS`: el modo del oscilador externo (`Auto` = el que corresponde a la frecuencia).
    OscMode(String),
    /// `PLL: 4`, `PLL: Off`, `CLOCK_DIVIDER: 2`, `CLOCK_OUTPUT: On`, `FAIL_SAFE_CLOCK: On`, `CLOCK_SWITCHING: On`: un ajuste del reloj,
    /// como palabra (`On`, `Off`) o como expresion constante.
    Setting(Option<String>, Option<Box<Expr>>),
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
    Assign {
        target: Ident,
        value: Expr,
    },
    /// `buf[i] = v` (con `op`: `buf[i] += v`): escritura en un arreglo de RAM.
    IndexAssign {
        target: Ident,
        index: Expr,
        op: Option<BinOp>,
        value: Expr,
    },
    AugAssign {
        target: Ident,
        op: BinOp,
        value: Expr,
    },
    If {
        test: Expr,
        body: Vec<Stmt>,
        orelse: Vec<Stmt>,
    },
    While {
        test: Expr,
        body: Vec<Stmt>,
    },
    /// `for var in range(args...)`
    For {
        var: Ident,
        iter_span: Span,
        args: Vec<Expr>,
        body: Vec<Stmt>,
    },
    Return(Option<Expr>),
    Break,
    Continue,
    Pass,
    /// `etiqueta:` (solo la produce el frontend BASIC): destino de un `Goto` dentro de la misma funcion.
    Label(Ident),
    /// `GOTO etiqueta` (solo el frontend BASIC): salta a una etiqueta de la misma funcion.
    Goto(Ident),
    Global(Vec<Ident>),
    Expr(Expr),
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: Ident,
    pub ann: TypeExpr,
}

/// Argumento con nombre (`prescaler=16`): solo en los metodos de periféricos del SDK.
#[derive(Clone, Debug)]
pub struct Kwarg {
    pub name: String,
    pub name_span: Span,
    pub value: Expr,
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
    /// `from pyxpic import a, b`
    Import {
        span: Span,
        module: String,
        names: Vec<Ident>,
    },
}

#[derive(Clone, Debug, Default)]
pub struct Module {
    pub items: Vec<Item>,
    pub expr_count: u32,
}
