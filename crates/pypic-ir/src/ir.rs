//! IR tipada e independiente del destino.
//!
//! Modelo de memoria: no hay pila de datos. Cada local/parametro es un slot
//! estatico (el backend decide su direccion y banco). Los temporales son
//! registros virtuales que el backend asigna.

pub use pypic_frontend::ast::{BinOp, CmpOp};
pub use pypic_frontend::Type;

pub type BlockId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Invert,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Temp(u32),
    Local(u32),
    Global(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operand {
    Const { value: i128, ty: Type },
    Place(Place),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inst {
    Copy { dst: Place, src: Operand },
    /// El tipo de la operacion es el de `dst`.
    Unary { dst: Place, op: UnaryOp, src: Operand },
    /// Aritmetica modular en el tipo de `dst`. En desplazamientos `rhs` es un entero sin signo.
    Binary { dst: Place, op: BinOp, lhs: Operand, rhs: Operand },
    /// `dst` es bool; `lhs` y `rhs` tienen el mismo tipo.
    Compare { dst: Place, op: CmpOp, lhs: Operand, rhs: Operand },
    /// Conversion explicita (trunca/extiende) del tipo de `src` al de `dst`.
    Cast { dst: Place, src: Operand },
    Call { dst: Option<Place>, func: usize, args: Vec<Operand> },
    /// `RD0.output()` / `RD0.input()`: direccion del pin (y pasarlo a digital si era analogico).
    PinCfg { pin: usize, output: bool },
    /// `RD0.on()/off()/value(x)`: escribe el latch del pin (`value` es bool o u8; distinto de 0 = alto).
    PinSet { pin: usize, value: Operand },
    PinToggle { pin: usize },
    /// `RD0.value()`: lee el nivel del pin como u8 (0 o 1).
    PinGet { dst: Place, pin: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Terminator {
    Jump(BlockId),
    Branch { cond: Operand, then_bb: BlockId, else_bb: BlockId },
    Return(Option<Operand>),
    /// Fin de camino que el frontend garantiza inalcanzable.
    Unreachable,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub insts: Vec<Inst>,
    pub term: Terminator,
}

#[derive(Clone, Debug)]
pub struct Local {
    pub name: String,
    pub ty: Type,
    pub is_param: bool,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub is_interrupt: bool,
    /// Los primeros `num_params` locales son los parametros, en orden.
    pub num_params: usize,
    pub ret: Type,
    pub locals: Vec<Local>,
    pub temps: Vec<Type>,
    /// El bloque 0 es la entrada.
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug)]
pub struct Global {
    pub name: String,
    pub ty: Type,
    pub init: i128,
    pub is_const: bool,
}

/// Pin de E/S usado por el programa (importado de `pypic.gpio`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pin {
    pub name: String,
    pub port: char,
    pub bit: u8,
    /// Canal analogico `ANn`, si el pin lo tiene.
    pub analog: Option<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct Module {
    pub globals: Vec<Global>,
    pub functions: Vec<Function>,
    pub pins: Vec<Pin>,
}

impl Module {
    pub fn operand_ty(&self, f: &Function, op: &Operand) -> Type {
        match op {
            Operand::Const { ty, .. } => *ty,
            Operand::Place(p) => self.place_ty(f, *p),
        }
    }

    pub fn place_ty(&self, f: &Function, p: Place) -> Type {
        match p {
            Place::Temp(i) => f.temps[i as usize],
            Place::Local(i) => f.locals[i as usize].ty,
            Place::Global(i) => self.globals[i as usize].ty,
        }
    }
}
