//! Interprete de referencia de la IR (solo host, para tests).
//!
//! Sirve de oraculo de semantica: los backends deben producir el mismo
//! resultado observable. NO es un interprete de Python ni se ejecuta en el MCU
//! (ver docs/spec/00_PRINCIPIOS_NO_INTERPRETE.md): interpreta la IR ya compilada.

use crate::ir::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trap {
    DivByZero,
    StepLimit,
    NoSuchFunction(String),
    BadArgs,
}

pub struct Machine<'m> {
    module: &'m Module,
    pub globals: Vec<i128>,
    steps_left: u64,
}

struct Frame {
    locals: Vec<i128>,
    temps: Vec<i128>,
}

impl<'m> Machine<'m> {
    pub fn new(module: &'m Module) -> Self {
        Machine { module, globals: module.globals.iter().map(|g| g.init).collect(), steps_left: 10_000_000 }
    }

    pub fn with_step_limit(mut self, n: u64) -> Self {
        self.steps_left = n;
        self
    }

    pub fn global(&self, name: &str) -> Option<i128> {
        self.module.globals.iter().position(|g| g.name == name).map(|i| self.globals[i])
    }

    pub fn call(&mut self, name: &str, args: &[i128]) -> Result<Option<i128>, Trap> {
        let idx = self.module.functions.iter().position(|f| f.name == name).ok_or_else(|| Trap::NoSuchFunction(name.into()))?;
        self.run(idx, args)
    }

    fn run(&mut self, fi: usize, args: &[i128]) -> Result<Option<i128>, Trap> {
        let f = &self.module.functions[fi];
        if args.len() != f.num_params {
            return Err(Trap::BadArgs);
        }
        let mut fr = Frame { locals: vec![0; f.locals.len()], temps: vec![0; f.temps.len()] };
        for (i, a) in args.iter().enumerate() {
            fr.locals[i] = f.locals[i].ty.wrap(*a);
        }
        let mut bb = 0;
        loop {
            let block = &f.blocks[bb];
            for inst in &block.insts {
                self.tick()?;
                self.exec(f, &mut fr, inst)?;
            }
            self.tick()?;
            match &block.term {
                Terminator::Jump(t) => bb = *t,
                Terminator::Branch { cond, then_bb, else_bb } => {
                    bb = if self.read(&fr, cond) != 0 { *then_bb } else { *else_bb };
                }
                Terminator::Return(v) => return Ok(v.as_ref().map(|o| self.read(&fr, o))),
                Terminator::Unreachable => panic!("la IR llego a un 'unreachable' en {}", f.name),
            }
        }
    }

    fn tick(&mut self) -> Result<(), Trap> {
        if self.steps_left == 0 {
            return Err(Trap::StepLimit);
        }
        self.steps_left -= 1;
        Ok(())
    }

    fn read(&self, fr: &Frame, o: &Operand) -> i128 {
        match o {
            Operand::Const { value, .. } => *value,
            Operand::Place(Place::Temp(i)) => fr.temps[*i as usize],
            Operand::Place(Place::Local(i)) => fr.locals[*i as usize],
            Operand::Place(Place::Global(i)) => self.globals[*i as usize],
        }
    }

    fn write(&mut self, f: &Function, fr: &mut Frame, p: Place, v: i128) {
        let ty = self.module.place_ty(f, p);
        let v = if ty == Type::Bool { (v != 0) as i128 } else { ty.wrap(v) };
        match p {
            Place::Temp(i) => fr.temps[i as usize] = v,
            Place::Local(i) => fr.locals[i as usize] = v,
            Place::Global(i) => self.globals[i as usize] = v,
        }
    }

    fn exec(&mut self, f: &Function, fr: &mut Frame, inst: &Inst) -> Result<(), Trap> {
        match inst {
            Inst::Copy { dst, src } => {
                let v = self.read(fr, src);
                self.write(f, fr, *dst, v);
            }
            Inst::Unary { dst, op, src } => {
                let v = self.read(fr, src);
                let r = match op {
                    UnaryOp::Neg => -v,
                    UnaryOp::Invert => !v,
                    UnaryOp::Not => (v == 0) as i128,
                };
                self.write(f, fr, *dst, r);
            }
            Inst::Binary { dst, op, lhs, rhs } => {
                let (a, b) = (self.read(fr, lhs), self.read(fr, rhs));
                let r = match op {
                    BinOp::Add => a + b,
                    BinOp::Sub => a - b,
                    BinOp::Mul => a * b,
                    BinOp::Div | BinOp::Mod if b == 0 => return Err(Trap::DivByZero),
                    BinOp::Div => a / b, // trunca hacia cero
                    BinOp::Mod => a % b,
                    BinOp::BitAnd => a & b,
                    BinOp::BitOr => a | b,
                    BinOp::BitXor => a ^ b,
                    BinOp::Shl => a << b,
                    BinOp::Shr => a >> b,
                };
                self.write(f, fr, *dst, r);
            }
            Inst::Compare { dst, op, lhs, rhs } => {
                let (a, b) = (self.read(fr, lhs), self.read(fr, rhs));
                let r = match op {
                    CmpOp::Eq => a == b,
                    CmpOp::Ne => a != b,
                    CmpOp::Lt => a < b,
                    CmpOp::Le => a <= b,
                    CmpOp::Gt => a > b,
                    CmpOp::Ge => a >= b,
                };
                self.write(f, fr, *dst, r as i128);
            }
            Inst::Cast { dst, src } => {
                let v = self.read(fr, src);
                self.write(f, fr, *dst, v);
            }
            Inst::Call { dst, func, args } => {
                let a: Vec<i128> = args.iter().map(|o| self.read(fr, o)).collect();
                let r = self.run(*func, &a)?;
                if let (Some(d), Some(v)) = (dst, r) {
                    self.write(f, fr, *d, v);
                }
            }
        }
        Ok(())
    }
}
