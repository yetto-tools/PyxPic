//! Volcado textual de la IR (para tests, depuracion y `pypic ir`).

use std::fmt::Write;

use crate::ir::*;

fn place(m: &Module, f: &Function, p: Place) -> String {
    match p {
        Place::Temp(i) => format!("%t{i}"),
        Place::Local(i) => f.locals[i as usize].name.clone(),
        Place::Global(i) => format!("@{}", m.globals[i as usize].name),
    }
}

fn operand(m: &Module, f: &Function, o: &Operand) -> String {
    match o {
        Operand::Const { value, ty } => format!("{value}:{ty}"),
        Operand::Place(p) => place(m, f, *p),
    }
}

fn unary_sym(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Neg => "neg",
        UnaryOp::Invert => "inv",
        UnaryOp::Not => "not",
    }
}

pub fn print(m: &Module) -> String {
    let mut out = String::new();
    for g in &m.globals {
        let kw = if g.is_const { "const" } else { "global" };
        let _ = writeln!(out, "{kw} @{}: {} = {}", g.name, g.ty, g.init);
    }
    for p in &m.pins {
        let an = p.analog.map(|n| format!(", AN{n}")).unwrap_or_default();
        let _ = writeln!(out, "pin {} (puerto {}, bit {}{an})", p.name, p.port, p.bit);
    }
    for f in &m.functions {
        let params: Vec<String> = f.locals[..f.num_params].iter().map(|l| format!("{}: {}", l.name, l.ty)).collect();
        let tag = if f.is_interrupt { "interrupt " } else if f.is_init { "init " } else if f.asm.is_some() { "asm " } else { "" };
        let _ = writeln!(out, "\n{tag}fn {}({}) -> {} {{", f.name, params.join(", "), f.ret);
        let locals: Vec<String> = f.locals[f.num_params..].iter().map(|l| format!("{}: {}", l.name, l.ty)).collect();
        if !locals.is_empty() {
            let _ = writeln!(out, "  locals {}", locals.join(", "));
        }
        for (i, b) in f.blocks.iter().enumerate() {
            let _ = writeln!(out, " bb{i}:");
            for inst in &b.insts {
                let line = match inst {
                    Inst::Copy { dst, src } => format!("{} = {}", place(m, f, *dst), operand(m, f, src)),
                    Inst::Unary { dst, op, src } => format!("{} = {} {}", place(m, f, *dst), unary_sym(*op), operand(m, f, src)),
                    Inst::Binary { dst, op, lhs, rhs } => format!(
                        "{}: {} = {} {} {}",
                        place(m, f, *dst),
                        m.place_ty(f, *dst),
                        operand(m, f, lhs),
                        op.symbol(),
                        operand(m, f, rhs)
                    ),
                    Inst::Compare { dst, op, lhs, rhs } => {
                        format!("{} = {} {} {}", place(m, f, *dst), operand(m, f, lhs), op.symbol(), operand(m, f, rhs))
                    }
                    Inst::Cast { dst, src } => {
                        format!("{}: {} = cast {}", place(m, f, *dst), m.place_ty(f, *dst), operand(m, f, src))
                    }
                    Inst::Delay { unit, amount } => {
                        let u = match unit {
                            DelayUnit::Cycles => "cycles",
                            DelayUnit::Us => "us",
                            DelayUnit::Ms => "ms",
                            DelayUnit::S => "s",
                        };
                        format!("delay {u} {}", operand(m, f, amount))
                    }
                    Inst::Timer { call, dst, arg } => {
                        let a = arg.as_ref().map(|a| format!(" {}", operand(m, f, a))).unwrap_or_default();
                        let d = dst.map(|d| format!("{} = ", place(m, f, d))).unwrap_or_default();
                        format!("{d}timer {} {:?}{a}", call.timer.name(), call.op)
                    }
                    Inst::Uart { call, dst, arg } => {
                        let a = arg.as_ref().map(|a| format!(" {}", operand(m, f, a))).unwrap_or_default();
                        format!("{}uart {:?}{a}", dst.map(|d| format!("{} = ", place(m, f, d))).unwrap_or_default(), call.op)
                    }
                    Inst::Pwm { call, hi, dc } => {
                        let a: String = hi.iter().chain(dc.iter()).map(|a| format!(" {}", operand(m, f, a))).collect();
                        format!("pwm {} {:?}{a}", call.pwm.name(), call.op)
                    }
                    Inst::Adc { call, dst } => format!("{}adc {:?}", dst.map(|d| format!("{} = ", place(m, f, d))).unwrap_or_default(), call.op),
                    Inst::ArrGet { dst, arr, index } => format!("{} = array {}[{}]", place(m, f, *dst), m.arrays[*arr].0, operand(m, f, index)),
                    Inst::ArrSet { arr, index, value } => format!("array {}[{}] = {}", m.arrays[*arr].0, operand(m, f, index), operand(m, f, value)),
                    Inst::RomGet { dst, rom, index } => format!("{} = rom {}[{}]", place(m, f, *dst), m.roms[*rom].0, operand(m, f, index)),
                    Inst::Bits { op, dst, src } => {
                        let a = src.as_ref().map(|s| format!(" {}", operand(m, f, s))).unwrap_or_default();
                        let d = dst.map(|d| format!("{} = ", place(m, f, d))).unwrap_or_default();
                        format!("{d}bits {op:?}{a}")
                    }
                    Inst::Reg { call, dst, arg } => {
                        let a = arg.as_ref().map(|a| format!(" {}", operand(m, f, a))).unwrap_or_default();
                        let d = dst.map(|d| format!("{} = ", place(m, f, d))).unwrap_or_default();
                        let bit = call.bit.map(|b| format!(".{b}")).unwrap_or_default();
                        format!("{d}reg {}{bit} {:?}{a}", call.reg, call.op)
                    }
                    Inst::PinCfg { pin, output } => format!("pin {} = {}", m.pins[*pin].name, if *output { "output" } else { "input" }),
                    Inst::PinSet { pin, value } => format!("pin {} <- {}", m.pins[*pin].name, operand(m, f, value)),
                    Inst::PinToggle { pin } => format!("pin {} toggle", m.pins[*pin].name),
                    Inst::PinGet { dst, pin } => format!("{} = pin {}", place(m, f, *dst), m.pins[*pin].name),
                    Inst::Call { dst, func, args } => {
                        let a: Vec<String> = args.iter().map(|o| operand(m, f, o)).collect();
                        let call = format!("call {}({})", m.functions.get(*func).map_or("?", |x| x.name.as_str()), a.join(", "));
                        match dst {
                            Some(d) => format!("{} = {call}", place(m, f, *d)),
                            None => call,
                        }
                    }
                };
                let _ = writeln!(out, "    {line}");
            }
            let t = match &b.term {
                Terminator::Jump(t) => format!("jump bb{t}"),
                Terminator::Branch { cond, then_bb, else_bb } => {
                    format!("branch {} ? bb{then_bb} : bb{else_bb}", operand(m, f, cond))
                }
                Terminator::Return(Some(o)) => format!("return {}", operand(m, f, o)),
                Terminator::Return(None) => "return".to_string(),
                Terminator::Unreachable => "unreachable".to_string(),
            };
            let _ = writeln!(out, "    {t}");
        }
        out.push_str("}\n");
    }
    out
}
