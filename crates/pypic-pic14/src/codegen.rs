//! Generador de codigo PIC14: IR tipada -> instrucciones nativas (ABI en docs/abi-pic14.md).
//!
//! Primer corte: enteros de 8 y 16 bits y bool. Todos los datos en el banco 0 (20h-6Fh), asi se
//! cumple la invariante de banco 0 sin cambiar de banco. Sin multiplicacion/division ni
//! desplazamientos por variable (necesitan rutinas de runtime) ni u32/i32. Programas de hasta
//! 2048 palabras (no se maneja PCLATH). Todo corre en el host; el MCU ejecuta solo instrucciones nativas.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use pypic_ir::ir::{BinOp, CmpOp, Function, Module, Operand, Pin, Place, Terminator, Type, UnaryOp};

use crate::hex::Image;
use crate::isa::{disassemble, Dest::{F, W}, Inst, Inst::*};
use crate::samples::{BLINK_CONFIG1, BLINK_CONFIG2, CONFIG1, CONFIG2};

// ---- registros y zonas de memoria (DS40001291H p.27-28; reparto en docs/abi-pic14.md) ----------
const STATUS: u8 = 0x03;
const PCLATH: u8 = 0x0A;
const BIT_C: u8 = 0;
const BIT_Z: u8 = 2;
const BIT_RP0: u8 = 5;
const BIT_RP1: u8 = 6;
const BIT_IRP: u8 = 7;
/// Zona comun 70h-7Fh (visible en todos los bancos): contexto de la ISR, temporales y retorno de 16 bits.
const W_TEMP: u8 = 0x70;
const STATUS_TEMP: u8 = 0x71;
const PCLATH_TEMP: u8 = 0x72;
const SCRATCH_MAIN: u8 = 0x74; // 74h-77h
const SCRATCH_ISR: u8 = 0x78; // 78h-7Bh
const RETV_MAIN: u8 = 0x7C;
const RETV_ISR: u8 = 0x7D;
/// RAM de propiedad del compilador en el banco 0.
const RAM_START: u16 = 0x20;
const RAM_END: u16 = 0x6F;
const PAGE_WORDS: usize = 2048;
/// PORTA..PORTE estan en 05h-09h (banco 0) y TRISA..TRISE en 85h-89h (banco 1): mismo desplazamiento de 7 bits.
/// ANSEL=188h y ANSELH=189h (banco 3). DS40001291H p.27-29. TODO: sacarlos del Device Pack (registers.json, #22).
const ANSEL: u8 = 0x08;
const ANSELH: u8 = 0x09;

/// (registro de 7 bits de PORTx/TRISx, bit) de un pin; solo puertos A-E del PIC16F88x.
fn port_reg(p: &Pin) -> Result<(u8, u8), CodegenError> {
    if !('A'..='E').contains(&p.port) || p.bit > 7 {
        return unsupported(format!("el pin {} (el backend PIC16F88x solo maneja los puertos A a E, bits 0-7)", p.name));
    }
    Ok((0x05 + (p.port as u8 - b'A'), p.bit))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenError {
    NoMain,
    Unsupported(String),
    OutOfRam { needed: usize, available: usize },
    TooLarge { words: usize },
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodegenError::NoMain => f.write_str("falta la funcion 'main'"),
            CodegenError::Unsupported(s) => write!(f, "no soportado todavia por el backend PIC14: {s}"),
            CodegenError::OutOfRam { needed, available } => write!(
                f,
                "la RAM del banco 0 no alcanza ({needed} bytes necesarios, {available} disponibles); el uso de otros bancos y el solapamiento de locales aun no estan implementados"
            ),
            CodegenError::TooLarge { words } => write!(f, "el programa ocupa {words} palabras; el limite actual es una pagina de {PAGE_WORDS} (PCLATH no se maneja todavia)"),
        }
    }
}

impl std::error::Error for CodegenError {}

fn unsupported<T>(msg: impl Into<String>) -> Result<T, CodegenError> {
    Err(CodegenError::Unsupported(msg.into()))
}

/// Palabras de configuracion que se escriben en el HEX.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub config1: u16,
    pub config2: u16,
}

impl Default for Options {
    /// Oscilador interno INTOSCIO, WDT apagado, MCLR interno (los mismos que `blink_rd0`).
    /// TODO: tomarlos del Device Pack / pypic.toml.
    fn default() -> Self {
        Options { config1: BLINK_CONFIG1, config2: BLINK_CONFIG2 }
    }
}

#[derive(Debug, Clone)]
pub struct GlobalSym {
    pub name: String,
    pub addr: u8,
    pub ty: Type,
}

#[derive(Debug)]
pub struct Compiled {
    pub image: Image,
    pub globals: Vec<GlobalSym>,
    /// (nombre, direccion de palabra del punto de entrada).
    pub functions: Vec<(String, u32)>,
    pub listing: String,
    pub flash_words: usize,
    pub ram_bytes: usize,
}

// ---- ensamblado con etiquetas ---------------------------------------------------------------
type Label = usize;

enum Item {
    Inst(Inst),
    Goto(Label),
    Call(Label),
    Label(Label),
    Org(u32),
}

#[derive(Default)]
struct Asm {
    items: Vec<Item>,
    names: Vec<String>,
}

impl Asm {
    fn label(&mut self, name: impl Into<String>) -> Label {
        self.names.push(name.into());
        self.names.len() - 1
    }
    fn i(&mut self, inst: Inst) {
        self.items.push(Item::Inst(inst));
    }
    fn place(&mut self, l: Label) {
        self.items.push(Item::Label(l));
    }
    fn goto(&mut self, l: Label) {
        self.items.push(Item::Goto(l));
    }
    fn call(&mut self, l: Label) {
        self.items.push(Item::Call(l));
    }
}

// ---- asignacion de memoria ----------------------------------------------------------------
fn size(ty: Type) -> Result<usize, CodegenError> {
    match ty {
        Type::Bool => Ok(1),
        Type::Int { bits: 8, .. } => Ok(1),
        Type::Int { bits: 16, .. } => Ok(2),
        Type::Void => Ok(0),
        other => unsupported(format!("el tipo {other} (solo 8 y 16 bits y bool)")),
    }
}

#[derive(Clone, Copy)]
enum B {
    Imm(u8),
    Reg(u8),
}

struct Alloc {
    globals: Vec<u8>,
    locals: HashMap<usize, Vec<u8>>,
    temps: HashMap<usize, Vec<u8>>,
    used: usize,
}

fn allocate(m: &Module, live: &[usize]) -> Result<Alloc, CodegenError> {
    let mut next = RAM_START as usize;
    let mut take = |n: usize| -> u8 {
        let a = next;
        next += n;
        a as u8
    };
    let mut globals = Vec::new();
    for g in &m.globals {
        let n = size(g.ty)?;
        globals.push(take(n));
    }
    let mut locals = HashMap::new();
    let mut temps = HashMap::new();
    for &fi in live {
        let f = &m.functions[fi];
        locals.insert(fi, f.locals.iter().map(|l| size(l.ty).map(&mut take)).collect::<Result<Vec<_>, _>>()?);
        temps.insert(fi, f.temps.iter().map(|&t| size(t).map(&mut take)).collect::<Result<Vec<_>, _>>()?);
    }
    let used = next - RAM_START as usize;
    let available = (RAM_END - RAM_START + 1) as usize;
    if used > available {
        return Err(CodegenError::OutOfRam { needed: used, available });
    }
    Ok(Alloc { globals, locals, temps, used })
}

// ---- grafo de llamadas -----------------------------------------------------------------------
fn callees(f: &Function) -> HashSet<usize> {
    f.blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .filter_map(|i| if let pypic_ir::ir::Inst::Call { func, .. } = i { Some(*func) } else { None })
        .collect()
}

fn reach(m: &Module, roots: &[usize]) -> HashSet<usize> {
    let mut seen = HashSet::new();
    let mut stack: Vec<usize> = roots.to_vec();
    while let Some(f) = stack.pop() {
        if seen.insert(f) {
            stack.extend(callees(&m.functions[f]));
        }
    }
    seen
}

fn reachable_blocks(f: &Function) -> Vec<bool> {
    let mut seen = vec![false; f.blocks.len()];
    let mut stack = vec![0usize];
    while let Some(b) = stack.pop() {
        if std::mem::replace(&mut seen[b], true) {
            continue;
        }
        match &f.blocks[b].term {
            Terminator::Jump(t) => stack.push(*t),
            Terminator::Branch { then_bb, else_bb, .. } => stack.extend([*then_bb, *else_bb]),
            _ => {}
        }
    }
    seen
}

// ---- generador ------------------------------------------------------------------------------
struct Gen<'a> {
    m: &'a Module,
    al: Alloc,
    asm: Asm,
    fn_labels: HashMap<usize, Label>,
    isr_ctx: HashSet<usize>,
    cur: usize,
}

impl<'a> Gen<'a> {
    fn f(&self) -> &'a Function {
        &self.m.functions[self.cur]
    }

    fn in_isr(&self) -> bool {
        self.isr_ctx.contains(&self.cur)
    }

    fn scratch(&self) -> u8 {
        if self.in_isr() { SCRATCH_ISR } else { SCRATCH_MAIN }
    }

    fn retv(&self) -> u8 {
        if self.in_isr() { RETV_ISR } else { RETV_MAIN }
    }

    fn place_addr(&self, p: Place) -> u8 {
        match p {
            Place::Global(i) => self.al.globals[i as usize],
            Place::Local(i) => self.al.locals[&self.cur][i as usize],
            Place::Temp(i) => self.al.temps[&self.cur][i as usize],
        }
    }

    fn ty_of(&self, op: &Operand) -> Type {
        self.m.operand_ty(self.f(), op)
    }

    fn bytes(&self, op: &Operand) -> Result<Vec<B>, CodegenError> {
        let ty = self.ty_of(op);
        let n = size(ty)?;
        Ok(match *op {
            Operand::Const { value, .. } => {
                let v = value.rem_euclid(1i128 << (8 * n)) as u64;
                (0..n).map(|i| B::Imm((v >> (8 * i)) as u8)).collect()
            }
            Operand::Place(p) => {
                let a = self.place_addr(p);
                (0..n).map(|i| B::Reg(a + i as u8)).collect()
            }
        })
    }

    fn regs(&self, p: Place) -> Result<Vec<u8>, CodegenError> {
        let n = size(self.m.place_ty(self.f(), p))?;
        let a = self.place_addr(p);
        Ok((0..n).map(|i| a + i as u8).collect())
    }

    // ---- primitivas (W es el unico acumulador) ----
    fn ld(&mut self, b: B) {
        self.asm.i(match b {
            B::Imm(k) => Movlw(k),
            B::Reg(r) => Movf(r, W),
        });
    }
    fn st(&mut self, r: u8) {
        self.asm.i(Movwf(r));
    }
    fn add_w(&mut self, b: B) {
        self.asm.i(match b {
            B::Imm(k) => Addlw(k),
            B::Reg(r) => Addwf(r, W),
        });
    }
    /// W = b - W
    fn sub_from(&mut self, b: B) {
        self.asm.i(match b {
            B::Imm(k) => Sublw(k),
            B::Reg(r) => Subwf(r, W),
        });
    }
    fn and_w(&mut self, b: B) {
        self.asm.i(match b {
            B::Imm(k) => Andlw(k),
            B::Reg(r) => Andwf(r, W),
        });
    }
    fn or_w(&mut self, b: B) {
        self.asm.i(match b {
            B::Imm(k) => Iorlw(k),
            B::Reg(r) => Iorwf(r, W),
        });
    }
    fn xor_w(&mut self, b: B) {
        self.asm.i(match b {
            B::Imm(k) => Xorlw(k),
            B::Reg(r) => Xorwf(r, W),
        });
    }

    // ---- instrucciones de IR ----
    fn copy(&mut self, dst: Place, src: &Operand) -> Result<(), CodegenError> {
        let (d, s) = (self.regs(dst)?, self.bytes(src)?);
        for (r, b) in d.into_iter().zip(s) {
            self.ld(b);
            self.st(r);
        }
        Ok(())
    }

    fn binary(&mut self, dst: Place, op: BinOp, lhs: &Operand, rhs: &Operand) -> Result<(), CodegenError> {
        let d = self.regs(dst)?;
        let n = d.len();
        match op {
            BinOp::Add | BinOp::Sub => {
                let (l, r) = (self.bytes(lhs)?, self.bytes(rhs)?);
                for i in 0..n {
                    if op == BinOp::Add {
                        self.ld(l[i]);
                        if i > 0 {
                            self.asm.i(Btfsc(STATUS, BIT_C));
                            self.asm.i(Addlw(1)); // acarreo del byte anterior
                        }
                        self.add_w(r[i]);
                    } else {
                        self.ld(r[i]);
                        if i > 0 {
                            self.asm.i(Btfss(STATUS, BIT_C));
                            self.asm.i(Addlw(1)); // prestamo del byte anterior (modular: correcto aun con 0xFF + 1)
                        }
                        self.sub_from(l[i]);
                    }
                    self.st(d[i]);
                }
            }
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
                let (l, r) = (self.bytes(lhs)?, self.bytes(rhs)?);
                for i in 0..n {
                    self.ld(l[i]);
                    match op {
                        BinOp::BitAnd => self.and_w(r[i]),
                        BinOp::BitOr => self.or_w(r[i]),
                        _ => self.xor_w(r[i]),
                    }
                    self.st(d[i]);
                }
            }
            BinOp::Shl | BinOp::Shr => {
                let Operand::Const { value: k, .. } = *rhs else {
                    return unsupported("desplazamiento por una cantidad variable (necesita rutina de runtime, #27)");
                };
                let l = self.bytes(lhs)?;
                for i in 0..n {
                    self.ld(l[i]);
                    self.st(d[i]);
                }
                let signed = self.m.place_ty(self.f(), dst).is_signed();
                for _ in 0..k {
                    match (op, signed) {
                        (BinOp::Shl, _) => {
                            self.asm.i(Bcf(STATUS, BIT_C));
                            for &r in &d {
                                self.asm.i(Rlf(r, F));
                            }
                        }
                        (_, false) => {
                            self.asm.i(Bcf(STATUS, BIT_C));
                            for &r in d.iter().rev() {
                                self.asm.i(Rrf(r, F));
                            }
                        }
                        (_, true) => {
                            let hi = *d.last().unwrap();
                            self.asm.i(Rlf(hi, W)); // C = signo
                            for &r in d.iter().rev() {
                                self.asm.i(Rrf(r, F));
                            }
                        }
                    }
                }
            }
            BinOp::Mul | BinOp::Div | BinOp::Mod => {
                return unsupported(format!("la operacion '{}' (necesita rutina de runtime, #27)", op.symbol()));
            }
        }
        Ok(())
    }

    fn unary(&mut self, dst: Place, op: UnaryOp, src: &Operand) -> Result<(), CodegenError> {
        let (d, s) = (self.regs(dst)?, self.bytes(src)?);
        match op {
            UnaryOp::Not => {
                self.ld(s[0]);
                self.asm.i(Xorlw(1));
                self.st(d[0]);
            }
            UnaryOp::Invert => {
                for i in 0..d.len() {
                    self.ld(s[i]);
                    self.asm.i(Xorlw(0xFF));
                    self.st(d[i]);
                }
            }
            UnaryOp::Neg => {
                // -x = ~x + 1, con acarreo entre bytes
                for i in 0..d.len() {
                    self.ld(s[i]);
                    self.asm.i(Xorlw(0xFF));
                    if i == 0 {
                        self.asm.i(Addlw(1));
                    } else {
                        self.asm.i(Btfsc(STATUS, BIT_C));
                        self.asm.i(Addlw(1));
                    }
                    self.st(d[i]);
                }
            }
        }
        Ok(())
    }

    /// Deja C = (a >= b) con la semantica (con o sin signo) del tipo.
    fn sub_flags(&mut self, a: &[B], b: &[B], signed: bool) {
        let s0 = self.scratch();
        let hi = a.len() - 1;
        if signed {
            // a ^ 0x80 y b ^ 0x80 convierten la comparacion con signo en una sin signo
            self.ld(a[hi]);
            self.asm.i(Xorlw(0x80));
            self.st(s0);
            self.ld(b[hi]);
            self.asm.i(Xorlw(0x80));
            self.asm.i(Subwf(s0, W));
        } else {
            self.ld(b[hi]);
            self.sub_from(a[hi]);
        }
        if hi > 0 {
            let done = self.asm.label("cmp.fin");
            self.asm.i(Btfss(STATUS, BIT_Z));
            self.asm.goto(done); // los bytes altos difieren: C ya es el resultado
            self.ld(b[0]);
            self.sub_from(a[0]);
            self.asm.place(done);
        }
    }

    fn compare(&mut self, dst: Place, op: CmpOp, lhs: &Operand, rhs: &Operand) -> Result<(), CodegenError> {
        let d = self.regs(dst)?[0];
        let ty = self.ty_of(lhs);
        let signed = ty.is_signed();
        let (l, r) = (self.bytes(lhs)?, self.bytes(rhs)?);
        match op {
            CmpOp::Eq | CmpOp::Ne => {
                self.ld(l[0]);
                self.xor_w(r[0]);
                if l.len() == 2 {
                    let s0 = self.scratch();
                    self.st(s0);
                    self.ld(l[1]);
                    self.xor_w(r[1]);
                    self.asm.i(Iorwf(s0, W));
                }
                self.asm.i(Movlw(0));
                self.st(d);
                self.asm.i(if op == CmpOp::Eq { Btfsc(STATUS, BIT_Z) } else { Btfss(STATUS, BIT_Z) });
                self.asm.i(Incf(d, F));
            }
            _ => {
                // a >= b deja C=1; Lt/Gt son verdaderos con C=0, Ge/Le con C=1 (Gt y Le intercambian operandos)
                let (a, b, true_when_c) = match op {
                    CmpOp::Lt => (&l, &r, false),
                    CmpOp::Ge => (&l, &r, true),
                    CmpOp::Gt => (&r, &l, false),
                    _ => (&r, &l, true),
                };
                self.sub_flags(a, b, signed);
                self.asm.i(Movlw(0));
                self.st(d);
                self.asm.i(if true_when_c { Btfsc(STATUS, BIT_C) } else { Btfss(STATUS, BIT_C) });
                self.asm.i(Incf(d, F));
            }
        }
        Ok(())
    }

    fn cast(&mut self, dst: Place, src: &Operand) -> Result<(), CodegenError> {
        let dty = self.m.place_ty(self.f(), dst);
        let d = self.regs(dst)?;
        if let Operand::Const { value, .. } = *src {
            let v = dty.wrap(value).rem_euclid(1i128 << (8 * d.len())) as u64;
            for (i, &r) in d.iter().enumerate() {
                self.asm.i(Movlw((v >> (8 * i)) as u8));
                self.st(r);
            }
            return Ok(());
        }
        let s = self.bytes(src)?;
        let signed_src = self.ty_of(src).is_signed();
        for i in 0..d.len().min(s.len()) {
            self.ld(s[i]);
            self.st(d[i]);
        }
        for i in s.len()..d.len() {
            // extension: con signo copia el bit 7 del ultimo byte; sin signo, ceros
            self.asm.i(Clrf(d[i]));
            if signed_src {
                if let B::Reg(last) = s[s.len() - 1] {
                    self.asm.i(Btfsc(last, 7));
                    self.asm.i(Decf(d[i], F));
                }
            }
        }
        Ok(())
    }

    fn call_inst(&mut self, dst: Option<Place>, func: usize, args: &[Operand]) -> Result<(), CodegenError> {
        let callee = &self.m.functions[func];
        for (i, a) in args.iter().enumerate() {
            let slot = self.al.locals[&func][i];
            let n = size(callee.locals[i].ty)?;
            let bytes = self.bytes(a)?;
            for (k, b) in bytes.into_iter().enumerate().take(n) {
                self.ld(b);
                self.st(slot + k as u8);
            }
        }
        let l = self.fn_labels[&func];
        self.asm.call(l);
        if let Some(d) = dst {
            let d = self.regs(d)?;
            self.st(d[0]); // el byte bajo vuelve en W
            if d.len() == 2 {
                let r = self.retv();
                self.asm.i(Movf(r, W));
                self.st(d[1]);
            }
        }
        Ok(())
    }

    /// Pasa el pin a digital (si era analogico) y fija su direccion. Los TRIS estan en el banco 1 y
    /// ANSEL/ANSELH en el banco 3; siempre se vuelve al banco 0 (invariante de la ABI).
    fn pin_cfg(&mut self, pin: usize, output: bool) -> Result<(), CodegenError> {
        let p = &self.m.pins[pin];
        let (reg, bit) = port_reg(p)?;
        if let Some(an) = p.analog {
            let (r, b) = if an < 8 { (ANSEL, an) } else { (ANSELH, an - 8) };
            self.asm.i(Bsf(STATUS, BIT_RP0));
            self.asm.i(Bsf(STATUS, BIT_RP1));
            self.asm.i(Bcf(r, b));
            self.asm.i(Bcf(STATUS, BIT_RP0));
            self.asm.i(Bcf(STATUS, BIT_RP1));
        }
        self.asm.i(Bsf(STATUS, BIT_RP0));
        self.asm.i(if output { Bcf(reg, bit) } else { Bsf(reg, bit) });
        self.asm.i(Bcf(STATUS, BIT_RP0));
        Ok(())
    }

    fn pin_set(&mut self, pin: usize, value: &Operand) -> Result<(), CodegenError> {
        let (reg, bit) = port_reg(&self.m.pins[pin])?;
        match self.bytes(value)?[0] {
            B::Imm(k) => self.asm.i(if k != 0 { Bsf(reg, bit) } else { Bcf(reg, bit) }),
            B::Reg(r) => {
                // sin parpadeos intermedios: se decide antes de tocar el pin
                let (clear, end) = (self.asm.label("pin.clr"), self.asm.label("pin.fin"));
                self.asm.i(Movf(r, W)); // Z = 1 si el valor es 0
                self.asm.i(Btfsc(STATUS, BIT_Z));
                self.asm.goto(clear);
                self.asm.i(Bsf(reg, bit));
                self.asm.goto(end);
                self.asm.place(clear);
                self.asm.i(Bcf(reg, bit));
                self.asm.place(end);
            }
        }
        Ok(())
    }

    fn inst(&mut self, inst: &pypic_ir::ir::Inst) -> Result<(), CodegenError> {
        use pypic_ir::ir::Inst as I;
        match inst {
            I::Copy { dst, src } => self.copy(*dst, src),
            I::Unary { dst, op, src } => self.unary(*dst, *op, src),
            I::Binary { dst, op, lhs, rhs } => self.binary(*dst, *op, lhs, rhs),
            I::Compare { dst, op, lhs, rhs } => self.compare(*dst, *op, lhs, rhs),
            I::Cast { dst, src } => self.cast(*dst, src),
            I::Call { dst, func, args } => self.call_inst(*dst, *func, args),
            I::PinCfg { pin, output } => self.pin_cfg(*pin, *output),
            I::PinSet { pin, value } => self.pin_set(*pin, value),
            I::PinToggle { pin } => {
                let (reg, bit) = port_reg(&self.m.pins[*pin])?;
                self.asm.i(Movlw(1 << bit));
                self.asm.i(Xorwf(reg, F));
                Ok(())
            }
            I::PinGet { dst, pin } => {
                let (reg, bit) = port_reg(&self.m.pins[*pin])?;
                let d = self.regs(*dst)?[0];
                self.asm.i(Clrf(d));
                self.asm.i(Btfsc(reg, bit));
                self.asm.i(Incf(d, F));
                Ok(())
            }
        }
    }

    fn epilogue_return(&mut self, value: &Option<Operand>) -> Result<(), CodegenError> {
        if let Some(op) = value {
            let b = self.bytes(op)?;
            if b.len() == 2 {
                self.ld(b[1]);
                let r = self.retv();
                self.st(r);
            }
            self.ld(b[0]); // el byte bajo sale en W
        }
        if self.f().is_interrupt {
            // restaura PCLATH, STATUS (con los bits de banco) y W sin alterar STATUS (ejemplo 14-1, p.219)
            self.asm.i(Movf(PCLATH_TEMP, W));
            self.asm.i(Movwf(PCLATH));
            self.asm.i(Swapf(STATUS_TEMP, W));
            self.asm.i(Movwf(STATUS));
            self.asm.i(Swapf(W_TEMP, F));
            self.asm.i(Swapf(W_TEMP, W));
            self.asm.i(Retfie);
        } else {
            self.asm.i(Return);
        }
        Ok(())
    }

    fn function(&mut self, fi: usize, bb_labels: &[Label]) -> Result<(), CodegenError> {
        self.cur = fi;
        let f = self.f();
        let entry = self.fn_labels[&fi];
        self.asm.place(entry);
        if f.is_interrupt {
            // prologo (p.219): W y STATUS a la RAM comun, PCLATH, y banco 0 / IRP = 0
            self.asm.i(Movwf(W_TEMP));
            self.asm.i(Swapf(STATUS, W));
            self.asm.i(Movwf(STATUS_TEMP));
            self.asm.i(Movf(PCLATH, W));
            self.asm.i(Movwf(PCLATH_TEMP));
            self.asm.i(Clrf(PCLATH));
            self.asm.i(Bcf(STATUS, BIT_RP0));
            self.asm.i(Bcf(STATUS, BIT_RP1));
            self.asm.i(Bcf(STATUS, BIT_IRP));
        }
        let live = reachable_blocks(f);
        let order: Vec<usize> = (0..f.blocks.len()).filter(|&b| live[b]).collect();
        for (pos, &b) in order.iter().enumerate() {
            let next = order.get(pos + 1).copied();
            self.asm.place(bb_labels[b]);
            let block = &f.blocks[b];
            for inst in &block.insts {
                self.inst(inst).map_err(|e| match e {
                    CodegenError::Unsupported(s) => CodegenError::Unsupported(format!("{s} (en la funcion '{}')", f.name)),
                    other => other,
                })?;
            }
            match &block.term {
                Terminator::Jump(t) => {
                    if Some(*t) != next {
                        self.asm.goto(bb_labels[*t]);
                    }
                }
                Terminator::Branch { cond, then_bb, else_bb } => {
                    match *cond {
                        Operand::Const { value, .. } => {
                            let t = if value != 0 { then_bb } else { else_bb };
                            if Some(*t) != next {
                                self.asm.goto(bb_labels[*t]);
                            }
                        }
                        Operand::Place(_) => {
                            let c = self.bytes(cond)?;
                            self.ld(c[0]); // MOVF afecta a Z: Z=1 si la condicion es falsa
                            if Some(*then_bb) == next {
                                self.asm.i(Btfsc(STATUS, BIT_Z));
                                self.asm.goto(bb_labels[*else_bb]);
                            } else if Some(*else_bb) == next {
                                self.asm.i(Btfss(STATUS, BIT_Z));
                                self.asm.goto(bb_labels[*then_bb]);
                            } else {
                                self.asm.i(Btfsc(STATUS, BIT_Z));
                                self.asm.goto(bb_labels[*else_bb]);
                                self.asm.goto(bb_labels[*then_bb]);
                            }
                        }
                    }
                }
                Terminator::Return(v) => self.epilogue_return(v)?,
                Terminator::Unreachable => self.asm.i(Sleep),
            }
        }
        Ok(())
    }
}

/// Compila un modulo de IR a una imagen de memoria de programa lista para escribir como HEX.
pub fn compile(m: &Module, opts: &Options) -> Result<Compiled, CodegenError> {
    let main = m.functions.iter().position(|f| f.name == "main").ok_or(CodegenError::NoMain)?;
    let isrs: Vec<usize> = (0..m.functions.len()).filter(|&i| m.functions[i].is_interrupt).collect();
    let mut roots = vec![main];
    roots.extend(&isrs);
    let live_set = reach(m, &roots);
    let mut live: Vec<usize> = live_set.iter().copied().collect();
    live.sort_unstable();
    let isr_ctx = reach(m, &isrs);

    let al = allocate(m, &live)?;
    let mut g = Gen { m, al, asm: Asm::default(), fn_labels: HashMap::new(), isr_ctx, cur: main };
    for &fi in &live {
        let l = g.asm.label(m.functions[fi].name.clone());
        g.fn_labels.insert(fi, l);
    }
    let startup = g.asm.label("startup");
    let halt = g.asm.label("halt");

    // vectores
    g.asm.items.push(Item::Org(0));
    g.asm.goto(startup);
    g.asm.items.push(Item::Org(4));
    match isrs.as_slice() {
        [] => g.asm.i(Retfie),
        [isr] => {
            let l = g.fn_labels[isr];
            g.asm.goto(l);
        }
        _ => return unsupported("mas de una funcion @interrupt"),
    }
    // arranque: inicializa todos los globales (la RAM no esta definida tras el reset), llama a main y duerme
    g.asm.items.push(Item::Org(5));
    g.asm.place(startup);
    for (i, gl) in m.globals.iter().enumerate() {
        let n = size(gl.ty)?;
        let addr = g.al.globals[i];
        let v = gl.init.rem_euclid(1i128 << (8 * n.max(1))) as u64;
        for k in 0..n {
            let byte = (v >> (8 * k)) as u8;
            if byte == 0 {
                g.asm.i(Clrf(addr + k as u8));
            } else {
                g.asm.i(Movlw(byte));
                g.asm.i(Movwf(addr + k as u8));
            }
        }
    }
    let mut ports: Vec<u8> = Vec::new();
    for p in &m.pins {
        let (reg, _) = port_reg(p)?;
        if !ports.contains(&reg) {
            ports.push(reg);
        }
    }
    for reg in ports {
        g.asm.i(Clrf(reg)); // los latches de PORTx quedan indefinidos tras el reset
    }
    let main_label = g.fn_labels[&main];
    g.asm.call(main_label);
    g.asm.place(halt);
    g.asm.i(Sleep);
    g.asm.goto(halt);

    for &fi in &live {
        let f = &m.functions[fi];
        let bb_labels: Vec<Label> = (0..f.blocks.len()).map(|b| g.asm.label(format!("{}.bb{b}", f.name))).collect();
        g.function(fi, &bb_labels)?;
    }

    // ---- ensamblado: direcciones y etiquetas ----
    let mut addr_of = vec![None::<u32>; g.asm.names.len()];
    let mut pc = 0u32;
    let mut placed: Vec<(u32, &Item)> = Vec::new();
    for it in &g.asm.items {
        match it {
            Item::Org(a) => pc = *a,
            Item::Label(l) => addr_of[*l] = Some(pc),
            other => {
                placed.push((pc, other));
                pc += 1;
            }
        }
    }
    let flash_words = placed.len();
    let top = placed.iter().map(|(a, _)| *a as usize + 1).max().unwrap_or(0);
    if top > PAGE_WORDS {
        return Err(CodegenError::TooLarge { words: top });
    }
    let mut image = Image::new();
    for (a, it) in &placed {
        let inst = match it {
            Item::Inst(i) => *i,
            Item::Goto(l) => Goto(addr_of[*l].expect("etiqueta sin colocar") as u16),
            Item::Call(l) => Call(addr_of[*l].expect("etiqueta sin colocar") as u16),
            _ => unreachable!(),
        };
        image.set(*a, inst.encode().expect("operandos validos")).expect("sin solapes");
    }
    image.set(CONFIG1, opts.config1).expect("config");
    image.set(CONFIG2, opts.config2).expect("config");

    // ---- listado ----
    let mut by_addr: BTreeMap<u32, Vec<&str>> = BTreeMap::new();
    for (l, a) in addr_of.iter().enumerate() {
        if let Some(a) = a {
            by_addr.entry(*a).or_default().push(&g.asm.names[l]);
        }
    }
    let words: Vec<(u32, u16)> = image.iter().filter(|(a, _)| *a < 0x2000).collect();
    let mut listing = String::new();
    for (a, w) in &words {
        for name in by_addr.get(a).into_iter().flatten() {
            listing.push_str(&format!("{name}:\n"));
        }
        listing.push_str(&disassemble(&[*w], *a));
    }

    let globals = m
        .globals
        .iter()
        .enumerate()
        .map(|(i, gl)| GlobalSym { name: gl.name.clone(), addr: g.al.globals[i], ty: gl.ty })
        .collect();
    let functions = live.iter().map(|&fi| (m.functions[fi].name.clone(), addr_of[g.fn_labels[&fi]].unwrap())).collect();
    Ok(Compiled { image, globals, functions, listing, flash_words, ram_bytes: g.al.used })
}
