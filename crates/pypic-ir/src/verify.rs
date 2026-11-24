//! Verificador de buena formacion de la IR. Todo backend puede asumir que un
//! modulo que lo pasa esta bien tipado y su grafo es valido.

use crate::ir::*;

pub fn verify(m: &Module) -> Result<(), Vec<String>> {
    let mut errs = Vec::new();
    for f in &m.functions {
        verify_fn(m, f, &mut errs);
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

fn verify_fn(m: &Module, f: &Function, errs: &mut Vec<String>) {
    let mut e = |msg: String| errs.push(format!("{}: {msg}", f.name));
    let nb = f.blocks.len();
    let valid = |p: Place| match p {
        Place::Temp(i) => (i as usize) < f.temps.len(),
        Place::Local(i) => (i as usize) < f.locals.len(),
        Place::Global(i) => (i as usize) < m.globals.len(),
    };
    let ty = |o: &Operand| match o {
        Operand::Const { ty, .. } => *ty,
        Operand::Place(p) if valid(*p) => m.place_ty(f, *p),
        Operand::Place(_) => Type::Error,
    };
    if nb == 0 {
        e("sin bloques".into());
        return;
    }
    for (bi, b) in f.blocks.iter().enumerate() {
        let at = |i: usize| format!("bb{bi}[{i}]");
        for (i, inst) in b.insts.iter().enumerate() {
            let dst_ok = |p: &Place| valid(*p);
            match inst {
                Inst::Copy { dst, src } => {
                    if !dst_ok(dst) || ty(src) == Type::Error || m.place_ty(f, *dst) != ty(src) {
                        e(format!("{}: copy con tipos distintos", at(i)));
                    }
                }
                Inst::Unary { dst, op, src } => {
                    let (d, s) = if dst_ok(dst) { (m.place_ty(f, *dst), ty(src)) } else { (Type::Error, Type::Error) };
                    let ok = d == s && match op {
                        UnaryOp::Not => d == Type::Bool,
                        UnaryOp::Neg => d.is_int() && d.is_signed(),
                        UnaryOp::Invert => d.is_int(),
                    };
                    if !ok {
                        e(format!("{}: operacion unaria mal tipada", at(i)));
                    }
                }
                Inst::Binary { dst, op, lhs, rhs } => {
                    if !dst_ok(dst) {
                        e(format!("{}: destino invalido", at(i)));
                        continue;
                    }
                    let d = m.place_ty(f, *dst);
                    let shift = matches!(op, BinOp::Shl | BinOp::Shr);
                    let ok = d.is_int() && ty(lhs) == d && if shift { ty(rhs).is_int() && !ty(rhs).is_signed() } else { ty(rhs) == d };
                    if !ok {
                        e(format!("{}: operacion binaria '{}' mal tipada", at(i), op.symbol()));
                    }
                }
                Inst::Compare { dst, lhs, rhs, .. } => {
                    let ok = dst_ok(dst) && m.place_ty(f, *dst) == Type::Bool && ty(lhs) == ty(rhs) && ty(lhs) != Type::Error;
                    if !ok {
                        e(format!("{}: comparacion mal tipada", at(i)));
                    }
                }
                Inst::Cast { dst, src } => {
                    let ok = dst_ok(dst) && m.place_ty(f, *dst).is_int() && matches!(ty(src), Type::Int { .. } | Type::Bool);
                    if !ok {
                        e(format!("{}: conversion invalida", at(i)));
                    }
                }
                Inst::Delay { unit, amount } => {
                    let konst = matches!(amount, Operand::Const { .. });
                    let ok = match unit {
                        DelayUnit::Cycles | DelayUnit::Us => konst,
                        DelayUnit::Ms | DelayUnit::S => konst || ty(amount) == (Type::Int { bits: 16, signed: false }),
                    };
                    if !ok {
                        e(format!("{}: Delay mal tipado (ciclos/us: constante; ms/s: constante o u16)", at(i)));
                    }
                }
                Inst::Timer { call, dst, arg } => {
                    let width = call.timer.bits();
                    let want_ty = |t: Type| t.is_int() && t.bits() == width && !t.is_signed();
                    match call.op {
                        TimerOp::Value if !dst.is_some_and(|d| valid(d) && want_ty(m.place_ty(f, d))) => e(format!("{}: Timer value necesita un destino de {width} bits sin signo", at(i))),
                        TimerOp::Expired if !dst.is_some_and(|d| valid(d) && m.place_ty(f, d) == Type::Bool) => e(format!("{}: Timer expired necesita un destino bool", at(i))),
                        TimerOp::Set if !arg.as_ref().is_some_and(|a| want_ty(ty(a))) => e(format!("{}: Timer set necesita un valor de {width} bits sin signo", at(i))),
                        _ => {}
                    }
                }
                Inst::RomGet { dst, rom, index } => {
                    let u8_ty = Type::Int { bits: 8, signed: false };
                    if *rom >= m.roms.len() || !valid(*dst) || m.place_ty(f, *dst) != u8_ty || ty(index) != u8_ty {
                        e(format!("{}: RomGet mal formado", at(i)));
                    }
                }
                Inst::Bits { op, dst, src } => {
                    let u8_ty = Type::Int { bits: 8, signed: false };
                    let dst_ok = |t: Type| dst.is_some_and(|d| valid(d) && m.place_ty(f, d) == t);
                    let src_ty = src.as_ref().map(|s| ty(s));
                    let bad = match op {
                        BitsOp::Swap => !(dst_ok(u8_ty) && src_ty == Some(u8_ty)),
                        BitsOp::RolC | BitsOp::RorC => src_ty != Some(u8_ty) || !matches!(src, Some(Operand::Place(_))) || dst.is_some_and(|d| !valid(d) || m.place_ty(f, d) != u8_ty),
                        BitsOp::ClearCarry | BitsOp::SetCarry => dst.is_some() || src.is_some(),
                        BitsOp::Carry => !dst_ok(Type::Bool) || src.is_some(),
                        BitsOp::BitSet(n) | BitsOp::BitClear(n) | BitsOp::BitTest(n) => {
                            !matches!(src, Some(Operand::Place(_))) || src_ty.is_none_or(|t| !t.is_int() || t.bits() > 16 || *n as u32 >= t.bits()) || (matches!(op, BitsOp::BitTest(_)) && !dst_ok(Type::Bool))
                        }
                    };
                    if bad {
                        e(format!("{}: Bits {op:?} mal formado", at(i)));
                    }
                }
                Inst::Reg { call, dst, arg } => {
                    let u8_ty = Type::Int { bits: 8, signed: false };
                    match call.op {
                        RegOp::Read | RegOp::Value if !dst.is_some_and(|d| valid(d) && m.place_ty(f, d) == u8_ty) => e(format!("{}: Reg read/value necesita un destino u8", at(i))),
                        RegOp::Write if !arg.as_ref().is_some_and(|a| ty(a) == u8_ty) => e(format!("{}: Reg write necesita un valor u8", at(i))),
                        RegOp::Set | RegOp::Clear | RegOp::Toggle | RegOp::Value if call.bit.is_none_or(|b| b > 7) => e(format!("{}: Reg necesita un bit 0..7", at(i))),
                        _ => {}
                    }
                }
                Inst::PinCfg { pin, .. } | Inst::PinToggle { pin } => {
                    if *pin >= m.pins.len() {
                        e(format!("{}: pin invalido", at(i)));
                    }
                }
                Inst::PinSet { pin, value } => {
                    if *pin >= m.pins.len() || !matches!(ty(value), Type::Bool | Type::Int { bits: 8, signed: false }) {
                        e(format!("{}: PinSet mal tipado (pin valido y valor bool o u8)", at(i)));
                    }
                }
                Inst::PinGet { dst, pin } => {
                    if *pin >= m.pins.len() || !dst_ok(dst) || m.place_ty(f, *dst) != (Type::Int { bits: 8, signed: false }) {
                        e(format!("{}: PinGet mal tipado (pin valido y destino u8)", at(i)));
                    }
                }
                Inst::Call { dst, func, args } => {
                    let Some(callee) = m.functions.get(*func) else {
                        e(format!("{}: funcion invalida", at(i)));
                        continue;
                    };
                    if args.len() != callee.num_params
                        || args.iter().zip(&callee.locals).any(|(a, p)| ty(a) != p.ty)
                    {
                        e(format!("{}: argumentos de '{}' no coinciden", at(i), callee.name));
                    }
                    if let Some(d) = dst {
                        if !dst_ok(d) || m.place_ty(f, *d) != callee.ret {
                            e(format!("{}: tipo de retorno de '{}' no coincide", at(i), callee.name));
                        }
                    }
                }
            }
        }
        match &b.term {
            Terminator::Jump(t) if *t >= nb => e(format!("bb{bi}: salto a bloque inexistente")),
            Terminator::Branch { cond, then_bb, else_bb } => {
                if *then_bb >= nb || *else_bb >= nb {
                    e(format!("bb{bi}: salto a bloque inexistente"));
                }
                if ty(cond) != Type::Bool {
                    e(format!("bb{bi}: la condicion del branch no es bool"));
                }
            }
            Terminator::Return(Some(o)) if ty(o) != f.ret => e(format!("bb{bi}: return de tipo {} en funcion -> {}", ty(o), f.ret)),
            Terminator::Return(None) if f.ret != Type::Void => e(format!("bb{bi}: return sin valor en funcion -> {}", f.ret)),
            _ => {}
        }
    }
}
