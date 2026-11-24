//! Generador de codigo PIC14: IR tipada -> instrucciones nativas (ABI en docs/abi-pic14.md).
//!
//! Primer corte: enteros de 8 y 16 bits y bool. Todos los datos en el banco 0 (20h-6Fh), asi se
//! cumple la invariante de banco 0 sin cambiar de banco. Sin multiplicacion/division ni
//! desplazamientos por variable (necesitan rutinas de runtime) ni u32/i32. Programas de hasta
//! 2048 palabras (no se maneja PCLATH). Todo corre en el host; el MCU ejecuta solo instrucciones nativas.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use pypic_ir::ir::{BinOp, CmpOp, DelayUnit, Function, Module, Operand, Pin, Place, Terminator, TimerCall, TimerId, TimerOp, TimerStart, Type, UnaryOp};
use pypic_frontend::timer::{GateSource, TimerSource};

use crate::asm::{assemble_fragment, AsmError, FragItem};
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
    /// El dispositivo no es compatible con este backend o con el programa.
    Device(String),
    /// Errores del ensamblador de una funcion `@asm_function` (con la linea del fuente).
    Asm(Vec<AsmError>),
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
            CodegenError::Device(s) => f.write_str(s),
            CodegenError::Asm(errs) => {
                let lines: Vec<String> = errs.iter().map(|e| format!("linea {}:{}: error[{}]: {}", e.line, e.col, e.code, e.message)).collect();
                write!(f, "ensamblador en linea:\n  {}", lines.join("\n  "))
            }
            CodegenError::TooLarge { words } => write!(f, "el programa ocupa {words} palabras; el limite actual es una pagina de {PAGE_WORDS} (PCLATH no se maneja todavia)"),
        }
    }
}

impl std::error::Error for CodegenError {}

fn unsupported<T>(msg: impl Into<String>) -> Result<T, CodegenError> {
    Err(CodegenError::Unsupported(msg.into()))
}

/// Parametros del dispositivo y de la salida (se rellenan desde el Device Pack).
#[derive(Clone, Debug)]
pub struct Options {
    /// Nombre del dispositivo (`PIC16F887`): el backend solo genera codigo para los que conoce.
    pub device: String,
    /// Palabras de la memoria de programa.
    pub program_words: u32,
    /// Rango de RAM del banco 0 que usa el compilador (sin la zona comun 70h-7Fh).
    pub ram_start: u16,
    pub ram_end: u16,
    /// Direcciones de palabra de CONFIG1 y CONFIG2.
    pub config_addr: (u32, u32),
    pub config: (u16, u16),
    /// Relojes del oscilador por ciclo de instruccion (4 en PIC16): convierte tiempo en ciclos.
    pub clocks_per_cycle: u32,
    /// Mapa de registros del dispositivo (`registers.json` del Device Pack): el ensamblador en linea y los
    /// temporizadores los usan por nombre.
    pub registers: pypic_device::Registers,
}

/// Registros del PIC16F887 (`devices/pic16f887/registers.json`, incluido en el binario) para los usos sin pack.
fn builtin_registers() -> pypic_device::Registers {
    pypic_device::Registers::parse(include_str!("../../../devices/pic16f887/registers.json")).expect("registers.json incluido es valido")
}

impl Default for Options {
    /// PIC16F887 con oscilador interno INTOSCIO, WDT apagado y MCLR interno (los valores de `blink_rd0`).
    /// TODO: tomar las palabras de configuracion por nombre del Device Pack (#33).
    fn default() -> Self {
        Options {
            device: "PIC16F887".into(),
            program_words: 8192,
            ram_start: 0x20,
            ram_end: 0x6F,
            config_addr: (CONFIG1, CONFIG2),
            config: (BLINK_CONFIG1, BLINK_CONFIG2),
            clocks_per_cycle: 4,
            registers: builtin_registers(),
        }
    }
}

/// Dispositivos para los que el backend conoce el mapa de registros (PORTx, TRISx, ANSEL...).
const SUPPORTED_DEVICES: &[&str] = &["PIC16F887"];

#[derive(Debug, Clone)]
pub struct GlobalSym {
    pub name: String,
    pub addr: u8,
    pub ty: Type,
    /// Constante `Final`: se sustituye por su valor al compilar y no ocupa RAM (`addr` no es valido).
    pub is_const: bool,
}

/// Una espera generada: lo pedido, los ciclos de instruccion emitidos y el tiempo real que dan.
#[derive(Debug, Clone)]
pub struct DelayReport {
    pub function: String,
    pub unit: DelayUnit,
    /// Cantidad pedida; `None` si es una variable.
    pub requested: Option<u64>,
    /// Constante: ciclos totales de la espera. Variable: ciclos por unidad.
    pub cycles: u64,
    /// Variable: ciclos fijos de entrada que se suman a `n` unidades (0 en constantes).
    pub fixed_cycles: u64,
    /// Duracion de un ciclo de instruccion en ns.
    pub cycle_ns: f64,
}

impl DelayReport {
    pub fn unit_ns(&self) -> f64 {
        match self.unit {
            DelayUnit::Cycles => self.cycle_ns,
            DelayUnit::Us => 1e3,
            DelayUnit::Ms => 1e6,
            DelayUnit::S => 1e9,
        }
    }
    pub fn requested_ns(&self) -> Option<f64> {
        self.requested.map(|n| n as f64 * self.unit_ns())
    }
    /// Constante: duracion total real. Variable: duracion real de una unidad.
    pub fn achieved_ns(&self) -> f64 {
        self.cycles as f64 * self.cycle_ns
    }
}

#[derive(Debug)]
pub struct Compiled {
    pub timing: Vec<DelayReport>,
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
    /// 5 bytes de contadores para las esperas (solo si el programa las usa; solo contexto principal).
    delay_ws: u8,
    globals: Vec<u8>,
    locals: HashMap<usize, Vec<u8>>,
    temps: HashMap<usize, Vec<u8>>,
    used: usize,
}

fn allocate(m: &Module, live: &[usize], opts: &Options) -> Result<Alloc, CodegenError> {
    let mut next = opts.ram_start as usize;
    let mut take = |n: usize| -> u8 {
        let a = next;
        next += n;
        a as u8
    };
    let mut globals = Vec::new();
    for g in &m.globals {
        if g.is_const {
            globals.push(0); // las constantes se sustituyen por su valor al compilar: no ocupan RAM
            continue;
        }
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
    let uses_delay = live.iter().any(|&fi| m.functions[fi].blocks.iter().any(|b| b.insts.iter().any(|i| matches!(i, pypic_ir::ir::Inst::Delay { .. }))));
    let delay_ws = if uses_delay { take(5) } else { 0 };
    let used = next - opts.ram_start as usize;
    let available = (opts.ram_end - opts.ram_start + 1) as usize;
    if used > available {
        return Err(CodegenError::OutOfRam { needed: used, available });
    }
    Ok(Alloc { delay_ws, globals, locals, temps, used })
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
    clock_hz: Option<u32>,
    cpc: u32,
    timing: Vec<DelayReport>,
    registers: pypic_device::Registers,
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

    // ---- esperas: tiempo -> ciclos de instruccion exactos ------------------------------------------
    fn cycles_for(&self, unit: DelayUnit, n: u128) -> Result<u128, CodegenError> {
        let Some(hz) = self.clock_hz else {
            return Err(CodegenError::Device("falta el reloj (CLOCK_HZ) para generar las esperas".into()));
        };
        let (hz, cpc) = (hz as u128, self.cpc as u128);
        Ok(match unit {
            DelayUnit::Cycles => n,
            DelayUnit::Us => (n * hz + cpc * 500_000) / (cpc * 1_000_000),
            DelayUnit::Ms => (n * hz + cpc * 500) / (cpc * 1_000),
            DelayUnit::S => (n * hz + cpc / 2) / cpc,
        })
    }

    /// `p` ciclos sin efecto: `GOTO` a la instruccion siguiente (2 ciclos) y un `NOP` si sobra uno.
    fn pad(&mut self, mut p: u64) {
        while p >= 2 {
            let l = self.asm.label("relleno");
            self.asm.goto(l);
            self.asm.place(l);
            p -= 2;
        }
        if p == 1 {
            self.asm.i(Nop);
        }
    }

    /// Cuenta atras con 1 a 3 contadores (valores efectivos 1..=256; el registro 0 vale 256) en `ws`.
    /// Con una sola etiqueta de retorno: cada `DECFSZ` que no llega a 0 salta de vuelta al primero.
    fn count_loops(&mut self, ws: u8, v: &[u64]) {
        for (i, &k) in v.iter().enumerate() {
            self.asm.i(Movlw((k & 0xFF) as u8));
            self.asm.i(Movwf(ws + i as u8));
        }
        let l = self.asm.label("espera");
        self.asm.place(l);
        for i in 0..v.len() {
            self.asm.i(Decfsz(ws + i as u8, F));
            self.asm.goto(l);
        }
    }

    /// Emite codigo que dura **exactamente** `n` ciclos. Formulas (derivadas del recuento de ciclos de
    /// `DECFSZ`/`GOTO` y comprobadas contra el simulador): un contador `3a+1`, dos `4+3a+770b-769`,
    /// tres `6+3a+770b+197122c-197889`; lo que sobra (hasta 6 ciclos) se completa con relleno.
    fn exact_cycles(&mut self, mut n: u64, ws: u8) {
        const BLOCK: u64 = 50_000_000;
        while n > BLOCK {
            self.exact_block(BLOCK, ws);
            n -= BLOCK;
        }
        self.exact_block(n, ws);
    }

    fn exact_block(&mut self, n: u64, ws: u8) {
        if n <= 16 {
            return self.pad(n);
        }
        if n <= 3 * 256 + 1 + 6 {
            let a = ((n - 1) / 3).clamp(1, 256);
            self.count_loops(ws, &[a]);
            return self.pad(n - (3 * a + 1));
        }
        if n <= 4 + (3 * 256 + 770 * 256 - 769) + 6 {
            let r = n + 769 - 4; // = 3a + 770b + relleno
            let b = ((r - 3) / 770).clamp(1, 256);
            let rem = r - 770 * b;
            let a = (rem / 3).clamp(1, 256);
            self.count_loops(ws, &[a, b]);
            return self.pad(rem - 3 * a);
        }
        let r = n + 197_889 - 6; // = 3a + 770b + 197122c + relleno
        let c = ((r - 773) / 197_122).clamp(1, 256);
        let r2 = r - 197_122 * c;
        let b = ((r2 - 3) / 770).clamp(1, 256);
        let rem = r2 - 770 * b;
        let a = (rem / 3).clamp(1, 256);
        self.count_loops(ws, &[a, b, c]);
        self.pad(rem - 3 * a);
    }

    fn delay(&mut self, unit: DelayUnit, amount: &Operand) -> Result<(), CodegenError> {
        if self.in_isr() {
            return unsupported("esperas dentro de la interrupcion (ocupan la CPU y comparten memoria con el contexto principal)");
        }
        let ws = self.al.delay_ws;
        let hz = self.clock_hz.ok_or_else(|| CodegenError::Device("falta el reloj (CLOCK_HZ) para generar las esperas".into()))?;
        let cycle_ns = self.cpc as f64 * 1e9 / hz as f64;
        let function = self.f().name.clone();
        match *amount {
            Operand::Const { value, .. } => {
                let cyc = u64::try_from(self.cycles_for(unit, value as u128)?).map_err(|_| CodegenError::Device("la espera pedida no cabe en 64 bits de ciclos".into()))?;
                self.timing.push(DelayReport { function, unit, requested: Some(value as u64), cycles: cyc, fixed_cycles: 0, cycle_ns });
                self.exact_cycles(cyc, ws);
            }
            Operand::Place(_) => {
                // cantidad variable (u16): n vueltas de exactamente 1 unidad + 8 ciclos fijos de entrada
                let per = u64::try_from(self.cycles_for(unit, 1)?).unwrap_or(u64::MAX);
                if per < 9 {
                    return unsupported(format!("esperas con cantidad variable a este reloj: una unidad dura {per} ciclos y el bucle necesita al menos 9; usa una constante"));
                }
                self.timing.push(DelayReport { function, unit, requested: None, cycles: per, fixed_cycles: 8, cycle_ns });
                let b = self.bytes(amount)?;
                let (lo, hi) = (ws + 3, ws + 4);
                self.ld(b[0]);
                self.st(lo);
                self.ld(b[1]);
                self.st(hi);
                let (done, top) = (self.asm.label("espera.fin"), self.asm.label("espera.vuelta"));
                self.asm.i(Movf(lo, W));
                self.asm.i(Iorwf(hi, W));
                self.asm.i(Btfsc(STATUS, BIT_Z));
                self.asm.goto(done); // cantidad 0: no espera
                self.asm.place(top);
                self.exact_cycles(per - 9, ws);
                self.asm.i(Movlw(1));
                self.asm.i(Subwf(lo, F));
                self.asm.i(Btfss(STATUS, BIT_C));
                self.asm.i(Decf(hi, F)); // 16 bits: el prestamo baja el byte alto
                self.asm.i(Movf(lo, W));
                self.asm.i(Iorwf(hi, W));
                self.asm.i(Btfss(STATUS, BIT_Z));
                self.asm.goto(top);
                self.asm.i(Nop); // iguala la ultima vuelta: el salto no tomado cuesta 1 ciclo menos
                self.asm.place(done);
            }
        }
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
            I::Delay { unit, amount } => self.delay(*unit, amount),
            I::Timer { call, dst, arg } => self.timer(call, *dst, arg.as_ref()),
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

    /// Cuerpo de una funcion `@asm_function`: se ensambla en su sitio con los parametros y los globales por nombre.
    fn asm_function(&mut self, fi: usize) -> Result<(), CodegenError> {
        let m = self.m;
        let f = &m.functions[fi];
        let a = f.asm.as_ref().expect("funcion de ensamblador");
        let syms = self.registers.asm_symbols();
        let mut ext: BTreeMap<String, i64> = syms.iter().map(|(n, v)| (n.clone(), *v as i64)).collect();
        ext.insert("RETV".into(), RETV_MAIN as i64);
        let reserved = |n: &str| (n == "RETV" || syms.iter().any(|(s, _)| s == n)) && a.text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).any(|w| w == n);
        let mut clashes = Vec::new();
        for (i, g) in m.globals.iter().enumerate() {
            if reserved(&g.name) {
                clashes.push(g.name.clone());
            }
            ext.insert(g.name.clone(), if g.is_const { g.init as i64 } else { self.al.globals[i] as i64 });
        }
        for (i, l) in f.locals.iter().enumerate() {
            if reserved(&l.name) {
                clashes.push(l.name.clone());
            }
            ext.insert(l.name.clone(), self.al.locals[&fi][i] as i64); // un parametro oculta a un global del mismo nombre
        }
        if !clashes.is_empty() {
            clashes.sort();
            clashes.dedup();
            return Err(CodegenError::Asm(vec![AsmError {
                line: a.first_line,
                col: 1,
                code: "ASM005",
                message: format!("en '{}': {} coincide(n) con nombres que el ensamblador en linea predefine (los registros y bits del pack, y RETV) y que el bloque usa: renombra el parametro o el global", f.name, clashes.join(", ")),
            }]));
        }
        let frag = assemble_fragment(&a.text, a.first_line, &ext).map_err(CodegenError::Asm)?;
        let labels: BTreeMap<u32, Label> = frag.targets.iter().map(|&t| (t, self.asm.label(format!("{}.asm{t}", f.name)))).collect();
        for (i, item) in frag.items.iter().enumerate() {
            if let Some(&l) = labels.get(&(i as u32)) {
                self.asm.place(l);
            }
            match item {
                FragItem::Inst(x) => self.asm.i(*x),
                FragItem::Goto(t) => self.asm.goto(labels[t]),
                FragItem::Call(t) => self.asm.call(labels[t]),
            }
        }
        Ok(())
    }

    fn function(&mut self, fi: usize, bb_labels: &[Label]) -> Result<(), CodegenError> {
        self.cur = fi;
        let f = self.f();
        let entry = self.fn_labels[&fi];
        self.asm.place(entry);
        if f.asm.is_some() {
            return self.asm_function(fi);
        }
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
    if !SUPPORTED_DEVICES.iter().any(|d| d.eq_ignore_ascii_case(&opts.device)) {
        return Err(CodegenError::Device(format!(
            "el backend PIC14 aun no soporta '{}' (soportados: {}): falta su mapa de registros (#22)",
            opts.device,
            SUPPORTED_DEVICES.join(", ")
        )));
    }
    if let Some(d) = &m.device {
        if !d.eq_ignore_ascii_case(&opts.device) {
            return Err(CodegenError::Device(format!("el programa declara '{d}' pero se compila para '{}'", opts.device)));
        }
    }
    if opts.ram_start > opts.ram_end || opts.ram_end >= W_TEMP as u16 {
        return Err(CodegenError::Device(format!(
            "rango de RAM invalido {:#x}-{:#x}: debe terminar antes de la zona comun que usa la ABI ({:#x})",
            opts.ram_start, opts.ram_end, W_TEMP
        )));
    }
    let main = m.functions.iter().position(|f| f.name == "main").ok_or(CodegenError::NoMain)?;
    let isrs: Vec<usize> = (0..m.functions.len()).filter(|&i| m.functions[i].is_interrupt).collect();
    let inits: Vec<usize> = (0..m.functions.len()).filter(|&i| m.functions[i].is_init).collect();
    let mut roots = vec![main];
    roots.extend(&isrs);
    roots.extend(&inits);
    let live_set = reach(m, &roots);
    let mut live: Vec<usize> = live_set.iter().copied().collect();
    live.sort_unstable();
    let isr_ctx = reach(m, &isrs);

    let al = allocate(m, &live, opts)?;
    let mut g = Gen { m, al, asm: Asm::default(), fn_labels: HashMap::new(), isr_ctx, cur: main, clock_hz: m.clock_hz, cpc: opts.clocks_per_cycle, timing: Vec::new(), registers: opts.registers.clone() };
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
        if gl.is_const {
            continue;
        }
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
    for &i in &inits {
        let l = g.fn_labels[&i];
        g.asm.call(l); // la funcion @init_device va antes de main
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
    if top > PAGE_WORDS.min(opts.program_words as usize) {
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
    image.set(opts.config_addr.0, opts.config.0).expect("config");
    image.set(opts.config_addr.1, opts.config.1).expect("config");

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
        .map(|(i, gl)| GlobalSym { name: gl.name.clone(), addr: g.al.globals[i], ty: gl.ty, is_const: gl.is_const })
        .collect();
    let functions = live.iter().map(|&fi| (m.functions[fi].name.clone(), addr_of[g.fn_labels[&fi]].unwrap())).collect();
    Ok(Compiled { timing: g.timing, image, globals, functions, listing, flash_words, ram_bytes: g.al.used })
}

// ---- temporizadores (pypic.periph.timer) ---------------------------------------------------------------
// Los registros y bits salen del Device Pack (registers.json) por nombre; ver docs/sdk-timer.md para el
// equivalente en ensamblador de cada operacion.
impl<'a> Gen<'a> {
    fn reg_addr(&self, name: &str) -> Result<u16, CodegenError> {
        self.registers.get(name).map(|r| r.addresses[0].0 as u16).ok_or_else(|| CodegenError::Device(format!("el Device Pack no define el registro {name}")))
    }

    /// (direccion del registro, numero de bit) del bit con nombre `name`.
    fn bit_loc(&self, name: &str) -> Result<(u16, u8), CodegenError> {
        for r in &self.registers.registers {
            if let Some(i) = r.bits.iter().position(|b| b.as_deref() == Some(name)) {
                return Ok((r.addresses[0].0 as u16, i as u8));
            }
        }
        Err(CodegenError::Device(format!("el Device Pack no define el bit {name}")))
    }

    fn bit_pos(&self, name: &str) -> Result<u8, CodegenError> {
        Ok(self.bit_loc(name)?.1)
    }

    /// Direccion de 7 bits de un registro del banco 0 (donde viven los datos del compilador).
    fn reg_bank0(&self, name: &str) -> Result<u8, CodegenError> {
        let a = self.reg_addr(name)?;
        if a > 0x7F {
            return Err(CodegenError::Unsupported(format!("{name} no esta en el banco 0")));
        }
        Ok(a as u8)
    }

    /// Selecciona el banco de `addr`, ejecuta `body` con su desplazamiento de 7 bits y vuelve al banco 0 (ABI).
    fn banked(&mut self, addr: u16, body: impl FnOnce(&mut Self, u8)) {
        let bank = (addr >> 7) & 3;
        if bank & 1 != 0 {
            self.asm.i(Bsf(STATUS, BIT_RP0));
        }
        if bank & 2 != 0 {
            self.asm.i(Bsf(STATUS, BIT_RP1));
        }
        body(self, (addr & 0x7F) as u8);
        if bank & 1 != 0 {
            self.asm.i(Bcf(STATUS, BIT_RP0));
        }
        if bank & 2 != 0 {
            self.asm.i(Bcf(STATUS, BIT_RP1));
        }
    }

    fn bit_op(&mut self, name: &str, set: bool) -> Result<(), CodegenError> {
        let (addr, b) = self.bit_loc(name)?;
        self.banked(addr, |g, f| g.asm.i(if set { Bsf(f, b) } else { Bcf(f, b) }));
        Ok(())
    }

    fn timer_flag(t: TimerId) -> &'static str {
        match t {
            TimerId::T0 => "T0IF",
            TimerId::T1 => "TMR1IF",
            TimerId::T2 => "TMR2IF",
        }
    }

    fn timer_enable(t: TimerId) -> &'static str {
        match t {
            TimerId::T0 => "T0IE",
            TimerId::T1 => "TMR1IE",
            TimerId::T2 => "TMR2IE",
        }
    }

    fn timer(&mut self, call: &TimerCall, dst: Option<Place>, arg: Option<&Operand>) -> Result<(), CodegenError> {
        let t = call.timer;
        match call.op {
            TimerOp::Start(st) => match t {
                TimerId::T0 => self.t0_start(&st),
                TimerId::T1 => self.t1_start(&st),
                TimerId::T2 => self.t2_start(&st),
            },
            TimerOp::Stop => match t {
                TimerId::T1 => self.bit_op("TMR1ON", false),
                TimerId::T2 => self.bit_op("TMR2ON", false),
                TimerId::T0 => unsupported("TMR0.stop no existe en el hardware"),
            },
            TimerOp::Value => {
                let d = self.regs(dst.expect("Value lleva destino"))?;
                match t {
                    TimerId::T0 | TimerId::T2 => {
                        let r = self.reg_bank0(if t == TimerId::T0 { "TMR0" } else { "TMR2" })?;
                        self.asm.i(Movf(r, W));
                        self.st(d[0]);
                    }
                    TimerId::T1 => {
                        // lectura de 16 bits coherente: H, L, H; si H cambio entre las dos lecturas se repite (DS40001291H 6.5.1)
                        let (h, l) = (self.reg_bank0("TMR1H")?, self.reg_bank0("TMR1L")?);
                        let retry = self.asm.label("tmr1.read");
                        self.asm.place(retry);
                        self.asm.i(Movf(h, W));
                        self.st(d[1]);
                        self.asm.i(Movf(l, W));
                        self.st(d[0]);
                        self.asm.i(Movf(h, W));
                        self.asm.i(Subwf(d[1], W));
                        self.asm.i(Btfss(STATUS, BIT_Z));
                        self.asm.goto(retry);
                    }
                }
                Ok(())
            }
            TimerOp::Set => {
                let b = self.bytes(arg.expect("Set lleva valor"))?;
                match t {
                    TimerId::T0 | TimerId::T2 => {
                        let r = self.reg_bank0(if t == TimerId::T0 { "TMR0" } else { "TMR2" })?;
                        self.ld(b[0]);
                        self.st(r);
                    }
                    TimerId::T1 => {
                        // se pone a cero el byte bajo antes de escribir el alto para que un acarreo no ensucie la escritura
                        let (h, l) = (self.reg_bank0("TMR1H")?, self.reg_bank0("TMR1L")?);
                        self.asm.i(Clrf(l));
                        self.ld(b[1]);
                        self.st(h);
                        self.ld(b[0]);
                        self.st(l);
                    }
                }
                Ok(())
            }
            TimerOp::Expired => {
                let (addr, bit) = self.bit_loc(Self::timer_flag(t))?;
                if addr > 0x7F {
                    return unsupported("la bandera del temporizador no esta en el banco 0");
                }
                let d = self.regs(dst.expect("Expired lleva destino"))?[0];
                self.asm.i(Clrf(d));
                self.asm.i(Btfsc(addr as u8, bit));
                self.asm.i(Incf(d, F));
                Ok(())
            }
            TimerOp::Clear => self.bit_op(Self::timer_flag(t), false),
            TimerOp::InterruptOn => {
                self.bit_op(Self::timer_enable(t), true)?;
                if t != TimerId::T0 {
                    self.bit_op("PEIE", true)?; // las interrupciones de perifericos pasan por PEIE
                }
                Ok(())
            }
            TimerOp::InterruptOff => self.bit_op(Self::timer_enable(t), false),
        }
    }

    /// Timer0: bits de OPTION_REG uno a uno (no se pisa RBPU ni INTEDG) y la secuencia del datasheet para
    /// cambiar la asignacion del preescalador (ejemplos 5-1 y 5-2, p.76).
    fn t0_start(&mut self, st: &TimerStart) -> Result<(), CodegenError> {
        let opt = self.reg_addr("OPTION_REG")?;
        let (t0cs, t0se, psa) = (self.bit_pos("T0CS")?, self.bit_pos("T0SE")?, self.bit_pos("PSA")?);
        let external = st.source == TimerSource::T0cki;
        let edge_falling = external && st.falling;
        if st.prescaler >= 2 {
            let ps = (st.prescaler.trailing_zeros() - 1) as u8; // 2 -> 000 ... 256 -> 111
            self.asm.i(Clrwdt);
            self.banked(opt, |g, f| {
                g.asm.i(if external { Bsf(f, t0cs) } else { Bcf(f, t0cs) });
                g.asm.i(if edge_falling { Bsf(f, t0se) } else { Bcf(f, t0se) });
                g.asm.i(Movlw(0xF0)); // conserva RBPU, INTEDG, T0CS y T0SE; pone PSA = 0 y PS<2:0>
                g.asm.i(Andwf(f, W));
                g.asm.i(Iorlw(ps));
                g.asm.i(Movwf(f));
            });
        } else {
            // 1:1: el preescalador pasa al WDT (PSA = 1)
            let tmr0 = self.reg_bank0("TMR0")?;
            self.asm.i(Clrwdt);
            self.asm.i(Clrf(tmr0)); // pone a cero TMR0 y el preescalador
            self.banked(opt, |g, f| {
                g.asm.i(Bsf(f, psa));
                g.asm.i(Clrwdt);
                g.asm.i(if external { Bsf(f, t0cs) } else { Bcf(f, t0cs) });
                g.asm.i(if edge_falling { Bsf(f, t0se) } else { Bcf(f, t0se) });
                g.asm.i(Movlw(0xF8));
                g.asm.i(Andwf(f, W));
                g.asm.i(Iorlw(0x05)); // el valor del ejemplo 5-1: el preescalador del WDT a 1:32
                g.asm.i(Movwf(f));
            });
            // Movwf(f) borra PSA con la mascara 0xF8 solo en los bits PS; PSA queda en 1 porque 0xF8 lo conserva
        }
        Ok(())
    }

    /// Timer1: todo T1CON se escribe de una vez (todos sus bits son de Timer1); la fuente de la compuerta
    /// (T1GSS) esta en CM2CON1.
    fn t1_start(&mut self, st: &TimerStart) -> Result<(), CodegenError> {
        let external = st.source != TimerSource::Internal;
        let mut v: u8 = 0;
        for (name, on) in [("TMR1ON", true), ("TMR1CS", external), ("T1SYNC", external && !st.sync), ("T1OSCEN", st.source == TimerSource::T1osc)] {
            if on {
                v |= 1 << self.bit_pos(name)?; // T1SYNC = 1: no sincronizar
            }
        }
        v |= (st.prescaler.trailing_zeros() as u8) << self.bit_pos("T1CKPS0")?; // 1,2,4,8 -> 0..3
        if let Some(g) = st.gate {
            v |= 1 << self.bit_pos("TMR1GE")?;
            if g.active_high {
                v |= 1 << self.bit_pos("T1GINV")?;
            }
            let gss = g.source == GateSource::T1g;
            self.bit_op("T1GSS", gss)?;
            if gss {
                self.bit_op("ANS13", false)?; // RB5/AN13/T1G arranca como analogico
            }
        }
        let t1con = self.reg_bank0("T1CON")?;
        self.asm.i(Movlw(v));
        self.asm.i(Movwf(t1con));
        Ok(())
    }

    /// Timer2: TMR2 a cero, PR2 y despues T2CON con TMR2ON; el periodo de la bandera es
    /// `prescaler * (PR2 + 1) * postscaler` ciclos de instruccion.
    fn t2_start(&mut self, st: &TimerStart) -> Result<(), CodegenError> {
        let (tmr2, t2con) = (self.reg_bank0("TMR2")?, self.reg_bank0("T2CON")?);
        let pr2 = self.reg_addr("PR2")?;
        let ckps: u8 = match st.prescaler {
            1 => 0,
            4 => 1,
            _ => 2,
        };
        let v = (((st.postscaler - 1) as u8) << self.bit_pos("TOUTPS0")?) | (1 << self.bit_pos("TMR2ON")?) | (ckps << self.bit_pos("T2CKPS0")?);
        self.asm.i(Clrf(tmr2)); // escribir T2CON no pone a cero TMR2 (p.83)
        self.asm.i(Movlw(st.period as u8));
        self.banked(pr2, |g, f| g.asm.i(Movwf(f)));
        self.asm.i(Movlw(v));
        self.asm.i(Movwf(t2con));
        Ok(())
    }
}
