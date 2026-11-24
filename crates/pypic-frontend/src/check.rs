//! Validacion semantica, analisis de tipos y evaluacion de constantes.
//!
//! Semantica fijada aqui (ver docs/frontend.md):
//!  * Los literales enteros toman el tipo del contexto si su valor cabe.
//!  * No hay conversiones implicitas entre tipos: se usa u8(x), i16(x), ...
//!  * Aritmetica en ejecucion: modular (wrap-around) en el ancho del tipo.
//!  * Una constante que no cabe en su tipo es un error; nunca se trunca en silencio.
//!  * `//` y `%` truncan hacia cero (semantica C, no la de Python).
//!  * Las condiciones deben ser bool (sin "truthiness").
//!  * Sin recursion: la memoria es estatica y la pila de hardware es limitada.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::ast::*;
use crate::diag::{Diagnostics, Span};
use crate::target::{PinInfo, TargetProfile};
use crate::types::{Type, INFERENCE_ORDER, U16, U32, U8};

/// Nombres predeclarados (no requieren import; `from pypic import ...` es
/// opcional para que el archivo siga siendo Python valido en otros editores).
const PREDECLARED: &[&str] = &["u8", "i8", "u16", "i16", "u32", "i32", "bool", "Final", "interrupt", "init_device"];

#[derive(Clone, Copy, Debug)]
pub struct ExprInfo {
    pub ty: Type,
    /// Valor si la expresion es constante en tiempo de compilacion.
    pub konst: Option<i128>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymKind {
    Global,
    Local,
    Param,
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: String,
    pub ty: Type,
    pub kind: SymKind,
    pub konst: Option<i128>,
    pub is_final: bool,
}

#[derive(Clone, Debug)]
pub struct FuncInfo {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub ret: Type,
    pub is_interrupt: bool,
    /// Decorada con `@init_device`: la ejecuta el arranque antes de `main`.
    pub is_init: bool,
    pub calls: BTreeSet<String>,
    pub span: Span,
}

/// Unidad de una espera (`pypic.system.time`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelayUnit {
    /// Ciclos de instruccion: la unidad minima del hardware.
    Cycles,
    Us,
    Ms,
    S,
}

/// Funciones de `pypic.system.time` implementadas (nombre -> unidad). Una sola forma por operacion, sin sinonimos:
/// `pause` = milisegundos (como PICAXE), `wait` = segundos, `delay_us` = microsegundos, `delay_cycles` = ciclos.
pub fn time_unit(name: &str) -> Option<DelayUnit> {
    match name {
        "delay_cycles" => Some(DelayUnit::Cycles),
        "delay_us" => Some(DelayUnit::Us),
        "pause" => Some(DelayUnit::Ms),
        "wait" => Some(DelayUnit::S),
        _ => None,
    }
}

/// Una llamada a una espera, para las comprobaciones de todo el programa.
struct TimeUse {
    func: usize,
    span: Span,
    name: String,
    unit: DelayUnit,
    /// Valor constante pedido; `None` si la cantidad es una variable.
    amount: Option<i128>,
}

/// Un pin importado con `from pypic.periph.gpio import RD0` (no ocupa memoria: se resuelve al compilar).
#[derive(Clone, Debug)]
pub struct PinDecl {
    /// Nombre del pin en el dispositivo y en el programa (`RD0`).
    pub pin: String,
    pub port: char,
    pub bit: u8,
    pub input_only: bool,
    /// Canal analogico (`ANn`): hay que pasarlo a digital para usarlo como E/S.
    pub analog: Option<u8>,
    pub span: Span,
}

/// Uso de un pin a lo largo del programa (comprobacion global, sin importar el orden).
#[derive(Default)]
struct PinUse {
    out_cfg: bool,
    in_cfg: bool,
    needs_out: Option<Span>,
    needs_cfg: Option<Span>,
}

/// Resultado del analisis: AST + tipos/constantes por expresion + simbolos.
#[derive(Debug)]
pub struct Program {
    pub module: Module,
    /// Indexado por `Expr::id`.
    pub info: Vec<ExprInfo>,
    pub globals: BTreeMap<String, Symbol>,
    pub functions: Vec<FuncInfo>,
    pub pins: Vec<PinDecl>,
    /// Dispositivo declarado con `from pypic.device import PIC16F887`.
    pub device: Option<String>,
    /// Reloj efectivo: el de `CLOCK_HZ: Final[u32]` o, si usa esperas y no lo declaro, el del dispositivo.
    pub clock_hz: Option<u32>,
    /// `clock_hz` es el reloj por defecto del dispositivo (no se declaro `CLOCK_HZ`).
    pub clock_defaulted: bool,
}

pub struct Checked {
    pub info: Vec<ExprInfo>,
    pub globals: BTreeMap<String, Symbol>,
    pub functions: Vec<FuncInfo>,
    pub pins: Vec<PinDecl>,
    pub device: Option<String>,
    pub clock_hz: Option<u32>,
    pub clock_defaulted: bool,
}

pub fn check(module: &Module, target: &TargetProfile, d: &mut Diagnostics) -> Checked {
    let mut c = Checker {
        d,
        target,
        info: vec![ExprInfo { ty: Type::Error, konst: None }; module.expr_count as usize],
        globals: BTreeMap::new(),
        functions: Vec::new(),
        fidx: HashMap::new(),
        cur: None,
        locals: HashMap::new(),
        global_decls: HashSet::new(),
        loop_depth: 0,
        warned: HashSet::new(),
        pins: Vec::new(),
        pin_names: HashMap::new(),
        pin_uses: Vec::new(),
        declared_device: None,
        clock_hz: None,
        clock_defaulted: false,
        in_const_decl: false,
        time_fns: HashMap::new(),
        time_uses: Vec::new(),
    };
    c.run(module);
    Checked { info: c.info, globals: c.globals, functions: c.functions, pins: c.pins, device: c.declared_device, clock_hz: c.clock_hz, clock_defaulted: c.clock_defaulted }
}

fn fold(op: BinOp, a: i128, b: i128) -> Option<i128> {
    match op {
        BinOp::Add => a.checked_add(b),
        BinOp::Sub => a.checked_sub(b),
        BinOp::Mul => a.checked_mul(b),
        BinOp::Div => a.checked_div(b), // trunca hacia cero
        BinOp::Mod => a.checked_rem(b), // signo del dividendo (como C)
        BinOp::BitAnd => Some(a & b),
        BinOp::BitOr => Some(a | b),
        BinOp::BitXor => Some(a ^ b),
        BinOp::Shl => {
            if !(0..100).contains(&b) {
                None
            } else {
                a.checked_mul(1i128 << b)
            }
        }
        BinOp::Shr => Some(a >> b.clamp(0, 127)),
    }
}

struct Checker<'a> {
    d: &'a mut Diagnostics,
    target: &'a TargetProfile,
    info: Vec<ExprInfo>,
    globals: BTreeMap<String, Symbol>,
    functions: Vec<FuncInfo>,
    fidx: HashMap<String, usize>,
    cur: Option<usize>,
    locals: HashMap<String, Symbol>,
    global_decls: HashSet<String>,
    loop_depth: u32,
    /// Tipos emulados ya avisados (un aviso por tipo).
    warned: HashSet<Type>,
    /// Pines importados de `pypic.periph.gpio`, en orden de importacion.
    pins: Vec<PinDecl>,
    pin_names: HashMap<String, usize>,
    pin_uses: Vec<PinUse>,
    declared_device: Option<String>,
    clock_hz: Option<u32>,
    /// El reloj no se declaro y se uso el del dispositivo por defecto.
    clock_defaulted: bool,
    /// Verdadero mientras se resuelve el tipo de una constante `Final`: no cuesta nada en el dispositivo.
    in_const_decl: bool,
    /// Funciones de `pypic.system.time` importadas.
    time_fns: HashMap<String, DelayUnit>,
    time_uses: Vec<TimeUse>,
}

impl<'a> Checker<'a> {
    fn run(&mut self, m: &Module) {
        for (i, it) in m.items.iter().enumerate() {
            if let Item::Import { span, module, names } = it {
                if module == "pypic.device" {
                    self.check_device_decl(i, *span, names);
                } else {
                    self.check_import(*span, module, names);
                }
            }
        }
        for it in &m.items {
            if let Item::Func(f) = it {
                self.declare_function(f);
            }
        }
        for it in &m.items {
            match it {
                Item::Var(v) => self.declare_variable(v, true),
                Item::Import { .. } | Item::Func(_) => {}
            }
        }
        for it in &m.items {
            if let Item::Func(f) = it {
                self.check_function(f);
            }
        }
        self.check_recursion();
        self.check_interrupt_sharing();
        self.check_stack_depth();
        self.check_pin_usage();
        self.check_time_usage();
    }

    /// `from pypic.device import PIC16F887`: declara el dispositivo destino; debe ser la primera linea.
    fn check_device_decl(&mut self, index: usize, span: Span, names: &[Ident]) {
        if self.declared_device.is_some() {
            self.d.error("PYP2091", span, "el dispositivo ya se declaro: un programa tiene un solo 'from pypic.device import ...'");
            return;
        }
        let [name] = names else {
            self.d.error("PYP2092", span, "se declara un solo dispositivo, p. ej. 'from pypic.device import PIC16F887'");
            return;
        };
        if index != 0 {
            self.d.error("PYP2092", span, "la declaracion del dispositivo debe ser la primera linea del programa (antes de cualquier otro import o declaracion)");
        }
        match &self.target.device {
            Some(d) if !d.eq_ignore_ascii_case(&name.name) => {
                self.d.error("PYP2090", name.span, format!("el programa declara '{}' pero se compila para '{d}'", name.name));
            }
            None if !looks_like_device(&name.name) => {
                self.d.error("PYP2096", name.span, format!("'{}' no parece un dispositivo PIC (se espera algo como PIC16F887)", name.name));
            }
            _ => {}
        }
        self.declared_device = Some(name.name.clone());
    }

    /// `CLOCK_HZ: Final[u32] = 4_000_000`: frecuencia del reloj del sistema.
    fn check_clock(&mut self, v: &VarDecl, ty: Type, is_final: bool, konst: Option<i128>) {
        if ty == Type::Error {
            return;
        }
        if !is_final || ty != U32 {
            self.d.error("PYP2093", v.name.span, "CLOCK_HZ debe declararse como 'CLOCK_HZ: Final[u32] = 4_000_000' (frecuencia del reloj del sistema en hercios)");
            return;
        }
        match konst {
            Some(0) | None => self.d.error("PYP2093", v.name.span, "CLOCK_HZ debe ser una constante mayor que 0"),
            Some(hz) => {
                self.clock_hz = Some(hz as u32);
                if let Some(max) = self.target.max_clock_hz {
                    if hz > max as i128 {
                        self.d.error("PYP2094", v.name.span, format!("CLOCK_HZ = {hz} Hz excede el reloj maximo de '{}' ({max} Hz)", self.target.name));
                    }
                }
            }
        }
    }

    fn check_import(&mut self, span: Span, module: &str, names: &[Ident]) {
        if let Some((code, msg)) = module_problem(module) {
            self.d.error(code, span, msg);
            return;
        }
        if module == "pypic.system.time" {
            for n in names {
                if let Some(u) = time_unit(&n.name) {
                    self.time_fns.insert(n.name.clone(), u);
                } else if n.name == "delay_ms" || n.name == "delay" {
                    self.d.error(
                        "PYP2104",
                        n.span,
                        format!("'{}' no existe: PyPIC tiene una sola forma por operacion. Usa 'pause' (milisegundos), 'wait' (segundos), 'delay_us' (microsegundos) o 'delay_cycles' (ciclos)", n.name),
                    );
                } else if n.name == "sleep" || n.name == "nap" {
                    self.d.error(
                        "PYP2103",
                        n.span,
                        format!("'{}' (espera de bajo consumo con el watchdog, aproximada) todavia no esta implementada; para una espera exacta usa pause o wait", n.name),
                    );
                } else {
                    self.d.error("PYP1002", n.span, format!("'pypic.system.time' no exporta '{}' (exporta: delay_cycles, delay_us, pause, wait; reservadas: nap, sleep)", n.name));
                }
            }
            return;
        }
        if module == "pypic.periph.gpio" {
            for n in names {
                match self.pin_info(&n.name) {
                    Some(info) => {
                        if !self.pin_names.contains_key(&n.name) {
                            self.pin_names.insert(n.name.clone(), self.pins.len());
                            self.pins.push(PinDecl { pin: n.name.clone(), port: info.port, bit: info.bit, input_only: info.input_only, analog: info.analog, span: n.span });
                            self.pin_uses.push(PinUse::default());
                        }
                    }
                    None => {
                        let msg = match &self.target.pins {
                            Some(map) => format!(
                                "'pypic.periph.gpio' no exporta '{}' en '{}' (pines de E/S: {})",
                                n.name,
                                self.target.name,
                                map.keys().map(String::as_str).collect::<Vec<_>>().join(", ")
                            ),
                            None => format!("'{}' no es un nombre de pin valido (se espera algo como RD0)", n.name),
                        };
                        self.d.error("PYP2074", n.span, msg);
                    }
                }
            }
            return;
        }
        let ok = module == "pypic" && names.iter().all(|n| PREDECLARED.contains(&n.name.as_str()));
        if !ok {
            self.d.error(
                "PYP1002",
                span,
                format!("import de '{module}' no soportado; modulos disponibles: 'pypic' (tipos), 'pypic.device', y por categoria 'pypic.periph.*' (hardware), 'pypic.system.*' (CPU, reloj, energia) y 'pypic.soft.*' (software)"),
            );
        }
    }

    // ------------------------------------------------------------------ pines
    fn time_call(&mut self, e: &Expr, func: &str, unit: DelayUnit, args: &[Expr]) -> Type {
        if args.len() != 1 {
            self.d.error("PYP2055", e.span, format!("'{func}' recibe exactamente 1 argumento"));
            for a in args {
                self.expr(a);
            }
            return self.set(e, Type::Void, None);
        }
        let a = &args[0];
        let t = self.expr(a);
        let konst = self.konst(a);
        match unit {
            DelayUnit::Cycles | DelayUnit::Us => match (t, konst) {
                (Type::Error, _) => {}
                (t, Some(v)) if matches!(t, Type::Literal | Type::Int { .. }) => {
                    if v < 0 || v > U32.max() {
                        self.d.error("PYP2040", a.span, format!("{func}({v}): el valor debe estar entre 0 y {}", U32.max()));
                    } else if t == Type::Literal {
                        self.coerce_literal(a, U32);
                    }
                }
                _ => self.d.error("PYP2100", a.span, format!("{func}() necesita una constante: el tiempo se calcula al compilar, al ciclo; para una cantidad calculada usa pause o wait con un u16")),
            },
            DelayUnit::Ms | DelayUnit::S => match t {
                Type::Error => {}
                Type::Literal => {
                    self.coerce_literal(a, U16);
                }
                t if t == U16 => {}
                t => self.d.error("PYP2041", a.span, format!("{func}() recibe un u16 o una constante, no {t}")),
            },
        }
        if let Some(cur) = self.cur {
            self.time_uses.push(TimeUse { func: cur, span: e.span, name: func.to_string(), unit, amount: konst });
        }
        self.set(e, Type::Void, None)
    }

    /// Reglas globales de las esperas: necesitan CLOCK_HZ, no van en la interrupcion y avisan si el
    /// tiempo pedido no cae en un numero entero de ciclos de instruccion.
    fn check_time_usage(&mut self) {
        let uses = std::mem::take(&mut self.time_uses);
        let Some(first) = uses.first() else { return };
        let hz = match self.clock_hz {
            Some(hz) => hz,
            None => match self.target.default_clock_hz {
                Some(def) => {
                    self.d.warning(
                        "PYP3004",
                        first.span,
                        format!("no se declaro CLOCK_HZ: se usa el reloj por defecto de {} ({def} Hz); declara 'CLOCK_HZ: Final[u32] = {def}' para fijarlo y quitar este aviso", self.target.name),
                    );
                    self.clock_hz = Some(def);
                    self.clock_defaulted = true;
                    def
                }
                None => {
                    self.d.error("PYP2101", first.span, format!("{} necesita el reloj del sistema y no hay dispositivo declarado con reloj por defecto: declara 'CLOCK_HZ: Final[u32] = 4_000_000'", first.name));
                    return;
                }
            },
        };
        if let (false, Some(def)) = (self.clock_defaulted, self.target.default_clock_hz) {
            if hz != def {
                self.d.warning(
                    "PYP3005",
                    first.span,
                    format!(
                        "CLOCK_HZ = {hz} Hz difiere del reloj que produce la configuracion por defecto de {} ({def} Hz, oscilador interno): el compilador todavia no configura el oscilador (OSCCON y palabras de configuracion, #33), asi que los tiempos solo seran correctos si el reloj real es {hz} Hz",
                        self.target.name
                    ),
                );
            }
        }
        // funciones alcanzables desde la interrupcion
        let mut reach: BTreeSet<String> = BTreeSet::new();
        let mut stack: Vec<String> = self.functions.iter().filter(|f| f.is_interrupt).map(|f| f.name.clone()).collect();
        while let Some(n) = stack.pop() {
            if let Some(&i) = self.fidx.get(&n) {
                if reach.insert(n) {
                    stack.extend(self.functions[i].calls.iter().cloned());
                }
            }
        }
        let cpc = self.target.clocks_per_cycle;
        let mut warned_var: HashSet<&'static str> = HashSet::new();
        for u in &uses {
            if reach.contains(&self.functions[u.func].name) {
                self.d.error("PYP2102", u.span, format!("{}() ocupa la CPU y no puede usarse en la interrupcion ni en las funciones que ella llama", u.name));
            }
            let Some(cpc) = cpc else { continue };
            let div: u128 = match u.unit {
                DelayUnit::Cycles => continue,
                DelayUnit::Us => 1_000_000,
                DelayUnit::Ms => 1_000,
                DelayUnit::S => 1,
            };
            let den = cpc as u128 * div;
            match u.amount {
                Some(n) if n >= 0 => {
                    let num = n as u128 * hz as u128;
                    if num % den != 0 {
                        let cycles = (num + den / 2) / den;
                        let ideal_ns = num as f64 * 1e9 / den as f64 / (hz as f64 / cpc as f64) * (hz as f64 / cpc as f64) / hz as f64 * cpc as f64;
                        let got_ns = cycles as f64 * cpc as f64 * 1e9 / hz as f64;
                        let _ = ideal_ns;
                        let want_ns = n as f64 * (1e9 / div as f64);
                        self.d.warning(
                            "PYP3003",
                            u.span,
                            format!(
                                "{}({n}) no cae en un numero entero de ciclos a {hz} Hz (un ciclo dura {:.1} ns): se redondea a {cycles} ciclos, error de {:+.1} ns",
                                u.name,
                                cpc as f64 * 1e9 / hz as f64,
                                got_ns - want_ns
                            ),
                        );
                    }
                }
                None if (hz as u128) % den != 0 && warned_var.insert(if u.unit == DelayUnit::Ms { "ms" } else { "s" }) => {
                    let exact = hz as f64 / den as f64;
                    let ppm = (exact.round() - exact).abs() / exact * 1e6;
                    self.d.warning(
                        "PYP3002",
                        u.span,
                        format!(
                            "a {hz} Hz una unidad de {} no es un numero entero de ciclos ({exact:.3}): con una cantidad variable el error es de {ppm:.0} ppm por unidad; con una constante la espera es exacta",
                            if u.unit == DelayUnit::Ms { "milisegundo" } else { "segundo" }
                        ),
                    );
                }
                _ => {}
            }
        }
    }

    fn pin_info(&self, name: &str) -> Option<PinInfo> {
        match &self.target.pins {
            Some(map) => map.get(name).cloned(),
            None => parse_pin_name(name).map(|(port, bit)| PinInfo { port, bit, input_only: false, analog: None }),
        }
    }

    fn method_call(&mut self, e: &Expr, obj: &str, obj_span: Span, method: &str, args: &[Expr]) -> Type {
        let Some(&pi) = self.pin_names.get(obj) else {
            let hint = if self.pin_info(obj).is_some() { format!("; importalo con 'from pypic.periph.gpio import {obj}'") } else { String::new() };
            self.d.error("PYP2078", obj_span, format!("'{obj}' no es un pin importado{hint}"));
            for a in args {
                self.expr(a);
            }
            return self.set(e, Type::Error, None);
        };
        let arities: &[usize] = match method {
            "output" | "input" | "on" | "off" | "toggle" => &[0],
            "value" => &[0, 1],
            other => {
                self.d.error("PYP2080", e.span, format!("un pin no tiene el metodo '{other}' (disponibles: output, input, on, off, toggle, value)"));
                for a in args {
                    self.expr(a);
                }
                return self.set(e, Type::Error, None);
            }
        };
        if !arities.contains(&args.len()) {
            self.d.error("PYP2055", e.span, format!("'{obj}.{method}' no acepta {} argumento(s)", args.len()));
            for a in args {
                self.expr(a);
            }
            return self.set(e, Type::Error, None);
        }
        match method {
            "output" => {
                if self.pins[pi].input_only {
                    self.d.error("PYP2075", e.span, format!("el pin '{obj}' es solo de entrada: usa {obj}.input()"));
                }
                self.pin_uses[pi].out_cfg = true;
                self.set(e, Type::Void, None)
            }
            "input" => {
                self.pin_uses[pi].in_cfg = true;
                self.set(e, Type::Void, None)
            }
            "on" | "off" | "toggle" => {
                self.pin_uses[pi].needs_out.get_or_insert(e.span);
                self.set(e, Type::Void, None)
            }
            _ => match args {
                [] => {
                    self.pin_uses[pi].needs_cfg.get_or_insert(e.span);
                    self.set(e, U8, None)
                }
                [a] => {
                    self.pin_uses[pi].needs_out.get_or_insert(e.span);
                    match self.expr(a) {
                        Type::Bool | Type::Error => {}
                        Type::Literal => {
                            self.coerce_literal(a, U8);
                        }
                        t if t == U8 => {}
                        t => self.d.error("PYP2041", a.span, format!("value() recibe un u8 o un bool, no {t}")),
                    }
                    self.set(e, Type::Void, None)
                }
                _ => unreachable!(),
            },
        }
    }

    /// Un pin debe configurarse en algun punto del programa antes de usarse (la RAM de los registros
    /// arranca como entrada y analogico).
    fn check_pin_usage(&mut self) {
        for (i, pin) in self.pins.iter().enumerate() {
            let u = &self.pin_uses[i];
            if let Some(sp) = u.needs_out {
                if !u.out_cfg {
                    let msg = if u.in_cfg {
                        format!("el pin '{0}' solo se configura como entrada ({0}.input()); on/off/toggle/value(x) necesitan {0}.output()", pin.pin)
                    } else {
                        format!("el pin '{0}' nunca se configura como salida: llama a {0}.output()", pin.pin)
                    };
                    self.d.error("PYP2079", sp, msg);
                    continue;
                }
            }
            if let Some(sp) = u.needs_cfg {
                if !u.out_cfg && !u.in_cfg {
                    self.d.error("PYP2081", sp, format!("el pin '{0}' nunca se configura: llama a {0}.input() (o {0}.output())", pin.pin));
                }
            }
        }
    }

    // ---------------------------------------------------------------- tipos
    fn resolve_type(&mut self, te: &TypeExpr, allow_void: bool) -> Type {
        match &te.kind {
            TypeExprKind::None if allow_void => Type::Void,
            TypeExprKind::Name(n) => match Type::from_name(n) {
                Some(t) if self.target.supports(t) => {
                    self.note_emulated(t, te.span);
                    t
                }
                Some(t) => {
                    self.d.error(
                        "PYP2030",
                        te.span,
                        format!("el tipo '{t}' no esta soportado por el destino '{}' (maximo {} bits)", self.target.name, self.target.max_int_bits),
                    );
                    Type::Error
                }
                None => {
                    self.d.error("PYP2001", te.span, format!("tipo '{n}' no valido; usa u8, i8, u16, i16, u32, i32 o bool"));
                    Type::Error
                }
            },
            _ => {
                self.d.error("PYP2001", te.span, "tipo no valido en este contexto");
                Type::Error
            }
        }
    }

    /// Aviso de coste (una vez por tipo) si el destino emula `t` por software.
    fn note_emulated(&mut self, t: Type, span: Span) {
        if self.in_const_decl {
            return; // una constante en tiempo de compilacion no cuesta nada en el dispositivo
        }
        if self.target.is_emulated(t) && self.warned.insert(t) {
            self.d.warning(
                "PYP3001",
                span,
                format!(
                    "'{t}' excede el ancho nativo de {} bits de '{}': se emulara por software (mayor coste en flash y ciclos)",
                    self.target.native_bits, self.target.name
                ),
            );
        }
    }

    fn set(&mut self, e: &Expr, ty: Type, konst: Option<i128>) -> Type {
        self.info[e.id as usize] = ExprInfo { ty, konst };
        ty
    }
    fn konst(&self, e: &Expr) -> Option<i128> {
        self.info[e.id as usize].konst
    }
    fn ty(&self, e: &Expr) -> Type {
        self.info[e.id as usize].ty
    }

    // -------------------------------------------------------- declaraciones
    fn declare_function(&mut self, f: &FuncDef) {
        if self.fidx.contains_key(&f.name.name) {
            self.d.error("PYP2002", f.name.span, format!("funcion '{}' ya definida", f.name.name));
            return;
        }
        if self.pin_names.contains_key(&f.name.name) || self.time_fns.contains_key(&f.name.name) {
            self.d.error("PYP2009", f.name.span, format!("'{}' ya esta declarado (es un pin o una funcion del SDK importados)", f.name.name));
            return;
        }
        let mut params = Vec::new();
        let mut seen = HashSet::new();
        for p in &f.params {
            if !seen.insert(p.name.name.clone()) {
                self.d.error("PYP2009", p.name.span, format!("parametro '{}' duplicado", p.name.name));
            }
            let ty = self.resolve_type(&p.ann, false);
            params.push((p.name.name.clone(), ty));
        }
        let ret = match &f.ret {
            Some(r) => self.resolve_type(r, true),
            None => {
                self.d.error(
                    "PYP2004",
                    f.name.span,
                    format!("la funcion '{}' necesita anotacion de retorno (usa '-> None' si no devuelve valor)", f.name.name),
                );
                Type::Error
            }
        };
        let mut is_interrupt = false;
        let mut is_init = false;
        for dec in &f.decorators {
            if dec.name == "interrupt" {
                if self.functions.iter().any(|x| x.is_interrupt) {
                    self.d.error("PYP2005", dec.span, "solo se permite una funcion @interrupt");
                }
                is_interrupt = true;
                if !params.is_empty() || !matches!(ret, Type::Void | Type::Error) {
                    self.d.error("PYP2006", f.name.span, "una funcion @interrupt no recibe parametros ni devuelve valor");
                }
            } else if dec.name == "init_device" {
                if self.functions.iter().any(|x| x.is_init) {
                    self.d.error("PYP2099", dec.span, "solo se permite una funcion @init_device");
                }
                is_init = true;
                if !params.is_empty() || !matches!(ret, Type::Void | Type::Error) {
                    self.d.error("PYP2097", f.name.span, "una funcion @init_device no recibe parametros ni devuelve valor: la ejecuta el arranque antes de main");
                }
            } else {
                self.d.error("PYP1004", dec.span, "decorador no soportado (solo @interrupt y @init_device)");
            }
        }
        if is_interrupt && is_init {
            self.d.error("PYP2097", f.name.span, "una funcion no puede ser a la vez @interrupt y @init_device");
        }
        self.fidx.insert(f.name.name.clone(), self.functions.len());
        self.functions.push(FuncInfo {
            name: f.name.name.clone(),
            params,
            ret,
            is_interrupt,
            is_init,
            calls: BTreeSet::new(),
            span: f.name.span,
        });
    }

    fn declare_variable(&mut self, v: &VarDecl, is_global: bool) {
        let name = &v.name.name;
        let (inner, is_final) = match &v.ann.kind {
            TypeExprKind::Final(inner) => (&**inner, true),
            _ => (&v.ann, false),
        };
        self.in_const_decl = is_final;
        let ty = self.resolve_type(inner, false);
        self.in_const_decl = false;
        if v.value.is_none() {
            self.d.error("PYP2007", v.span, format!("'{name}' debe inicializarse en su declaracion (no hay variables sin inicializar)"));
        }
        let exists = if is_global { self.globals.contains_key(name) } else { self.locals.contains_key(name) };
        if !is_global && self.globals.contains_key(name) {
            self.d.error("PYP2008", v.name.span, format!("'{name}' oculta una variable global del mismo nombre"));
        } else if exists || self.fidx.contains_key(name) || self.pin_names.contains_key(name) || self.time_fns.contains_key(name) {
            self.d.error("PYP2009", v.name.span, format!("'{name}' ya esta declarado"));
        }
        let mut konst = None;
        if let Some(val) = &v.value {
            if ty != Type::Error {
                self.expect(val, ty, None);
                konst = self.konst(val);
                if konst.is_none() && (is_global || is_final) {
                    let (code, msg) = if is_global {
                        ("PYP2010", "el inicializador de una variable global debe ser una constante")
                    } else {
                        ("PYP2011", "un valor Final debe ser una expresion constante")
                    };
                    self.d.error(code, val.span, msg);
                }
            } else {
                self.expr(val);
            }
        }
        if is_global && name == "CLOCK_HZ" {
            self.check_clock(v, ty, is_final, konst);
        }
        let sym = Symbol {
            name: name.clone(),
            ty,
            kind: if is_global { SymKind::Global } else { SymKind::Local },
            konst: if is_final { konst } else { None },
            is_final,
        };
        if is_global {
            self.globals.insert(name.clone(), sym);
        } else {
            self.locals.insert(name.clone(), sym);
        }
    }

    // ------------------------------------------------------------ funciones
    fn check_function(&mut self, f: &FuncDef) {
        let Some(&idx) = self.fidx.get(&f.name.name) else { return };
        if self.functions[idx].span != f.name.span {
            return; // duplicado ya reportado
        }
        self.cur = Some(idx);
        self.locals.clear();
        self.global_decls.clear();
        self.loop_depth = 0;
        let params = self.functions[idx].params.clone();
        for (name, ty) in params {
            if self.globals.contains_key(&name) {
                self.d.error("PYP2008", f.name.span, format!("el parametro '{name}' oculta una variable global"));
            }
            self.locals.insert(name.clone(), Symbol { name, ty, kind: SymKind::Param, konst: None, is_final: false });
        }
        self.block(&f.body);
        let ret = self.functions[idx].ret;
        if !matches!(ret, Type::Void | Type::Error) && !terminates(&f.body) {
            self.d.error("PYP2012", f.name.span, format!("la funcion '{}' debe devolver {ret} en todos los caminos", f.name.name));
        }
        self.cur = None;
    }

    fn check_recursion(&mut self) {
        for f in &self.functions {
            let mut seen = HashSet::new();
            let mut stack: Vec<&String> = f.calls.iter().collect();
            let mut recursive = false;
            while let Some(cur) = stack.pop() {
                if *cur == f.name {
                    recursive = true;
                    break;
                }
                if !seen.insert(cur.clone()) {
                    continue;
                }
                if let Some(&i) = self.fidx.get(cur) {
                    stack.extend(self.functions[i].calls.iter());
                }
            }
            if recursive {
                self.d.error(
                    "PYP2013",
                    f.span,
                    format!("recursion no permitida ('{}' se llama a si misma, directa o indirectamente): la memoria es estatica y la pila de hardware es limitada", f.name),
                );
            }
        }
    }

    /// Una funcion alcanzable desde `main` y desde la @interrupt compartiria sus
    /// variables estaticas: la ISR podria corromper una ejecucion en curso.
    fn check_interrupt_sharing(&mut self) {
        let Some(isr) = self.functions.iter().find(|f| f.is_interrupt) else { return };
        let reach = |roots: Vec<String>, fns: &[FuncInfo], idx: &HashMap<String, usize>| {
            let mut seen: BTreeSet<String> = BTreeSet::new();
            let mut stack = roots;
            while let Some(n) = stack.pop() {
                if let Some(&i) = idx.get(&n) {
                    if seen.insert(n) {
                        stack.extend(fns[i].calls.iter().cloned());
                    }
                }
            }
            seen
        };
        let isr_name = isr.name.clone();
        let isr_reach = reach(isr.calls.iter().cloned().collect(), &self.functions, &self.fidx);
        // contexto principal: `main` si existe; si no, toda funcion sin llamadores (que no sea la ISR)
        let called: BTreeSet<&String> = self.functions.iter().flat_map(|f| f.calls.iter()).collect();
        let roots: Vec<String> = if self.fidx.contains_key("main") {
            std::iter::once("main".to_string()).chain(self.functions.iter().filter(|f| f.is_init).map(|f| f.name.clone())).collect()
        } else {
            self.functions.iter().filter(|f| !f.is_interrupt && !called.contains(&f.name)).map(|f| f.name.clone()).collect()
        };
        let main_reach = reach(roots, &self.functions, &self.fidx);
        for name in isr_reach.intersection(&main_reach) {
            if *name == isr_name {
                continue;
            }
            let span = self.functions[self.fidx[name]].span;
            self.d.error(
                "PYP2060",
                span,
                format!("'{name}' se llama desde la interrupcion y desde el codigo principal; su memoria es estatica y la ISR podria corromperla. Crea una version separada para cada contexto"),
            );
        }
    }

    /// Profundidad maxima de CALL anidados desde `f` (hoja = 0). Sin recursion es finita;
    /// el guardia de ciclos solo evita bucles si ya se reporto `PYP2013`.
    fn call_depth(&self, f: &str, visiting: &mut HashSet<String>, memo: &mut HashMap<String, u32>) -> u32 {
        if let Some(&d) = memo.get(f) {
            return d;
        }
        let Some(&i) = self.fidx.get(f) else { return 0 };
        if !visiting.insert(f.to_string()) {
            return 0;
        }
        let calls: Vec<String> = self.functions[i].calls.iter().cloned().collect();
        let d = calls.iter().map(|c| 1 + self.call_depth(c, visiting, memo)).max().unwrap_or(0);
        visiting.remove(f);
        memo.insert(f.to_string(), d);
        d
    }

    /// La pila de retorno de hardware es chica, circular y no detecta desbordamiento:
    /// el peor caso (main en su punto mas profundo + interrupcion + sus llamadas) debe caber.
    fn check_stack_depth(&mut self) {
        let Some(limit) = self.target.stack_levels else { return };
        let (mut visiting, mut memo) = (HashSet::new(), HashMap::new());
        let called: BTreeSet<&String> = self.functions.iter().flat_map(|f| f.calls.iter()).collect();
        let roots: Vec<String> = if self.fidx.contains_key("main") {
            std::iter::once("main".to_string()).chain(self.functions.iter().filter(|f| f.is_init).map(|f| f.name.clone())).collect()
        } else {
            self.functions.iter().filter(|f| !f.is_interrupt && !called.contains(&f.name)).map(|f| f.name.clone()).collect()
        };
        // el arranque entra al contexto principal con un CALL (1 nivel)
        let mut worst: Option<(u32, String)> = None;
        for r in &roots {
            let u = 1 + self.call_depth(r, &mut visiting, &mut memo);
            if worst.as_ref().map_or(true, |(w, _)| u > *w) {
                worst = Some((u, r.clone()));
            }
        }
        let (main_use, main_root) = worst.unwrap_or((0, String::new()));
        // la entrada a la interrupcion empuja el PC (1 nivel) y la ISR puede llamar a otras
        let isr = self.functions.iter().find(|f| f.is_interrupt).map(|f| f.name.clone());
        let isr_use = isr.as_ref().map_or(0, |n| 1 + self.call_depth(n, &mut visiting, &mut memo));
        let total = main_use + isr_use;
        if total > limit {
            let at = if main_use > 0 { &main_root } else { isr.as_ref().unwrap() };
            let span = self.functions[self.fidx[at]].span;
            self.d.error(
                "PYP2061",
                span,
                format!(
                    "la pila de retorno de hardware de '{}' tiene {limit} niveles y el peor caso necesita {total} \
                     (contexto principal: {main_use}, incluida la llamada de arranque; interrupcion: {isr_use}, incluida su entrada). \
                     Reduce el anidamiento de llamadas",
                    self.target.name
                ),
            );
        }
    }

    // ----------------------------------------------------------- sentencias
    fn block(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn lookup(&self, name: &str) -> Option<Symbol> {
        self.locals.get(name).or_else(|| self.globals.get(name)).cloned()
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Pass => {}
            StmtKind::Decl(v) => self.declare_variable(v, false),
            StmtKind::Global(names) => {
                for n in names {
                    match self.globals.get(&n.name) {
                        None => self.d.error("PYP2020", n.span, format!("'{}' no es una variable global", n.name)),
                        Some(g) if g.is_final => {
                            self.d.error("PYP2021", n.span, format!("'{}' es constante (Final) y no puede modificarse", n.name))
                        }
                        Some(_) => {
                            self.global_decls.insert(n.name.clone());
                        }
                    }
                }
            }
            StmtKind::Break | StmtKind::Continue => {
                if self.loop_depth == 0 {
                    self.d.error("PYP2014", s.span, "'break'/'continue' fuera de un bucle");
                }
            }
            StmtKind::Return(v) => {
                let ret = self.functions[self.cur.unwrap()].ret;
                match (v, ret) {
                    (None, Type::Void | Type::Error) => {}
                    (None, r) => self.d.error("PYP2015", s.span, format!("'return' sin valor en una funcion que devuelve {r}")),
                    (Some(e), Type::Void) => {
                        self.expr(e);
                        self.d.error("PYP2015", s.span, "esta funcion no devuelve valor ('-> None')");
                    }
                    (Some(e), Type::Error) => {
                        self.expr(e);
                    }
                    (Some(e), r) => self.expect(e, r, None),
                }
            }
            StmtKind::Expr(e) => {
                self.expr(e);
            }
            StmtKind::Assign { target, value } => match self.assign_target(target) {
                Some(sym) if sym.ty != Type::Error => self.expect(value, sym.ty, None),
                _ => {
                    self.expr(value);
                }
            },
            StmtKind::AugAssign { target, op, value } => {
                let sym = self.assign_target(target);
                match sym {
                    Some(sym) if sym.ty.is_int() => {
                        if matches!(op, BinOp::Shl | BinOp::Shr) {
                            self.shift_amount(value, sym.ty);
                        } else {
                            self.expect(value, sym.ty, None);
                        }
                    }
                    Some(sym) => {
                        if sym.ty != Type::Error {
                            self.d.error("PYP2024", s.span, format!("el operador '{}=' requiere un entero, no {}", op.symbol(), sym.ty));
                        }
                        self.expr(value);
                    }
                    None => {
                        self.expr(value);
                    }
                }
            }
            StmtKind::If { test, body, orelse } => {
                self.cond(test);
                self.block(body);
                self.block(orelse);
            }
            StmtKind::While { test, body } => {
                self.cond(test);
                self.loop_depth += 1;
                self.block(body);
                self.loop_depth -= 1;
            }
            StmtKind::For { var, iter_span, args, body } => self.for_stmt(var, *iter_span, args, body),
        }
    }

    fn cond(&mut self, test: &Expr) {
        self.expect(test, Type::Bool, Some("la condicion debe ser bool (usa una comparacion, p. ej. 'x != 0')"));
    }

    fn assign_target(&mut self, t: &Ident) -> Option<Symbol> {
        let Some(sym) = self.lookup(&t.name) else {
            self.d.error("PYP2022", t.span, format!("'{0}' no esta declarada; declarala con tipo: '{0}: u8 = ...'", t.name));
            return None;
        };
        if sym.is_final {
            self.d.error("PYP2021", t.span, format!("'{}' es constante (Final) y no puede modificarse", t.name));
            return None;
        }
        if sym.kind == SymKind::Global && !self.global_decls.contains(&t.name) {
            self.d.error("PYP2023", t.span, format!("para modificar la global '{0}' declara 'global {0}' en la funcion", t.name));
        }
        Some(sym)
    }

    fn for_stmt(&mut self, var: &Ident, iter_span: Span, args: &[Expr], body: &[Stmt]) {
        let types: Vec<Type> = args.iter().map(|a| self.expr(a)).collect();
        let var_t = if types.contains(&Type::Error) {
            Type::Error
        } else if types.iter().any(|t| !matches!(t, Type::Int { .. } | Type::Literal)) {
            self.d.error("PYP2025", iter_span, "los argumentos de range() deben ser enteros");
            Type::Error
        } else {
            let mut typed: Vec<Type> = types.iter().copied().filter(|t| t.is_int()).collect();
            typed.dedup();
            typed.sort_by_key(|t| (t.bits(), t.is_signed()));
            typed.dedup();
            if typed.len() > 1 {
                let names: Vec<String> = typed.iter().map(|t| t.to_string()).collect();
                self.d.error("PYP2026", iter_span, format!("los argumentos de range() mezclan tipos distintos: {}", names.join(", ")));
                Type::Error
            } else if let Some(&t) = typed.first() {
                for (a, ty) in args.iter().zip(&types) {
                    if *ty == Type::Literal {
                        self.coerce_literal(a, t);
                    }
                }
                t
            } else {
                // todo literal: el tipo mas chico del destino que contenga todos los valores
                let vals: Vec<i128> = args.iter().map(|a| self.konst(a).unwrap()).collect();
                let pick = INFERENCE_ORDER.iter().copied().find(|c| self.target.supports(*c) && vals.iter().all(|v| c.contains(*v)));
                match pick {
                    Some(t) => {
                        self.note_emulated(t, iter_span);
                        for a in args {
                            let k = self.konst(a);
                            self.set(a, t, k);
                        }
                        t
                    }
                    None => {
                        self.d.error("PYP2027", iter_span, "los limites de range() no caben en ningun tipo del destino");
                        Type::Error
                    }
                }
            }
        };
        if let Some(step) = args.get(2) {
            match self.konst(step) {
                None if self.ty(step) != Type::Error => self.d.error("PYP2028", step.span, "el paso de range() debe ser una constante"),
                Some(0) => self.d.error("PYP2028", step.span, "el paso de range() no puede ser 0"),
                _ => {}
            }
        }
        match self.lookup(&var.name) {
            None => {
                self.locals.insert(var.name.clone(), Symbol { name: var.name.clone(), ty: var_t, kind: SymKind::Local, konst: None, is_final: false });
            }
            Some(s) if s.is_final || (s.kind == SymKind::Global && !self.global_decls.contains(&var.name)) => {
                self.d.error("PYP2023", var.span, format!("'{}' no puede usarse como variable de bucle", var.name));
            }
            Some(s) if s.ty != var_t && s.ty != Type::Error && var_t != Type::Error => {
                self.d.error("PYP2029", var.span, format!("'{}' es {} pero range() produce {}", var.name, s.ty, var_t));
            }
            Some(_) => {}
        }
        self.loop_depth += 1;
        self.block(body);
        self.loop_depth -= 1;
    }

    // ---------------------------------------------------------- expresiones
    fn coerce_literal(&mut self, e: &Expr, target: Type) -> bool {
        let v = self.konst(e).unwrap_or(0);
        if !target.contains(v) {
            let rng = if target.is_int() { format!("{}..{}", target.min(), target.max()) } else { target.to_string() };
            self.d.error("PYP2040", e.span, format!("el valor {v} no cabe en {target} (rango {rng})"));
            self.set(e, Type::Error, None);
            return false;
        }
        self.info[e.id as usize].ty = target;
        true
    }

    /// Comprueba que `e` tenga tipo `target` (los literales se adaptan).
    fn expect(&mut self, e: &Expr, target: Type, msg: Option<&str>) {
        let t = self.expr(e);
        if t == Type::Error || target == Type::Error {
            return;
        }
        if t == Type::Literal {
            if !target.is_int() {
                let m = msg.map(String::from).unwrap_or_else(|| format!("se esperaba {target}, se encontro un entero"));
                self.d.error("PYP2041", e.span, m);
            } else {
                self.coerce_literal(e, target);
            }
        } else if t != target {
            let hint = if t.is_int() && target.is_int() { format!("; usa {target}(...) para convertir explicitamente") } else { String::new() };
            let m = msg.map(String::from).unwrap_or_else(|| format!("se esperaba {target}, se encontro {t}{hint}"));
            self.d.error("PYP2041", e.span, m);
        }
    }

    fn expr(&mut self, e: &Expr) -> Type {
        match &e.kind {
            ExprKind::Int(v) => self.set(e, Type::Literal, Some(*v)),
            ExprKind::Bool(b) => self.set(e, Type::Bool, Some(*b as i128)),
            ExprKind::Name(n) => self.name(e, n),
            ExprKind::Bool_(_, vals) => {
                for v in vals {
                    self.expect(v, Type::Bool, Some("los operandos de 'and'/'or' deben ser bool"));
                }
                self.set(e, Type::Bool, None)
            }
            ExprKind::Unary(op, inner) => self.unary(e, *op, inner),
            ExprKind::Binary(op, l, r) => self.binary(e, *op, l, r),
            ExprKind::Compare(op, l, r) => self.compare(e, *op, l, r),
            ExprKind::Call { func, args, .. } => self.call(e, func, args),
            ExprKind::MethodCall { obj, obj_span, method, args } => self.method_call(e, obj, *obj_span, method, args),
        }
    }

    fn name(&mut self, e: &Expr, n: &str) -> Type {
        if let Some(sym) = self.lookup(n) {
            return self.set(e, sym.ty, sym.konst);
        }
        if self.pin_names.contains_key(n) {
            self.d.error("PYP2077", e.span, format!("'{n}' es un pin: usa sus metodos ({n}.on(), {n}.off(), {n}.toggle(), {n}.value())"));
        } else if self.fidx.contains_key(n) {
            self.d.error("PYP2042", e.span, format!("'{n}' es una funcion; llamala con parentesis"));
        } else if PREDECLARED.contains(&n) {
            self.d.error("PYP2042", e.span, format!("'{n}' es un tipo; no puede usarse como valor"));
        } else {
            self.d.error("PYP2043", e.span, format!("nombre '{n}' no definido"));
        }
        self.set(e, Type::Error, None)
    }

    fn unary(&mut self, e: &Expr, op: UnOp, inner: &Expr) -> Type {
        if op == UnOp::Not {
            self.expect(inner, Type::Bool, Some("'not' requiere un bool"));
            return self.set(e, Type::Bool, None);
        }
        let t = self.expr(inner);
        if t == Type::Error {
            return self.set(e, Type::Error, None);
        }
        if !matches!(t, Type::Int { .. } | Type::Literal) {
            self.d.error("PYP2044", e.span, format!("operador unario no aplicable a {t}"));
            return self.set(e, Type::Error, None);
        }
        let c = self.konst(inner);
        let r = match op {
            UnOp::Pos => c,
            UnOp::Neg => {
                if t.is_int() && !t.is_signed() {
                    self.d.error("PYP2044", e.span, format!("no se puede negar un {t} sin signo"));
                    return self.set(e, Type::Error, None);
                }
                c.map(|c| -c)
            }
            UnOp::Invert => {
                if t == Type::Literal {
                    self.d.error("PYP2044", e.span, "'~' requiere un operando con tipo explicito (p. ej. ~u8(5))");
                    return self.set(e, Type::Error, None);
                }
                c.map(|c| t.wrap(!c))
            }
            UnOp::Not => unreachable!(),
        };
        if let Some(r) = r {
            if t.is_int() && !t.contains(r) {
                self.d.error("PYP2045", e.span, format!("la constante {r} desborda {t}"));
                return self.set(e, Type::Error, None);
            }
        }
        self.set(e, t, r)
    }

    /// Tipo comun de dos operandos enteros; `None` si hay error (ya reportado).
    fn unify(&mut self, l: &Expr, r: &Expr, lt: Type, rt: Type, op: &str) -> Option<Type> {
        for (t, n) in [(lt, l), (rt, r)] {
            if !matches!(t, Type::Int { .. } | Type::Literal) {
                self.d.error("PYP2046", n.span, format!("el operador '{op}' requiere enteros, no {t}"));
                return None;
            }
        }
        match (lt, rt) {
            (Type::Literal, Type::Literal) => Some(Type::Literal),
            (Type::Literal, t) => self.coerce_literal(l, t).then_some(t),
            (t, Type::Literal) => self.coerce_literal(r, t).then_some(t),
            (a, b) if a != b => {
                self.d.error("PYP2047", l.span.to(r.span), format!("tipos incompatibles para '{op}': {a} y {b}; convierte explicitamente, p. ej. {a}(...)"));
                None
            }
            (a, _) => Some(a),
        }
    }

    fn shift_amount(&mut self, e: &Expr, left: Type) {
        let t = self.expr(e);
        if t == Type::Error {
            return;
        }
        if t == Type::Literal {
            let v = self.konst(e).unwrap();
            if v < 0 || v >= left.bits() as i128 {
                self.d.error("PYP2048", e.span, format!("desplazamiento {v} fuera de rango 0..{} para {left}", left.bits() - 1));
            } else {
                self.set(e, U8, Some(v));
            }
        } else if !(t.is_int() && !t.is_signed()) {
            self.d.error("PYP2048", e.span, "la cantidad de desplazamiento debe ser un literal o un entero sin signo");
        }
    }

    fn binary(&mut self, e: &Expr, op: BinOp, l: &Expr, r: &Expr) -> Type {
        let sym = op.symbol();
        let lt = self.expr(l);
        if matches!(op, BinOp::Shl | BinOp::Shr) {
            if lt == Type::Literal {
                let rt = self.expr(r);
                if rt == Type::Literal {
                    let (a, b) = (self.konst(l).unwrap(), self.konst(r).unwrap());
                    if b < 0 {
                        self.d.error("PYP2048", r.span, "desplazamiento negativo");
                        return self.set(e, Type::Error, None);
                    }
                    return match fold(op, a, b) {
                        Some(v) => self.set(e, Type::Literal, Some(v)),
                        None => {
                            self.d.error("PYP2045", e.span, "constante demasiado grande");
                            self.set(e, Type::Error, None)
                        }
                    };
                }
                if rt != Type::Error {
                    self.d.error("PYP2049", l.span, "el operando izquierdo de un desplazamiento necesita tipo explicito (p. ej. u8(1) << n)");
                }
                return self.set(e, Type::Error, None);
            }
            if !lt.is_int() {
                if lt != Type::Error {
                    self.d.error("PYP2046", l.span, format!("el operador '{sym}' requiere enteros, no {lt}"));
                }
                self.expr(r);
                return self.set(e, Type::Error, None);
            }
            self.shift_amount(r, lt);
            if let (Some(a), Some(b)) = (self.konst(l), self.konst(r)) {
                if (0..lt.bits() as i128).contains(&b) {
                    let v = fold(op, a, b).unwrap_or(i128::MAX);
                    if !lt.contains(v) {
                        self.d.error("PYP2045", e.span, format!("la constante {v} desborda {lt}"));
                        return self.set(e, Type::Error, None);
                    }
                    return self.set(e, lt, Some(v));
                }
            }
            return self.set(e, lt, None);
        }
        let rt = self.expr(r);
        if lt == Type::Error || rt == Type::Error {
            return self.set(e, Type::Error, None);
        }
        let Some(t) = self.unify(l, r, lt, rt, sym) else { return self.set(e, Type::Error, None) };
        let (a, b) = (self.konst(l), self.konst(r));
        if matches!(op, BinOp::Div | BinOp::Mod) && b == Some(0) {
            self.d.error("PYP2050", r.span, "division por cero");
            return self.set(e, Type::Error, None);
        }
        if let (Some(a), Some(b)) = (a, b) {
            return match fold(op, a, b) {
                Some(v) if !t.is_int() || t.contains(v) => self.set(e, t, Some(v)),
                Some(v) => {
                    self.d.error("PYP2045", e.span, format!("la constante {v} desborda {t}"));
                    self.set(e, Type::Error, None)
                }
                None => {
                    self.d.error("PYP2045", e.span, "constante demasiado grande");
                    self.set(e, Type::Error, None)
                }
            };
        }
        self.set(e, t, None)
    }

    fn compare(&mut self, e: &Expr, op: CmpOp, l: &Expr, r: &Expr) -> Type {
        let (lt, rt) = (self.expr(l), self.expr(r));
        if lt == Type::Error || rt == Type::Error {
            return self.set(e, Type::Bool, None);
        }
        if lt == Type::Bool && rt == Type::Bool {
            if !matches!(op, CmpOp::Eq | CmpOp::Ne) {
                self.d.error("PYP2046", e.span, "los bool solo se comparan con '==' y '!='");
            }
            return self.set(e, Type::Bool, None);
        }
        let t = self.unify(l, r, lt, rt, op.symbol());
        if let (Some(_), Some(a), Some(b)) = (t, self.konst(l), self.konst(r)) {
            let v = match op {
                CmpOp::Eq => a == b,
                CmpOp::Ne => a != b,
                CmpOp::Lt => a < b,
                CmpOp::Le => a <= b,
                CmpOp::Gt => a > b,
                CmpOp::Ge => a >= b,
            };
            return self.set(e, Type::Bool, Some(v as i128));
        }
        self.set(e, Type::Bool, None)
    }

    fn call(&mut self, e: &Expr, func: &str, args: &[Expr]) -> Type {
        if let Some(&unit) = self.time_fns.get(func) {
            return self.time_call(e, func, unit, args);
        }
        if let Some(t) = Type::from_name(func).filter(|t| t.is_int()) {
            return self.cast(e, t, args);
        }
        if func == "bool" {
            self.d.error("PYP2051", e.span, "no existe bool(x); usa una comparacion, p. ej. 'x != 0'");
        } else if func == "range" {
            self.d.error("PYP2051", e.span, "range() solo puede usarse como iterable de un 'for'");
        } else if let Some(&i) = self.fidx.get(func) {
            return self.call_user(e, i, args);
        } else {
            let hint = if time_unit(func).is_some() { format!("; importala con 'from pypic.system.time import {func}'") } else { String::new() };
            self.d.error("PYP2043", e.span, format!("funcion '{func}' no definida{hint}"));
        }
        for a in args {
            self.expr(a);
        }
        self.set(e, Type::Error, None)
    }

    fn cast(&mut self, e: &Expr, target: Type, args: &[Expr]) -> Type {
        for a in args {
            self.expr(a);
        }
        if args.len() != 1 {
            self.d.error("PYP2052", e.span, format!("{target}() recibe exactamente un argumento"));
            return self.set(e, Type::Error, None);
        }
        if !self.target.supports(target) {
            self.d.error("PYP2030", e.span, format!("el tipo '{target}' no esta soportado por el destino '{}'", self.target.name));
            return self.set(e, Type::Error, None);
        }
        self.note_emulated(target, e.span);
        let a = &args[0];
        let t = self.ty(a);
        match t {
            Type::Error => self.set(e, Type::Error, None),
            Type::Literal => {
                if !self.coerce_literal(a, target) {
                    return self.set(e, Type::Error, None);
                }
                let k = self.konst(a);
                self.set(e, target, k)
            }
            Type::Int { .. } | Type::Bool => {
                // cast explicito: el truncamiento es intencional
                let k = self.konst(a).map(|c| target.wrap(c));
                self.set(e, target, k)
            }
            _ => {
                self.d.error("PYP2053", e.span, format!("no se puede convertir {t} a {target}"));
                self.set(e, Type::Error, None)
            }
        }
    }

    fn call_user(&mut self, e: &Expr, fi: usize, args: &[Expr]) -> Type {
        let f = self.functions[fi].clone();
        if f.is_init {
            self.d.error("PYP2098", e.span, format!("'{}' es la funcion @init_device: la ejecuta el arranque antes de main y no se llama desde el codigo", f.name));
        }
        if f.is_interrupt {
            self.d.error("PYP2054", e.span, format!("'{}' es una rutina de interrupcion y no puede llamarse desde el codigo", f.name));
        }
        if let Some(cur) = self.cur {
            self.functions[cur].calls.insert(f.name.clone());
        }
        if args.len() != f.params.len() {
            self.d.error("PYP2055", e.span, format!("'{}' espera {} argumento(s), recibio {}", f.name, f.params.len(), args.len()));
            for a in args {
                self.expr(a);
            }
        } else {
            for (a, (_, pt)) in args.iter().zip(&f.params) {
                if *pt == Type::Error {
                    self.expr(a);
                } else {
                    self.expect(a, *pt, None);
                }
            }
        }
        self.set(e, f.ret, None)
    }
}

// ---- arbol de modulos del SDK: tres categorias ------------------------------------------------------
/// `pypic.periph.*`: hardware integrado del chip (validado por el Device Pack).
const PERIPH_MODULES: &[&str] = &["gpio", "adc", "timer", "pwm", "ccp", "eccp", "uart", "spi", "i2c", "comparator", "vref", "eeprom", "latch"];
/// `pypic.system.*`: CPU, reloj y energia.
const SYSTEM_MODULES: &[&str] = &["time", "irq", "wdt", "clock", "power", "flash"];
/// `pypic.soft.*`: implementaciones por software sobre cualquier GPIO (cuestan ciclos de CPU).
const SOFT_MODULES: &[&str] = &["spi", "i2c", "uart", "onewire"];
const IMPLEMENTED_MODULES: &[&str] = &["pypic.periph.gpio", "pypic.system.time"];

/// Problema con la ruta de un modulo `pypic.*` (renombrado, categoria sin modulo, inexistente o aun no implementado).
/// `None` si es un modulo implementado o no es del arbol de categorias.
fn module_problem(module: &str) -> Option<(&'static str, String)> {
    for (old, new) in [("pypic.gpio", "pypic.periph.gpio"), ("pypic.time", "pypic.system.time")] {
        if module == old {
            return Some((
                "PYP2106",
                format!("'{old}' se llama ahora '{new}': los modulos se agrupan por categoria (periph = hardware del chip, system = CPU, reloj y energia, soft = implementaciones por software)"),
            ));
        }
    }
    let categories: [(&str, &[&str], &str); 3] = [
        ("periph", PERIPH_MODULES, "periferico por hardware del chip"),
        ("system", SYSTEM_MODULES, "servicio del sistema: CPU, reloj o energia"),
        ("soft", SOFT_MODULES, "implementacion por software sobre cualquier GPIO"),
    ];
    for (cat, names, what) in categories {
        let pkg = format!("pypic.{cat}");
        if module == pkg {
            return Some(("PYP1002", format!("'{pkg}' es una categoria, no un modulo: importa desde uno de sus modulos ({})", names.join(", "))));
        }
        if let Some(name) = module.strip_prefix(&format!("{pkg}.")) {
            if IMPLEMENTED_MODULES.contains(&module) {
                return None;
            }
            return Some(if names.contains(&name) {
                ("PYP2105", format!("'{module}' ({what}) todavia no esta implementado; ver docs/catalogo-pic16f887.md y el issue #23"))
            } else {
                ("PYP1002", format!("'{module}' no existe; en '{pkg}' hay: {}", names.join(", ")))
            });
        }
    }
    None
}

/// `PIC16F887`, `dsPIC33...`
fn looks_like_device(s: &str) -> bool {
    let rest = s.strip_prefix("dsPIC").or_else(|| s.strip_prefix("PIC"));
    rest.is_some_and(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// `RD0` -> ('D', 0)
fn parse_pin_name(s: &str) -> Option<(char, u8)> {
    let b = s.as_bytes();
    if b.len() >= 3 && b[0] == b'R' && b[1].is_ascii_uppercase() && s[2..].bytes().all(|c| c.is_ascii_digit()) {
        let bit: u8 = s[2..].parse().ok()?;
        if bit <= 15 {
            return Some((b[1] as char, bit));
        }
    }
    None
}

/// Analisis simple de "todos los caminos terminan en return".
fn terminates(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match &s.kind {
        StmtKind::Return(_) => true,
        StmtKind::If { body, orelse, .. } => !orelse.is_empty() && terminates(body) && terminates(orelse),
        StmtKind::While { test, body } => matches!(test.kind, ExprKind::Bool(true)) && !has_break(body),
        _ => false,
    })
}

fn has_break(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match &s.kind {
        StmtKind::Break => true,
        StmtKind::If { body, orelse, .. } => has_break(body) || has_break(orelse),
        _ => false, // los break de bucles anidados no cuentan
    })
}
