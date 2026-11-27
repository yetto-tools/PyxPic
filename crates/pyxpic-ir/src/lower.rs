//! Bajada del `Program` tipado del frontend a la IR.

use std::collections::HashMap;

use pyxpic_frontend::ast::*;
use pyxpic_frontend::Program;

use crate::ir::{AsmBlock, Function, Global, Local, Module, Pin, RegOp, TimerOp, Type};
use crate::ir::{Block, BlockId, Inst, Operand, Place, Terminator, UnaryOp};
use pyxpic_frontend::check::FixedOp;
use pyxpic_frontend::BitsOp;

/// El programa tipado a IR, **con las optimizaciones** de `opt` (docs/ir-optimizaciones.md). Es lo que usa el compilador.
pub fn lower(prog: &Program) -> Module {
    let mut m = lower_unoptimized(prog);
    crate::opt::optimize(&mut m);
    m
}

/// El programa tipado a IR tal como sale del frontend, sin optimizar: el oraculo con el que se comprueban las optimizaciones.
#[allow(clippy::field_reassign_with_default)]
pub fn lower_unoptimized(prog: &Program) -> Module {
    let mut m = Module::default();
    m.device = prog.device.clone();
    m.clock_hz = prog.clock_hz;
    m.clock_defaulted = prog.clock_defaulted;
    m.clock_source = prog.clock_source.clone();
    m.roms = prog.roms.clone();
    m.arrays = prog.arrays.clone();
    m.array_inits = prog.array_inits.clone();
    m.adc_acq_ns = prog.adc_config.acq_ns;
    let mut gidx = HashMap::new();
    for it in &prog.module.items {
        if let Item::Var(v) = it {
            // las tablas viven en la memoria de programa, no son globales
            if prog.roms.iter().any(|t| t.0 == v.name.name) || prog.arrays.iter().any(|t| t.0 == v.name.name) || matches!(v.ann.kind, TypeExprKind::Clock(..) | TypeExprKind::Frequency(..) | TypeExprKind::OscMode(..) | TypeExprKind::Setting(..) | TypeExprKind::ConstAuto) {
                continue;
            }
            let sym = &prog.globals[&v.name.name];
            let init = v.value.as_ref().and_then(|e| prog.info[e.id as usize].konst).unwrap_or(0);
            gidx.insert(v.name.name.clone(), m.globals.len());
            m.globals.push(Global { name: v.name.name.clone(), ty: sym.ty, init, is_const: sym.is_final });
        }
    }
    let mut pin_idx: HashMap<String, usize> = HashMap::new();
    for p in &prog.pins {
        pin_idx.insert(p.pin.clone(), m.pins.len());
        m.pins.push(Pin { name: p.pin.clone(), port: p.port, bit: p.bit, analog: p.analog });
    }
    let fidx: HashMap<String, usize> = prog.functions.iter().enumerate().map(|(i, f)| (f.name.clone(), i)).collect();
    for it in &prog.module.items {
        if let Item::Func(f) = it {
            let info = &prog.functions[fidx[&f.name.name]];
            let mut fl = FnLower {
                prog,
                gidx: &gidx,
                fidx: &fidx,
                pin_idx: &pin_idx,
                locals: Vec::new(),
                lmap: HashMap::new(),
                temps: Vec::new(),
                pool: HashMap::new(),
                blocks: vec![(Vec::new(), None)],
                cur: 0,
                loops: Vec::new(),
                labels: HashMap::new(),
            };
            for (name, ty) in &info.params {
                fl.declare_local(name, *ty, true);
            }
            fl.block(&f.body);
            m.functions.push(fl.finish(
                info.name.clone(),
                info.is_interrupt,
                info.is_init,
                info.params.len(),
                info.ret,
                f.asm.as_ref().map(|a| AsmBlock { text: a.text.clone(), first_line: a.first_line }),
            ));
        }
    }
    m
}

struct FnLower<'a> {
    prog: &'a Program,
    gidx: &'a HashMap<String, usize>,
    fidx: &'a HashMap<String, usize>,
    pin_idx: &'a HashMap<String, usize>,
    locals: Vec<Local>,
    lmap: HashMap<String, u32>,
    temps: Vec<Type>,
    /// Temporales del formato de texto, reutilizados por todas las llamadas de la funcion (por papel y tipo): asi el costo en RAM
    /// no crece con el numero de `write_text` y las f-strings.
    pool: HashMap<(&'static str, Type), Place>,
    blocks: Vec<(Vec<Inst>, Option<Terminator>)>,
    cur: BlockId,
    /// (destino de break, destino de continue)
    loops: Vec<(BlockId, BlockId)>,
    /// Bloque de cada etiqueta (`GOTO`, solo BASIC), creado en el primer uso.
    labels: HashMap<String, BlockId>,
}

fn ann_type(te: &TypeExpr) -> Type {
    match &te.kind {
        TypeExprKind::Name(n) => Type::from_name(n).unwrap_or(Type::Error),
        // el punto fijo es el entero que lo guarda
        TypeExprKind::Fixed { base, .. } => Type::from_name(base).unwrap_or(Type::Error),
        TypeExprKind::Const(inner) => ann_type(inner),
        TypeExprKind::None => Type::Void,
        TypeExprKind::Rom(_) | TypeExprKind::Array(..) | TypeExprKind::Clock(..) | TypeExprKind::Frequency(..) | TypeExprKind::OscMode(..) | TypeExprKind::Setting(..) | TypeExprKind::ConstAuto => Type::Error,
    }
}

impl<'a> FnLower<'a> {
    fn finish(self, name: String, is_interrupt: bool, is_init: bool, num_params: usize, ret: Type, asm: Option<AsmBlock>) -> Function {
        let blocks = self.blocks.into_iter().map(|(insts, term)| Block { insts, term: term.unwrap_or(if ret == Type::Void { Terminator::Return(None) } else { Terminator::Unreachable }) }).collect();
        Function { name, is_interrupt, is_init, num_params, ret, locals: self.locals, temps: self.temps, blocks, asm }
    }

    // ------------------------------------------------------------ utilidades
    fn declare_local(&mut self, name: &str, ty: Type, is_param: bool) -> Place {
        let idx = self.locals.len() as u32;
        self.locals.push(Local { name: name.to_string(), ty, is_param });
        self.lmap.insert(name.to_string(), idx);
        Place::Local(idx)
    }

    fn new_temp(&mut self, ty: Type) -> Place {
        self.temps.push(ty);
        Place::Temp(self.temps.len() as u32 - 1)
    }

    fn pooled(&mut self, role: &'static str, ty: Type) -> Place {
        if let Some(&p) = self.pool.get(&(role, ty)) {
            return p;
        }
        let p = self.new_temp(ty);
        self.pool.insert((role, ty), p);
        p
    }

    fn new_block(&mut self) -> BlockId {
        self.blocks.push((Vec::new(), None));
        self.blocks.len() - 1
    }

    fn label_block(&mut self, name: &str) -> BlockId {
        if let Some(&b) = self.labels.get(name) {
            return b;
        }
        let b = self.new_block();
        self.labels.insert(name.to_string(), b);
        b
    }

    fn emit(&mut self, inst: Inst) {
        if self.blocks[self.cur].1.is_some() {
            self.cur = self.new_block(); // codigo muerto tras return/break
        }
        self.blocks[self.cur].0.push(inst);
    }

    fn terminate(&mut self, t: Terminator) {
        if self.blocks[self.cur].1.is_none() {
            self.blocks[self.cur].1 = Some(t);
        }
    }

    fn name_place(&self, name: &str) -> Place {
        match self.lmap.get(name) {
            Some(&i) => Place::Local(i),
            None => Place::Global(self.gidx[name] as u32),
        }
    }

    fn info(&self, e: &Expr) -> pyxpic_frontend::check::ExprInfo {
        self.prog.info[e.id as usize]
    }

    // ----------------------------------------------------------- expresiones
    fn value(&mut self, e: &Expr) -> Operand {
        let inf = self.info(e);
        debug_assert_ne!(inf.ty, Type::Literal, "literal sin tipo en la IR");
        if let Some(k) = inf.konst {
            return Operand::Const { value: k, ty: inf.ty };
        }
        if let ExprKind::Name(n) = &e.kind {
            return Operand::Place(self.name_place(n));
        }
        let t = self.new_temp(inf.ty);
        self.compute(e, Some(t));
        Operand::Place(t)
    }

    /// Evalua `e` directamente en `dst` (evita temporales intermedios).
    fn into(&mut self, dst: Place, e: &Expr) {
        let inf = self.info(e);
        if inf.konst.is_some() || matches!(e.kind, ExprKind::Name(_)) {
            let src = self.value(e);
            self.emit(Inst::Copy { dst, src });
        } else {
            self.compute(e, Some(dst));
        }
    }

    /// Una operacion de punto fijo (`FixedOp`): producto o escala por una fraccion exacta con un intermedio de 16 bits. Todo son
    /// instrucciones enteras de la IR (Cast, Mul, Div, Shr): el PIC no sabe de punto fijo.
    fn fixed_op(&mut self, e: &Expr, op: FixedOp, dst: Place) {
        let child = |this: &mut Self, idx: u8| -> Operand {
            let c: &Expr = match (&e.kind, idx) {
                (ExprKind::Binary(_, l, _), 0) => l,
                (ExprKind::Binary(_, _, r), _) => r,
                (ExprKind::Call { args, .. }, _) => &args[0],
                (ExprKind::FixedCast(_, a), _) => a,
                _ => unreachable!("una operacion de punto fijo solo cuelga de un binario, una llamada o una conversion"),
            };
            this.value(c)
        };
        let widen = |this: &mut Self, v: Operand, w: Type| -> Operand {
            let t = this.new_temp(w);
            this.emit(Inst::Cast { dst: t, src: v });
            Operand::Place(t)
        };
        match op {
            FixedOp::Mul { binary, scale, signed } => {
                let w = Type::Int { bits: 16, signed };
                let (a, b) = (child(self, 0), child(self, 1));
                let (wa, wb) = (widen(self, a, w), widen(self, b, w));
                let p = self.new_temp(w);
                self.emit(Inst::Binary { dst: p, op: BinOp::Mul, lhs: wa, rhs: wb });
                let q = self.new_temp(w);
                if binary {
                    self.emit(Inst::Binary { dst: q, op: BinOp::Shr, lhs: Operand::Place(p), rhs: Operand::Const { value: scale as i128, ty: Type::Int { bits: 8, signed: false } } });
                } else {
                    self.emit(Inst::Binary { dst: q, op: BinOp::Div, lhs: Operand::Place(p), rhs: Operand::Const { value: 10i128.pow(scale as u32), ty: w } });
                }
                self.emit(Inst::Cast { dst, src: Operand::Place(q) });
            }
            FixedOp::Scale { num, den, child: idx, signed } => {
                let w = Type::Int { bits: 16, signed };
                let v = child(self, idx);
                let mut cur = widen(self, v, w);
                if num != 1 {
                    let m = self.new_temp(w);
                    self.emit(Inst::Binary { dst: m, op: BinOp::Mul, lhs: cur, rhs: Operand::Const { value: num, ty: w } });
                    cur = Operand::Place(m);
                }
                if den != 1 {
                    let q = self.new_temp(w);
                    // un desplazamiento solo cuando es identico a la division (sin signo y potencia de dos)
                    if !signed && den > 0 && (den & (den - 1)) == 0 {
                        self.emit(Inst::Binary { dst: q, op: BinOp::Shr, lhs: cur, rhs: Operand::Const { value: den.trailing_zeros() as i128, ty: Type::Int { bits: 8, signed: false } } });
                    } else {
                        self.emit(Inst::Binary { dst: q, op: BinOp::Div, lhs: cur, rhs: Operand::Const { value: den, ty: w } });
                    }
                    cur = Operand::Place(q);
                }
                self.emit(Inst::Cast { dst, src: cur });
            }
        }
    }

    fn compute(&mut self, e: &Expr, dst: Option<Place>) {
        if let Some(op) = self.prog.fixed_ops.get(&e.id).copied() {
            if let Some(dst) = dst {
                self.fixed_op(e, op, dst);
            }
            return;
        }
        match &e.kind {
            ExprKind::Binary(op, l, r) => {
                let (lhs, rhs) = (self.value(l), self.value(r));
                self.emit(Inst::Binary { dst: dst.unwrap(), op: *op, lhs, rhs });
            }
            ExprKind::Unary(op, x) => {
                let src = self.value(x);
                let dst = dst.unwrap();
                let op = match op {
                    UnOp::Pos => return self.emit(Inst::Copy { dst, src }),
                    UnOp::Neg => UnaryOp::Neg,
                    UnOp::Invert => UnaryOp::Invert,
                    UnOp::Not => UnaryOp::Not,
                };
                self.emit(Inst::Unary { dst, op, src });
            }
            ExprKind::Compare(op, l, r) => {
                let (lhs, rhs) = (self.value(l), self.value(r));
                self.emit(Inst::Compare { dst: dst.unwrap(), op: *op, lhs, rhs });
            }
            ExprKind::Bool_(..) => {
                let dst = dst.unwrap();
                let (t, f, join) = (self.new_block(), self.new_block(), self.new_block());
                self.cond(e, t, f);
                for (bb, v) in [(t, 1), (f, 0)] {
                    self.cur = bb;
                    self.emit(Inst::Copy { dst, src: Operand::Const { value: v, ty: Type::Bool } });
                    self.terminate(Terminator::Jump(join));
                }
                self.cur = join;
            }
            ExprKind::MethodCall { args, .. } if self.prog.uart_calls.contains_key(&e.id) => {
                let call = self.prog.uart_calls[&e.id];
                if let pyxpic_frontend::UartOp::WriteText(rom) = call.op {
                    self.write_text(rom);
                    return;
                }
                match call.op {
                    pyxpic_frontend::UartOp::ReadLine(arr) => {
                        self.read_line(arr, dst);
                        return;
                    }
                    pyxpic_frontend::UartOp::WriteBuf(arr) => {
                        let n = self.value(&args[1]);
                        self.write_buf(arr, n);
                        return;
                    }
                    _ => {}
                }
                if call.op == pyxpic_frontend::UartOp::WriteFmt {
                    let parts = self.prog.uart_fmt[&e.id].clone();
                    self.write_fmt(&args[0], &parts);
                    return;
                }
                let arg =
                    matches!(call.op, pyxpic_frontend::UartOp::Write | pyxpic_frontend::UartOp::Send | pyxpic_frontend::UartOp::Write9 | pyxpic_frontend::UartOp::Send9).then(|| self.value(&args[0]));
                // las consultas de bandera sin usar no hacen nada; read/recv sacan un byte del FIFO aunque se descarte
                if call.op.is_pure_read() && dst.is_none() {
                    return;
                }
                let dst = match call.op {
                    pyxpic_frontend::UartOp::Read | pyxpic_frontend::UartOp::Recv => Some(dst.unwrap_or_else(|| self.new_temp(Type::Int { bits: 8, signed: false }))),
                    pyxpic_frontend::UartOp::Read9 | pyxpic_frontend::UartOp::Recv9 => Some(dst.unwrap_or_else(|| self.new_temp(Type::Int { bits: 16, signed: false }))),
                    op if op.is_pure_read() => dst,
                    _ => None,
                };
                self.emit(Inst::Uart { call, dst, arg });
            }
            ExprKind::MethodCall { .. } if self.prog.comp_calls.contains_key(&e.id) => {
                let call = self.prog.comp_calls[&e.id];
                // leer sin usar el resultado no tiene efecto: no se emite
                if !call.returns_value() || dst.is_some() {
                    self.emit(Inst::Comp { call, dst: if call.returns_value() { dst } else { None } });
                }
            }
            ExprKind::MethodCall { args, .. } if self.prog.eeprom_calls.contains_key(&e.id) => {
                let call = self.prog.eeprom_calls[&e.id];
                use pyxpic_frontend::EepromOp as E;
                let reads = matches!(call.op, E::Read | E::Busy | E::Failed);
                // leer sin usar el resultado no tiene efecto: no se emite
                if reads && dst.is_none() {
                    return;
                }
                let addr = matches!(call.op, E::Read | E::Write | E::StartWrite).then(|| self.value(&args[0]));
                let val = matches!(call.op, E::Write | E::StartWrite).then(|| self.value(&args[1]));
                self.emit(Inst::Eeprom { call, dst: if reads { dst } else { None }, addr, val });
            }
            ExprKind::MethodCall { args, kwargs, .. } if self.prog.i2c_calls.contains_key(&e.id) => {
                let call = self.prog.i2c_calls[&e.id];
                let arg = match call.op {
                    pyxpic_frontend::I2cOp::Write => Some(self.value(&args[0])),
                    pyxpic_frontend::I2cOp::Read => Some(self.value(&kwargs[0].value)),
                    _ => None,
                };
                self.emit(Inst::I2c { call, dst, arg });
            }
            ExprKind::MethodCall { args, .. } if self.prog.spi_calls.contains_key(&e.id) => {
                let call = self.prog.spi_calls[&e.id];
                let arg = (call.op == pyxpic_frontend::SpiOp::Transfer).then(|| self.value(&args[0]));
                self.emit(Inst::Spi { call, dst, arg });
            }
            ExprKind::MethodCall { args, .. } if self.prog.pwm_calls.contains_key(&e.id) => {
                let call = self.prog.pwm_calls[&e.id];
                let u8_ty = Type::Int { bits: 8, signed: false };
                let (hi, dc) = if call.op == pyxpic_frontend::PwmOp::Duty {
                    // el ciclo de trabajo es de 10 bits: CCPRxL lleva los ocho altos y DCxB los dos bajos; los bits sobre el 10 se ignoran
                    match self.value(&args[0]) {
                        Operand::Const { value, .. } => (Some(Operand::Const { value: (value >> 2) & 0xFF, ty: u8_ty }), Some(Operand::Const { value: value & 3, ty: u8_ty })),
                        v => {
                            let u16_ty = Type::Int { bits: 16, signed: false };
                            let shifted = self.new_temp(u16_ty);
                            self.emit(Inst::Binary { dst: shifted, op: BinOp::Shr, lhs: v, rhs: Operand::Const { value: 2, ty: u16_ty } });
                            let hi = self.new_temp(u8_ty);
                            self.emit(Inst::Cast { dst: hi, src: Operand::Place(shifted) });
                            let low = self.new_temp(u16_ty);
                            self.emit(Inst::Binary { dst: low, op: BinOp::BitAnd, lhs: v, rhs: Operand::Const { value: 3, ty: u16_ty } });
                            let dc = self.new_temp(u8_ty);
                            self.emit(Inst::Cast { dst: dc, src: Operand::Place(low) });
                            (Some(Operand::Place(hi)), Some(Operand::Place(dc)))
                        }
                    }
                } else {
                    (None, None)
                };
                self.emit(Inst::Pwm { call, hi, dc });
            }
            ExprKind::MethodCall { .. } if self.prog.adc_calls.contains_key(&e.id) => {
                let call = self.prog.adc_calls[&e.id];
                // leer sin usar el resultado no tiene efecto: no se emite
                let reads = matches!(call.op, pyxpic_frontend::AdcOp::Done | pyxpic_frontend::AdcOp::Result | pyxpic_frontend::AdcOp::Read(_));
                if !reads || dst.is_some() {
                    self.emit(Inst::Adc { call, dst: if reads { dst } else { None } });
                }
            }
            ExprKind::Index { index, .. } => {
                let index = self.value(index);
                if let Some(&arr) = self.prog.array_gets.get(&e.id) {
                    self.emit(Inst::ArrGet { dst: dst.unwrap(), arr, index });
                } else {
                    let rom = self.prog.rom_gets[&e.id];
                    self.emit(Inst::RomGet { dst: dst.unwrap(), rom, index });
                }
            }
            ExprKind::Str(_) | ExprKind::FStr(_) => unreachable!("el frontend rechaza el texto fuera de UART.write_text"),
            ExprKind::List(_) => unreachable!("el frontend rechaza las listas fuera de una tabla"),
            ExprKind::Call { args, .. } if self.prog.text_calls.contains_key(&e.id) => {
                let tc = self.prog.text_calls[&e.id];
                self.parse_text(tc, &args[1], &args[2], dst);
            }
            ExprKind::Call { func, args, .. } => {
                if self.prog.bits_calls.contains_key(&e.id) {
                    let op = self.prog.bits_calls[&e.id];
                    // una lectura sin destino no tiene efecto, salvo las rotaciones, que mueven el acarreo
                    if op.returns_value() && dst.is_none() && !matches!(op, BitsOp::RolC | BitsOp::RorC) {
                        return;
                    }
                    let src = match op {
                        BitsOp::ClearCarry | BitsOp::SetCarry | BitsOp::Carry => None,
                        _ => Some(self.value(&args[0])),
                    };
                    self.emit(Inst::Bits { op, dst: if op.returns_value() { dst } else { None }, src });
                    return;
                }
                if let (Some(unit), false) = (pyxpic_frontend::time_unit(func), self.fidx.contains_key(func)) {
                    let amount = self.value(&args[0]);
                    self.emit(Inst::Delay { unit, amount });
                } else if let Some(_ty) = Type::from_name(func).filter(|t| t.is_int()) {
                    let src = self.value(&args[0]);
                    self.emit(Inst::Cast { dst: dst.unwrap(), src });
                } else {
                    let ops: Vec<Operand> = args.iter().map(|a| self.value(a)).collect();
                    let fi = self.fidx[func];
                    let has_ret = self.prog.functions[fi].ret != Type::Void;
                    self.emit(Inst::Call { dst: if has_ret { dst } else { None }, func: fi, args: ops });
                }
            }
            ExprKind::MethodCall { args, .. } if self.prog.timer_calls.contains_key(&e.id) => {
                let call = self.prog.timer_calls[&e.id];
                let arg = if call.op == TimerOp::Set { Some(self.value(&args[0])) } else { None };
                // leer sin usar el resultado no tiene efecto: no se emite
                let reads = matches!(call.op, TimerOp::Value | TimerOp::Expired);
                if !reads || dst.is_some() {
                    self.emit(Inst::Timer { call, dst: if reads { dst } else { None }, arg });
                }
            }
            ExprKind::MethodCall { args, .. } if self.prog.reg_calls.contains_key(&e.id) => {
                let call = self.prog.reg_calls[&e.id].clone();
                let arg = if call.op == RegOp::Write { Some(self.value(&args[0])) } else { None };
                let reads = matches!(call.op, RegOp::Read | RegOp::Value);
                // leer sin usar el resultado no tiene efecto: no se emite
                if !reads || dst.is_some() {
                    self.emit(Inst::Reg { call, dst: if reads { dst } else { None }, arg });
                }
            }
            ExprKind::MethodCall { obj, method, args, .. } => {
                let pin = self.pin_idx[obj];
                let flag = |v: i128| Operand::Const { value: v, ty: Type::Bool };
                match method.as_str() {
                    "output" => self.emit(Inst::PinCfg { pin, output: true }),
                    "input" => self.emit(Inst::PinCfg { pin, output: false }),
                    "on" => self.emit(Inst::PinSet { pin, value: flag(1) }),
                    "off" => self.emit(Inst::PinSet { pin, value: flag(0) }),
                    "toggle" => self.emit(Inst::PinToggle { pin }),
                    _ => match args.first() {
                        Some(a) => {
                            let value = self.value(a);
                            self.emit(Inst::PinSet { pin, value });
                        }
                        None => {
                            if let Some(dst) = dst {
                                self.emit(Inst::PinGet { dst, pin });
                            }
                        }
                    },
                }
            }
            ExprKind::Int(_) | ExprKind::Quantity(..) | ExprKind::Decimal(..) | ExprKind::FixedCast(..) | ExprKind::Bool(_) | ExprKind::Name(_) => {
                let src = self.value(e);
                self.emit(Inst::Copy { dst: dst.unwrap(), src });
            }
        }
    }

    /// Salta a `t` o `f` segun `e`, con cortocircuito en and/or/not.
    fn cond(&mut self, e: &Expr, t: BlockId, f: BlockId) {
        if let Some(k) = self.info(e).konst {
            return self.terminate(Terminator::Jump(if k != 0 { t } else { f }));
        }
        match &e.kind {
            ExprKind::Unary(UnOp::Not, x) => self.cond(x, f, t),
            ExprKind::Bool_(op, vals) => {
                for v in &vals[..vals.len() - 1] {
                    let next = self.new_block();
                    match op {
                        BoolOp::And => self.cond(v, next, f),
                        BoolOp::Or => self.cond(v, t, next),
                    }
                    self.cur = next;
                }
                self.cond(vals.last().unwrap(), t, f);
            }
            _ => {
                let c = self.value(e);
                self.terminate(Terminator::Branch { cond: c, then_bb: t, else_bb: f });
            }
        }
    }

    // ----------------------------------------------------------- sentencias
    fn block(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Pass | StmtKind::Global(_) => {}
            StmtKind::Label(l) => {
                let b = self.label_block(&l.name);
                self.terminate(Terminator::Jump(b));
                self.cur = b;
            }
            StmtKind::Goto(l) => {
                let b = self.label_block(&l.name);
                self.terminate(Terminator::Jump(b));
            }
            // una constante local (`K: Const[u8] = 5`) no es una variable: el frontend ya dio valor a cada uso y la IR no
            // le reserva un slot ni escribe su valor (#11)
            StmtKind::Decl(v) if matches!(v.ann.kind, TypeExprKind::Const(_)) && v.value.as_ref().is_some_and(|e| self.info(e).konst.is_some()) => {}
            StmtKind::Decl(v) => {
                let ty = ann_type(&v.ann);
                let dst = self.declare_local(&v.name.name, ty, false);
                if let Some(val) = &v.value {
                    self.into(dst, val);
                }
            }
            StmtKind::Assign { target, value } => {
                let dst = self.name_place(&target.name);
                self.into(dst, value);
            }
            StmtKind::IndexAssign { target, index, op, value } => {
                let arr = self.prog.arrays.iter().position(|(n, _)| *n == target.name).expect("el frontend declara el arreglo");
                let idx = self.value(index);
                let u8t = Type::Int { bits: 8, signed: false };
                let new = match op {
                    None => self.value(value),
                    Some(op) => {
                        // buf[i] op= v: se lee, se opera y se escribe con el mismo indice (evaluado una sola vez)
                        let cur = self.new_temp(u8t);
                        self.emit(Inst::ArrGet { dst: cur, arr, index: idx });
                        let rhs = self.value(value);
                        self.emit(Inst::Binary { dst: cur, op: *op, lhs: Operand::Place(cur), rhs });
                        Operand::Place(cur)
                    }
                };
                self.emit(Inst::ArrSet { arr, index: idx, value: new });
            }
            StmtKind::AugAssign { target, op, value } => {
                let dst = self.name_place(&target.name);
                let rhs = self.value(value);
                self.emit(Inst::Binary { dst, op: *op, lhs: Operand::Place(dst), rhs });
            }
            StmtKind::Expr(e) => self.compute(e, None),
            StmtKind::Return(v) => {
                let op = v.as_ref().map(|e| self.value(e));
                self.terminate(Terminator::Return(op));
            }
            StmtKind::Break => {
                let (b, _) = *self.loops.last().unwrap();
                self.terminate(Terminator::Jump(b));
            }
            StmtKind::Continue => {
                let (_, c) = *self.loops.last().unwrap();
                self.terminate(Terminator::Jump(c));
            }
            StmtKind::If { test, body, orelse } => {
                let (t, f, join) = (self.new_block(), self.new_block(), self.new_block());
                self.cond(test, t, f);
                self.cur = t;
                self.block(body);
                self.terminate(Terminator::Jump(join));
                self.cur = f;
                self.block(orelse);
                self.terminate(Terminator::Jump(join));
                self.cur = join;
            }
            StmtKind::While { test, body } => {
                let (header, bodyb, exit) = (self.new_block(), self.new_block(), self.new_block());
                self.terminate(Terminator::Jump(header));
                self.cur = header;
                self.cond(test, bodyb, exit);
                self.cur = bodyb;
                self.loops.push((exit, header));
                self.block(body);
                self.loops.pop();
                self.terminate(Terminator::Jump(header));
                self.cur = exit;
            }
            StmtKind::For { var, args, body, .. } => self.for_range(var, args, body),
        }
    }

    /// `for v in range(start, stop, step)`.
    ///
    /// No se usa `v += step` seguido de `v < stop`: con `u8` y `range(0, 255, 10)`
    /// el contador daria la vuelta (250 + 10 = 4) y el bucle no terminaria. Al final de
    /// cada iteracion se comprueba el recorrido restante (en el tipo sin signo del mismo
    /// ancho, donde la resta es exacta) y solo se avanza si queda al menos un paso.
    /// `UART.read_line(buf)`: lee bytes con `UART.read` hasta un `\n`. El `\r` se ignora; el resto se guarda en el arreglo mientras
    /// quepa y, si la linea es mas larga, **se descarta el exceso** (sigue leyendo hasta el `\n` para no dejar media linea en el
    /// puerto). Devuelve cuantos bytes guardo (el `\n` no cuenta ni se guarda). Bloquea hasta recibir el `\n`.
    ///
    /// Un desbordamiento del receptor (`OERR`) detendria la recepcion para siempre y `read_line` se quedaria esperando: por eso se
    /// borra (`clear_overrun`) y **se reinicia la linea en curso**: lo guardado hasta entonces esta danado (faltan bytes) y los
    /// caracteres que quedaron en el FIFO del receptor (DS40001291H 12.1.2.5: siguen siendo legibles) se descartan; el resto de esa
    /// linea llegara como una linea mas.
    fn read_line(&mut self, arr: usize, dst: Option<Place>) {
        let u8t = Type::Int { bits: 8, signed: false };
        let len = self.prog.arrays[arr].1 as i128;
        let n = self.pooled("line_n", u8t);
        let byte = self.pooled("byte", u8t);
        let c = self.pooled("cond", Type::Bool);
        self.emit(Inst::Copy { dst: n, src: Operand::Const { value: 0, ty: u8t } });
        let (header, clear, flush, flush_take, poll, take, not_nl, not_cr, store, exit) =
            (self.new_block(), self.new_block(), self.new_block(), self.new_block(), self.new_block(), self.new_block(), self.new_block(), self.new_block(), self.new_block(), self.new_block());
        self.terminate(Terminator::Jump(header));
        // espera activa con las primitivas: comprueba el desbordamiento y la llegada de un byte
        self.cur = header;
        let uart = |op| pyxpic_frontend::UartCall { op };
        self.emit(Inst::Uart { call: uart(pyxpic_frontend::UartOp::Overrun), dst: Some(c), arg: None });
        self.terminate(Terminator::Branch { cond: Operand::Place(c), then_bb: clear, else_bb: poll });
        self.cur = clear;
        self.emit(Inst::Uart { call: uart(pyxpic_frontend::UartOp::ClearOverrun), dst: None, arg: None });
        self.emit(Inst::Copy { dst: n, src: Operand::Const { value: 0, ty: u8t } });
        self.terminate(Terminator::Jump(flush));
        self.cur = flush;
        self.emit(Inst::Uart { call: uart(pyxpic_frontend::UartOp::RxReady), dst: Some(c), arg: None });
        self.terminate(Terminator::Branch { cond: Operand::Place(c), then_bb: flush_take, else_bb: poll });
        self.cur = flush_take;
        self.emit(Inst::Uart { call: uart(pyxpic_frontend::UartOp::Recv), dst: Some(byte), arg: None });
        self.terminate(Terminator::Jump(flush));
        self.cur = poll;
        self.emit(Inst::Uart { call: uart(pyxpic_frontend::UartOp::RxReady), dst: Some(c), arg: None });
        self.terminate(Terminator::Branch { cond: Operand::Place(c), then_bb: take, else_bb: header });
        self.cur = take;
        self.emit(Inst::Uart { call: uart(pyxpic_frontend::UartOp::Recv), dst: Some(byte), arg: None });
        let eq = |this: &mut Self, v: i128| this.emit(Inst::Compare { dst: c, op: CmpOp::Eq, lhs: Operand::Place(byte), rhs: Operand::Const { value: v, ty: u8t } });
        eq(self, 10);
        self.terminate(Terminator::Branch { cond: Operand::Place(c), then_bb: exit, else_bb: not_nl });
        self.cur = not_nl;
        eq(self, 13);
        self.terminate(Terminator::Branch { cond: Operand::Place(c), then_bb: header, else_bb: not_cr });
        self.cur = not_cr;
        self.emit(Inst::Compare { dst: c, op: CmpOp::Lt, lhs: Operand::Place(n), rhs: Operand::Const { value: len, ty: u8t } });
        self.terminate(Terminator::Branch { cond: Operand::Place(c), then_bb: store, else_bb: header });
        self.cur = store;
        self.emit(Inst::ArrSet { arr, index: Operand::Place(n), value: Operand::Place(byte) });
        self.emit(Inst::Binary { dst: n, op: BinOp::Add, lhs: Operand::Place(n), rhs: Operand::Const { value: 1, ty: u8t } });
        self.terminate(Terminator::Jump(header));
        self.cur = exit;
        if let Some(d) = dst {
            self.emit(Inst::Copy { dst: d, src: Operand::Place(n) });
        }
    }

    /// `UART.write_buf(buf, n)`: `UART.write(buf[i])` para `i` de 0 a `min(n, capacidad)`.
    fn write_buf(&mut self, arr: usize, n: Operand) {
        let u8t = Type::Int { bits: 8, signed: false };
        let len = self.prog.arrays[arr].1 as i128;
        let i = self.pooled("idx", u8t);
        let lim = self.pooled("lim", u8t);
        let byte = self.pooled("byte", u8t);
        let c = self.pooled("cond", Type::Bool);
        self.emit(Inst::Copy { dst: lim, src: n });
        let (clamp, header, body, exit) = (self.new_block(), self.new_block(), self.new_block(), self.new_block());
        self.emit(Inst::Compare { dst: c, op: CmpOp::Gt, lhs: Operand::Place(lim), rhs: Operand::Const { value: len, ty: u8t } });
        self.terminate(Terminator::Branch { cond: Operand::Place(c), then_bb: clamp, else_bb: header });
        self.cur = clamp;
        self.emit(Inst::Copy { dst: lim, src: Operand::Const { value: len, ty: u8t } });
        self.terminate(Terminator::Jump(header));
        self.cur = header;
        self.emit(Inst::Copy { dst: i, src: Operand::Const { value: 0, ty: u8t } });
        let loop_head = self.new_block();
        self.terminate(Terminator::Jump(loop_head));
        self.cur = loop_head;
        self.emit(Inst::Compare { dst: c, op: CmpOp::Lt, lhs: Operand::Place(i), rhs: Operand::Place(lim) });
        self.terminate(Terminator::Branch { cond: Operand::Place(c), then_bb: body, else_bb: exit });
        self.cur = body;
        self.emit(Inst::ArrGet { dst: byte, arr, index: Operand::Place(i) });
        self.uart_put(Operand::Place(byte));
        self.emit(Inst::Binary { dst: i, op: BinOp::Add, lhs: Operand::Place(i), rhs: Operand::Const { value: 1, ty: u8t } });
        self.terminate(Terminator::Jump(loop_head));
        self.cur = exit;
    }

    /// Emite `lhs OP rhs` y salta a `then_bb` o `else_bb`.
    fn jump_if(&mut self, op: CmpOp, lhs: Operand, rhs: Operand, then_bb: BlockId, else_bb: BlockId) {
        let c = self.pooled("cond", Type::Bool);
        self.emit(Inst::Compare { dst: c, op, lhs, rhs });
        self.terminate(Terminator::Branch { cond: Operand::Place(c), then_bb, else_bb });
    }

    /// `parse_dec(buf, n, error)` / `parse_hex(buf, n, error)`: cifra a cifra con aritmetica de 16 bits, con el desbordamiento
    /// comprobado antes de multiplicar (`v > 6553`, o `v == 6553` y la cifra `> 5` en decimal; `v > 0FFFh` en hexadecimal). Un error deja
    /// `error = True` y el valor en 0; la conversion de referencia es `pyxpic_frontend::text::parse`.
    fn parse_text(&mut self, tc: pyxpic_frontend::TextCall, n: &Expr, err: &Expr, dst: Option<Place>) {
        use pyxpic_frontend::TextOp;
        let (u8t, u16t) = (Type::Int { bits: 8, signed: false }, Type::Int { bits: 16, signed: false });
        let k8 = |v: i128| Operand::Const { value: v, ty: u8t };
        let k16 = |v: i128| Operand::Const { value: v, ty: u16t };
        let len = self.prog.arrays[tc.arr].1 as i128;
        let ExprKind::Name(err_name) = &err.kind else { unreachable!("el frontend exige el nombre de una variable") };
        let err_place = self.name_place(err_name);
        let n_op = self.value(n);
        let (cap, i, v, bad) = (self.pooled("p_cap", u8t), self.pooled("idx", u8t), self.pooled("p_val", u16t), self.pooled("p_bad", Type::Bool));
        let (ch, d, d16) = (self.pooled("byte", u8t), self.pooled("digit", u8t), self.pooled("p_d16", u16t));
        let (clamp, after, header, body, digit, ovf, ok_b, notdigit, done, zero, join) = (
            self.new_block(),
            self.new_block(),
            self.new_block(),
            self.new_block(),
            self.new_block(),
            self.new_block(),
            self.new_block(),
            self.new_block(),
            self.new_block(),
            self.new_block(),
            self.new_block(),
        );
        // cap = min(n, capacidad); i = 0; v = 0; bad = False
        self.emit(Inst::Copy { dst: cap, src: n_op });
        self.jump_if(CmpOp::Gt, Operand::Place(cap), k8(len), clamp, after);
        self.cur = clamp;
        self.emit(Inst::Copy { dst: cap, src: k8(len) });
        self.terminate(Terminator::Jump(after));
        self.cur = after;
        self.emit(Inst::Copy { dst: i, src: k8(0) });
        self.emit(Inst::Copy { dst: v, src: k16(0) });
        self.emit(Inst::Copy { dst: bad, src: Operand::Const { value: 0, ty: Type::Bool } });
        let empty = self.new_block();
        match tc.op {
            TextOp::ParseDec => {
                self.jump_if(CmpOp::Eq, Operand::Place(cap), k8(0), empty, header);
            }
            TextOp::ParseHex => {
                // prefijo opcional "0x" / "0X": i = 2
                let (chk0, chk1, pfx, nopfx) = (self.new_block(), self.new_block(), self.new_block(), self.new_block());
                self.jump_if(CmpOp::Ge, Operand::Place(cap), k8(2), chk0, nopfx);
                self.cur = chk0;
                self.emit(Inst::ArrGet { dst: ch, arr: tc.arr, index: k8(0) });
                self.jump_if(CmpOp::Eq, Operand::Place(ch), k8(48), chk1, nopfx);
                self.cur = chk1;
                self.emit(Inst::ArrGet { dst: ch, arr: tc.arr, index: k8(1) });
                self.emit(Inst::Binary { dst: ch, op: BinOp::BitOr, lhs: Operand::Place(ch), rhs: k8(0x20) });
                self.jump_if(CmpOp::Eq, Operand::Place(ch), k8(120), pfx, nopfx);
                self.cur = pfx;
                self.emit(Inst::Copy { dst: i, src: k8(2) });
                self.terminate(Terminator::Jump(nopfx));
                self.cur = nopfx;
                // sin ninguna cifra (vacio o solo el prefijo) es un error
                self.jump_if(CmpOp::Ge, Operand::Place(i), Operand::Place(cap), empty, header);
            }
        }
        self.cur = empty;
        self.emit(Inst::Copy { dst: bad, src: Operand::Const { value: 1, ty: Type::Bool } });
        self.terminate(Terminator::Jump(done));

        self.cur = header;
        self.jump_if(CmpOp::Lt, Operand::Place(i), Operand::Place(cap), body, done);
        self.cur = body;
        self.emit(Inst::ArrGet { dst: ch, arr: tc.arr, index: Operand::Place(i) });
        self.emit(Inst::Binary { dst: d, op: BinOp::Sub, lhs: Operand::Place(ch), rhs: k8(48) });
        match tc.op {
            TextOp::ParseDec => self.jump_if(CmpOp::Le, Operand::Place(d), k8(9), digit, notdigit),
            TextOp::ParseHex => {
                // 0-9 directo; si no, la letra en minuscula menos 'a' debe ser 0..5 (A-F y a-f) y la cifra vale 10 mas
                let (letter, hexl) = (self.new_block(), self.new_block());
                self.jump_if(CmpOp::Le, Operand::Place(d), k8(9), digit, letter);
                self.cur = letter;
                self.emit(Inst::Binary { dst: d, op: BinOp::BitOr, lhs: Operand::Place(ch), rhs: k8(0x20) });
                self.emit(Inst::Binary { dst: d, op: BinOp::Sub, lhs: Operand::Place(d), rhs: k8(97) });
                self.jump_if(CmpOp::Le, Operand::Place(d), k8(5), hexl, notdigit);
                self.cur = hexl;
                self.emit(Inst::Binary { dst: d, op: BinOp::Add, lhs: Operand::Place(d), rhs: k8(10) });
                self.terminate(Terminator::Jump(digit));
            }
        }
        self.cur = notdigit;
        self.emit(Inst::Copy { dst: bad, src: Operand::Const { value: 1, ty: Type::Bool } });
        self.terminate(Terminator::Jump(done));

        // desbordamiento antes de multiplicar
        self.cur = digit;
        match tc.op {
            TextOp::ParseDec => {
                let (chk2, chk3) = (self.new_block(), self.new_block());
                self.jump_if(CmpOp::Gt, Operand::Place(v), k16(6553), ovf, chk2);
                self.cur = chk2;
                self.jump_if(CmpOp::Eq, Operand::Place(v), k16(6553), chk3, ok_b);
                self.cur = chk3;
                self.jump_if(CmpOp::Gt, Operand::Place(d), k8(5), ovf, ok_b);
            }
            TextOp::ParseHex => self.jump_if(CmpOp::Gt, Operand::Place(v), k16(0x0FFF), ovf, ok_b),
        }
        self.cur = ovf;
        self.emit(Inst::Copy { dst: bad, src: Operand::Const { value: 1, ty: Type::Bool } });
        self.terminate(Terminator::Jump(done));
        self.cur = ok_b;
        self.emit(Inst::Cast { dst: d16, src: Operand::Place(d) });
        match tc.op {
            TextOp::ParseDec => {
                // v * 10 + d = (v << 3) + (v << 1) + d
                let (a, b) = (self.pooled("p_a", u16t), self.pooled("p_b", u16t));
                self.emit(Inst::Binary { dst: a, op: BinOp::Shl, lhs: Operand::Place(v), rhs: k8(3) });
                self.emit(Inst::Binary { dst: b, op: BinOp::Shl, lhs: Operand::Place(v), rhs: k8(1) });
                self.emit(Inst::Binary { dst: v, op: BinOp::Add, lhs: Operand::Place(a), rhs: Operand::Place(b) });
                self.emit(Inst::Binary { dst: v, op: BinOp::Add, lhs: Operand::Place(v), rhs: Operand::Place(d16) });
            }
            TextOp::ParseHex => {
                self.emit(Inst::Binary { dst: v, op: BinOp::Shl, lhs: Operand::Place(v), rhs: k8(4) });
                self.emit(Inst::Binary { dst: v, op: BinOp::BitOr, lhs: Operand::Place(v), rhs: Operand::Place(d16) });
            }
        }
        self.emit(Inst::Binary { dst: i, op: BinOp::Add, lhs: Operand::Place(i), rhs: k8(1) });
        self.terminate(Terminator::Jump(header));

        // un error deja el valor en 0; el indicador se escribe siempre (un True viejo no se queda)
        self.cur = done;
        let zero_cond = Operand::Place(bad);
        self.terminate(Terminator::Branch { cond: zero_cond, then_bb: zero, else_bb: join });
        self.cur = zero;
        self.emit(Inst::Copy { dst: v, src: k16(0) });
        self.terminate(Terminator::Jump(join));
        self.cur = join;
        self.emit(Inst::Copy { dst: err_place, src: Operand::Place(bad) });
        if let Some(d) = dst {
            self.emit(Inst::Copy { dst: d, src: Operand::Place(v) });
        }
    }

    fn uart_put(&mut self, ch: Operand) {
        let call = pyxpic_frontend::UartCall { op: pyxpic_frontend::UartOp::Write };
        self.emit(Inst::Uart { call, dst: None, arg: Some(ch) });
    }

    /// `UART.write_text(f"...")`: cada parte en orden. El texto constante se envia con `write_text`; un campo se convierte con
    /// aritmetica y bifurcaciones de la IR (sin division ni memoria dinamica) y cada caracter sale con `UART.write`.
    fn write_fmt(&mut self, arg: &Expr, parts: &[pyxpic_frontend::FmtPart]) {
        let ExprKind::FStr(ast) = &arg.kind else { unreachable!("el frontend solo deja f-strings aqui") };
        for p in parts {
            match *p {
                pyxpic_frontend::FmtPart::Lit(rom) => self.write_text(rom),
                pyxpic_frontend::FmtPart::Field { ast: i, bits, spec, tables } => {
                    let pyxpic_frontend::ast::FPart::Field { expr, .. } = &ast[i] else { unreachable!("el campo existe en el AST") };
                    let v = self.value(expr);
                    if spec.kind == pyxpic_frontend::ast::FKind::Dec {
                        self.fmt_decimal(v, bits, spec, tables);
                    } else {
                        self.fmt_radix(v, bits, spec, tables);
                    }
                }
            }
        }
    }

    /// Decimal por restas sucesivas de 10000, 1000, 100, 10 y 1 (100, 10 y 1 con 8 bits), en un bucle sobre una tabla de potencias de
    /// diez: sin ceros a la izquierda y con el relleno del ancho pedido. El tamano no depende del ancho; el tiempo depende del
    /// valor (hasta 9 restas por cifra).
    fn fmt_decimal(&mut self, v: Operand, bits: u8, spec: pyxpic_frontend::ast::FSpec, tables: [Option<usize>; 2]) {
        let u8t = Type::Int { bits: 8, signed: false };
        let vt = Type::Int { bits, signed: false };
        let n: i128 = if bits == 8 { 3 } else { 5 };
        let w = spec.width as i128;
        let pad = Operand::Const { value: if spec.zero { b'0' } else { b' ' } as i128, ty: u8t };
        for _ in n..w {
            self.uart_put(pad);
        }
        let cur = self.pooled("cur", vt);
        self.emit(Inst::Copy { dst: cur, src: v });
        let started = self.pooled("started", Type::Bool);
        self.emit(Inst::Copy { dst: started, src: Operand::Const { value: 0, ty: Type::Bool } });
        let idx = self.pooled("idx", u8t);
        self.emit(Inst::Copy { dst: idx, src: Operand::Const { value: 0, ty: u8t } });
        let (header, body, exit) = (self.new_block(), self.new_block(), self.new_block());
        self.terminate(Terminator::Jump(header));
        self.cur = header;
        let c = self.pooled("cond", Type::Bool);
        self.emit(Inst::Compare { dst: c, op: CmpOp::Lt, lhs: Operand::Place(idx), rhs: Operand::Const { value: n, ty: u8t } });
        self.terminate(Terminator::Branch { cond: Operand::Place(c), then_bb: body, else_bb: exit });
        self.cur = body;
        // p = potencia de diez de esta vuelta, de la tabla (el byte alto solo con 16 bits)
        let lo = self.pooled("lo", u8t);
        self.emit(Inst::RomGet { dst: lo, rom: tables[0].expect("el frontend da la tabla de potencias de diez"), index: Operand::Place(idx) });
        let p = if bits == 8 {
            Operand::Place(lo)
        } else {
            let hi = self.pooled("hi", u8t);
            self.emit(Inst::RomGet { dst: hi, rom: tables[1].expect("el frontend da la tabla del byte alto"), index: Operand::Place(idx) });
            let pw = self.pooled("p16", vt);
            self.emit(Inst::Cast { dst: pw, src: Operand::Place(hi) });
            self.emit(Inst::Binary { dst: pw, op: BinOp::Shl, lhs: Operand::Place(pw), rhs: Operand::Const { value: 8, ty: u8t } });
            let lw = self.pooled("l16", vt);
            self.emit(Inst::Cast { dst: lw, src: Operand::Place(lo) });
            self.emit(Inst::Binary { dst: pw, op: BinOp::BitOr, lhs: Operand::Place(pw), rhs: Operand::Place(lw) });
            Operand::Place(pw)
        };
        let d = self.pooled("digit", u8t);
        self.emit(Inst::Copy { dst: d, src: Operand::Const { value: 0, ty: u8t } });
        let (ih, ib, ix) = (self.new_block(), self.new_block(), self.new_block());
        self.terminate(Terminator::Jump(ih));
        self.cur = ih;
        self.emit(Inst::Compare { dst: c, op: CmpOp::Ge, lhs: Operand::Place(cur), rhs: p });
        self.terminate(Terminator::Branch { cond: Operand::Place(c), then_bb: ib, else_bb: ix });
        self.cur = ib;
        self.emit(Inst::Binary { dst: cur, op: BinOp::Sub, lhs: Operand::Place(cur), rhs: p });
        self.emit(Inst::Binary { dst: d, op: BinOp::Add, lhs: Operand::Place(d), rhs: Operand::Const { value: 1, ty: u8t } });
        self.terminate(Terminator::Jump(ih));
        self.cur = ix;
        // se escribe la cifra si no es cero, si ya se escribio una, o si es la ultima (el valor 0 se escribe "0")
        let (emit_bb, chk_started, chk_last, pad_bb, join) = (self.new_block(), self.new_block(), self.new_block(), self.new_block(), self.new_block());
        let nz = self.pooled("nz", Type::Bool);
        self.emit(Inst::Compare { dst: nz, op: CmpOp::Ne, lhs: Operand::Place(d), rhs: Operand::Const { value: 0, ty: u8t } });
        self.terminate(Terminator::Branch { cond: Operand::Place(nz), then_bb: emit_bb, else_bb: chk_started });
        self.cur = chk_started;
        self.terminate(Terminator::Branch { cond: Operand::Place(started), then_bb: emit_bb, else_bb: chk_last });
        self.cur = chk_last;
        self.emit(Inst::Compare { dst: nz, op: CmpOp::Eq, lhs: Operand::Place(idx), rhs: Operand::Const { value: n - 1, ty: u8t } });
        self.terminate(Terminator::Branch { cond: Operand::Place(nz), then_bb: emit_bb, else_bb: pad_bb });
        self.cur = pad_bb;
        if w > 0 {
            // relleno si esta posicion (contada desde la derecha: n-1-idx) cabe en el ancho pedido
            let pos = self.pooled("pos", u8t);
            self.emit(Inst::Binary { dst: pos, op: BinOp::Sub, lhs: Operand::Const { value: n - 1, ty: u8t }, rhs: Operand::Place(idx) });
            let (put, skip) = (self.new_block(), self.new_block());
            self.emit(Inst::Compare { dst: nz, op: CmpOp::Lt, lhs: Operand::Place(pos), rhs: Operand::Const { value: w, ty: u8t } });
            self.terminate(Terminator::Branch { cond: Operand::Place(nz), then_bb: put, else_bb: skip });
            self.cur = put;
            self.uart_put(pad);
            self.terminate(Terminator::Jump(skip));
            self.cur = skip;
        }
        self.terminate(Terminator::Jump(join));
        self.cur = emit_bb;
        self.emit(Inst::Copy { dst: started, src: Operand::Const { value: 1, ty: Type::Bool } });
        let ch = self.pooled("char", u8t);
        self.emit(Inst::Binary { dst: ch, op: BinOp::Add, lhs: Operand::Place(d), rhs: Operand::Const { value: 48, ty: u8t } });
        self.uart_put(Operand::Place(ch));
        self.terminate(Terminator::Jump(join));
        self.cur = join;
        self.emit(Inst::Binary { dst: idx, op: BinOp::Add, lhs: Operand::Place(idx), rhs: Operand::Const { value: 1, ty: u8t } });
        self.terminate(Terminator::Jump(header));
        self.cur = exit;
    }

    /// Hexadecimal (cuatro bits por digito, con una tabla `Rom`) o binario (un bit por digito): ancho fijo, con ceros a la
    /// izquierda. Un bucle que toma el digito alto y desplaza el valor: el tamano y el costo por digito no dependen del valor.
    fn fmt_radix(&mut self, v: Operand, bits: u8, spec: pyxpic_frontend::ast::FSpec, tables: [Option<usize>; 2]) {
        let u8t = Type::Int { bits: 8, signed: false };
        let vt = Type::Int { bits, signed: false };
        let bin = spec.kind == pyxpic_frontend::ast::FKind::Bin;
        let (per, mask) = if bin { (1i128, 1i128) } else { (4, 15) };
        let digits = bits as i128 / per;
        for _ in digits..spec.width as i128 {
            self.uart_put(Operand::Const { value: b'0' as i128, ty: u8t });
        }
        let cur = self.pooled("cur", vt);
        self.emit(Inst::Copy { dst: cur, src: v });
        let idx = self.pooled("idx", u8t);
        self.emit(Inst::Copy { dst: idx, src: Operand::Const { value: 0, ty: u8t } });
        let (header, body, exit) = (self.new_block(), self.new_block(), self.new_block());
        self.terminate(Terminator::Jump(header));
        self.cur = header;
        let c = self.pooled("cond", Type::Bool);
        self.emit(Inst::Compare { dst: c, op: CmpOp::Lt, lhs: Operand::Place(idx), rhs: Operand::Const { value: digits, ty: u8t } });
        self.terminate(Terminator::Branch { cond: Operand::Place(c), then_bb: body, else_bb: exit });
        self.cur = body;
        let top = self.pooled("shifted", vt);
        self.emit(Inst::Binary { dst: top, op: BinOp::Shr, lhs: Operand::Place(cur), rhs: Operand::Const { value: bits as i128 - per, ty: u8t } });
        let low = self.pooled("low", u8t);
        self.emit(Inst::Cast { dst: low, src: Operand::Place(top) });
        let digit = self.pooled("digit", u8t);
        self.emit(Inst::Binary { dst: digit, op: BinOp::BitAnd, lhs: Operand::Place(low), rhs: Operand::Const { value: mask, ty: u8t } });
        let ch = self.pooled("char", u8t);
        if bin {
            self.emit(Inst::Binary { dst: ch, op: BinOp::Add, lhs: Operand::Place(digit), rhs: Operand::Const { value: 48, ty: u8t } });
        } else {
            self.emit(Inst::RomGet { dst: ch, rom: tables[0].expect("el frontend da la tabla de digitos hexadecimales"), index: Operand::Place(digit) });
        }
        self.uart_put(Operand::Place(ch));
        self.emit(Inst::Binary { dst: cur, op: BinOp::Shl, lhs: Operand::Place(cur), rhs: Operand::Const { value: per, ty: u8t } });
        self.emit(Inst::Binary { dst: idx, op: BinOp::Add, lhs: Operand::Place(idx), rhs: Operand::Const { value: 1, ty: u8t } });
        self.terminate(Terminator::Jump(header));
        self.cur = exit;
    }

    /// `UART.write_text`: `i = 0; mientras i < n: UART.write(TABLA[i]); i += 1`. Dos temporales de un byte y nada mas en RAM.
    fn write_text(&mut self, rom: usize) {
        let u8t = Type::Int { bits: 8, signed: false };
        let n = self.prog.roms[rom].1.len() as i128;
        let i = self.pooled("idx", u8t);
        let byte = self.pooled("byte", u8t);
        self.emit(Inst::Copy { dst: i, src: Operand::Const { value: 0, ty: u8t } });
        let (header, body, exit) = (self.new_block(), self.new_block(), self.new_block());
        self.terminate(Terminator::Jump(header));
        self.cur = header;
        let c = self.pooled("cond", Type::Bool);
        self.emit(Inst::Compare { dst: c, op: CmpOp::Lt, lhs: Operand::Place(i), rhs: Operand::Const { value: n, ty: u8t } });
        self.terminate(Terminator::Branch { cond: Operand::Place(c), then_bb: body, else_bb: exit });
        self.cur = body;
        self.emit(Inst::RomGet { dst: byte, rom, index: Operand::Place(i) });
        let call = pyxpic_frontend::UartCall { op: pyxpic_frontend::UartOp::Write };
        self.emit(Inst::Uart { call, dst: None, arg: Some(Operand::Place(byte)) });
        self.emit(Inst::Binary { dst: i, op: BinOp::Add, lhs: Operand::Place(i), rhs: Operand::Const { value: 1, ty: u8t } });
        self.terminate(Terminator::Jump(header));
        self.cur = exit;
    }

    fn for_range(&mut self, var: &Ident, args: &[Expr], body: &[Stmt]) {
        let vt = self.info(&args[0]).ty;
        let place = match self.lmap.get(&var.name) {
            Some(&i) => Place::Local(i),
            None => self.declare_local(&var.name, vt, false),
        };
        let zero = Operand::Const { value: 0, ty: vt };
        let (start, stop_e) = if args.len() == 1 { (zero, &args[0]) } else { (self.value(&args[0]), &args[1]) };
        self.emit(Inst::Copy { dst: place, src: start });
        // `stop` se evalua una sola vez
        let stop = match self.value(stop_e) {
            c @ Operand::Const { .. } => c,
            other => {
                let t = self.new_temp(vt);
                self.emit(Inst::Copy { dst: t, src: other });
                Operand::Place(t)
            }
        };
        let step = args.get(2).and_then(|a| self.info(a).konst).unwrap_or(1);
        let up = step > 0;

        let (header, bodyb, latch, stepb, exit) = (self.new_block(), self.new_block(), self.new_block(), self.new_block(), self.new_block());
        self.terminate(Terminator::Jump(header));
        self.cur = header;
        let c = self.new_temp(Type::Bool);
        let (op, l, r) = if up { (CmpOp::Lt, Operand::Place(place), stop) } else { (CmpOp::Gt, Operand::Place(place), stop) };
        self.emit(Inst::Compare { dst: c, op, lhs: l, rhs: r });
        self.terminate(Terminator::Branch { cond: Operand::Place(c), then_bb: bodyb, else_bb: exit });

        self.cur = bodyb;
        self.loops.push((exit, latch));
        self.block(body);
        self.loops.pop();
        self.terminate(Terminator::Jump(latch));

        self.cur = latch;
        let ut = Type::Int { bits: vt.bits() as u8, signed: false };
        let diff = self.new_temp(vt);
        let (a, b) = if up { (stop, Operand::Place(place)) } else { (Operand::Place(place), stop) };
        self.emit(Inst::Binary { dst: diff, op: BinOp::Sub, lhs: a, rhs: b });
        let udiff = self.new_temp(ut);
        self.emit(Inst::Cast { dst: udiff, src: Operand::Place(diff) });
        let last = self.new_temp(Type::Bool);
        self.emit(Inst::Compare { dst: last, op: CmpOp::Le, lhs: Operand::Place(udiff), rhs: Operand::Const { value: step.abs(), ty: ut } });
        self.terminate(Terminator::Branch { cond: Operand::Place(last), then_bb: exit, else_bb: stepb });

        self.cur = stepb;
        self.emit(Inst::Binary { dst: place, op: BinOp::Add, lhs: Operand::Place(place), rhs: Operand::Const { value: step, ty: vt } });
        self.terminate(Terminator::Jump(bodyb));
        self.cur = exit;
    }
}
