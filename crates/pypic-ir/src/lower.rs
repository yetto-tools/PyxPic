//! Bajada del `Program` tipado del frontend a la IR.

use std::collections::HashMap;

use pypic_frontend::ast::*;
use pypic_frontend::Program;

use crate::ir::{Block, BlockId, Inst, Operand, Place, Terminator, UnaryOp};
use crate::ir::{AsmBlock, Function, Global, Local, Module, Pin, RegOp, TimerOp, Type};
use pypic_frontend::BitsOp;

pub fn lower(prog: &Program) -> Module {
    let mut m = Module::default();
    m.device = prog.device.clone();
    m.clock_hz = prog.clock_hz;
    m.clock_defaulted = prog.clock_defaulted;
    m.tables = prog.tables.clone();
    let mut gidx = HashMap::new();
    for it in &prog.module.items {
        if let Item::Var(v) = it {
            // las tablas viven en la memoria de programa, no son globales
            if prog.tables.iter().any(|t| t.0 == v.name.name) {
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
                blocks: vec![(Vec::new(), None)],
                cur: 0,
                loops: Vec::new(),
            };
            for (name, ty) in &info.params {
                fl.declare_local(name, *ty, true);
            }
            fl.block(&f.body);
            m.functions.push(fl.finish(info.name.clone(), info.is_interrupt, info.is_init, info.params.len(), info.ret, f.asm.as_ref().map(|a| AsmBlock { text: a.text.clone(), first_line: a.first_line })));
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
    blocks: Vec<(Vec<Inst>, Option<Terminator>)>,
    cur: BlockId,
    /// (destino de break, destino de continue)
    loops: Vec<(BlockId, BlockId)>,
}

fn ann_type(te: &TypeExpr) -> Type {
    match &te.kind {
        TypeExprKind::Name(n) => Type::from_name(n).unwrap_or(Type::Error),
        TypeExprKind::Final(inner) => ann_type(inner),
        TypeExprKind::None => Type::Void,
        TypeExprKind::Table(_) => Type::Error,
    }
}

impl<'a> FnLower<'a> {
    fn finish(self, name: String, is_interrupt: bool, is_init: bool, num_params: usize, ret: Type, asm: Option<AsmBlock>) -> Function {
        let blocks = self
            .blocks
            .into_iter()
            .map(|(insts, term)| Block {
                insts,
                term: term.unwrap_or(if ret == Type::Void { Terminator::Return(None) } else { Terminator::Unreachable }),
            })
            .collect();
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

    fn new_block(&mut self) -> BlockId {
        self.blocks.push((Vec::new(), None));
        self.blocks.len() - 1
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

    fn info(&self, e: &Expr) -> pypic_frontend::check::ExprInfo {
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

    fn compute(&mut self, e: &Expr, dst: Option<Place>) {
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
            ExprKind::Index { index, .. } => {
                let table = self.prog.table_gets[&e.id];
                let index = self.value(index);
                self.emit(Inst::TableGet { dst: dst.unwrap(), table, index });
            }
            ExprKind::List(_) => unreachable!("el frontend rechaza las listas fuera de una tabla"),
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
                if let (Some(unit), false) = (pypic_frontend::time_unit(func), self.fidx.contains_key(func)) {
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
            ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Name(_) => {
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

        let (header, bodyb, latch, stepb, exit) =
            (self.new_block(), self.new_block(), self.new_block(), self.new_block(), self.new_block());
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
        self.emit(Inst::Compare {
            dst: last,
            op: CmpOp::Le,
            lhs: Operand::Place(udiff),
            rhs: Operand::Const { value: step.abs(), ty: ut },
        });
        self.terminate(Terminator::Branch { cond: Operand::Place(last), then_bb: exit, else_bb: stepb });

        self.cur = stepb;
        self.emit(Inst::Binary {
            dst: place,
            op: BinOp::Add,
            lhs: Operand::Place(place),
            rhs: Operand::Const { value: step, ty: vt },
        });
        self.terminate(Terminator::Jump(bodyb));
        self.cur = exit;
    }
}
