//! La semantica de las operaciones de la IR sobre valores: la usan el interprete de referencia y el optimizador (que pliega constantes), asi que
//! no pueden discrepar. Los valores son enteros de 128 bits ya dentro del rango de su tipo; `stored` los lleva al tipo del destino.

use crate::ir::{BinOp, CmpOp, Type, UnaryOp};

pub fn binary(op: BinOp, a: i128, b: i128) -> i128 {
    match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        // un divisor cero en ejecucion no tiene trampa en un PIC: a // 0 es todo unos y a % 0 es a (docs/sdk-runtime.md)
        BinOp::Div if b == 0 => -1,
        BinOp::Mod if b == 0 => a,
        BinOp::Div => a / b, // trunca hacia cero
        BinOp::Mod => a % b,
        BinOp::BitAnd => a & b,
        BinOp::BitOr => a | b,
        BinOp::BitXor => a ^ b,
        BinOp::Shl => {
            if b >= 128 {
                0
            } else {
                a << b
            }
        }
        BinOp::Shr => a >> b.min(127),
    }
}

pub fn unary(op: UnaryOp, v: i128) -> i128 {
    match op {
        UnaryOp::Neg => -v,
        UnaryOp::Invert => !v,
        UnaryOp::Not => (v == 0) as i128,
    }
}

pub fn compare(op: CmpOp, a: i128, b: i128) -> bool {
    match op {
        CmpOp::Eq => a == b,
        CmpOp::Ne => a != b,
        CmpOp::Lt => a < b,
        CmpOp::Le => a <= b,
        CmpOp::Gt => a > b,
        CmpOp::Ge => a >= b,
    }
}

/// El valor tal como queda guardado en un destino de tipo `ty`: un bool es 0 o 1 y un entero se ajusta (modular) a su ancho y signo.
pub fn stored(ty: Type, v: i128) -> i128 {
    if ty == Type::Bool {
        (v != 0) as i128
    } else {
        ty.wrap(v)
    }
}
