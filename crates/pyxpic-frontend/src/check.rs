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

use crate::adc::{self, AdcCall, AdcOp};
use crate::bits::{self, BitsOp};
use crate::clock::{self, ClockKind, ClockSource};
use crate::comparator::{self, CmpId, CmpRef, CompCall, CompOp, CvrefOp};
use crate::eeprom::{EepromCall, EepromOp};
use crate::i2c::{self, I2cCall, I2cOp, I2cStart};
use crate::pwm::{self, PwmCall, PwmId, PwmOp, PwmTiming};
use crate::regs::{RegCall, RegOp, BANK_BITS, WRITE_PROTECTED};
use crate::spi::{self, SpiCall, SpiOp, SpiStart};
use crate::target::RegInfo;
use crate::units::Dim;
use crate::text::{TextCall, TextOp};
use crate::timer::{self, Gate, GateSource, TimerCall, TimerId, TimerOp, TimerSource, TimerStart};
use crate::uart::{self, FmtPart, UartCall, UartOp, UartStart};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::ast::*;
use crate::diag::{Diagnostics, Span};
use crate::target::{PinInfo, TargetProfile};
use crate::types::{Type, INFERENCE_ORDER, U16, U32, U8};

/// Nombres predeclarados (no requieren import; `from pyxpic import ...` es
/// opcional para que el archivo siga siendo Python valido en otros editores).
const PREDECLARED: &[&str] = &["uint8", "int8", "uint16", "int16", "uint32", "int32", "u8", "i8", "u16", "i16", "u32", "i32", "bool", "Const", "interrupt", "init_device", "asm_function"];

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
    /// Decorada con `@asm_function`: el cuerpo es ensamblador (lo valida el backend).
    pub is_asm: bool,
    pub calls: BTreeSet<String>,
    /// Lee alguna tabla con un indice variable: la consulta es un CALL (un nivel de pila).
    pub uses_rom: bool,
    /// Niveles de pila que usa su peor rutina de runtime (`*`, `//`, `%`): 0 si no usa ninguna.
    pub runtime_depth: u32,
    pub span: Span,
}

/// Unidad de una espera (`pyxpic.system.time`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelayUnit {
    /// Ciclos de instruccion: la unidad minima del hardware.
    Cycles,
    Us,
    Ms,
    S,
}

/// Funciones de `pyxpic.system.time` implementadas (nombre -> unidad). Una sola forma por operacion, sin sinonimos:
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

/// Un pin importado con `from pyxpic.periph.gpio import RD0` (no ocupa memoria: se resuelve al compilar).
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
    /// Dispositivo declarado con `from pyxpic.device import PIC16F887`.
    pub device: Option<String>,
    /// Reloj efectivo: el de `Clock: ...` (o la forma manual `CLOCK_HZ: Const[u32]`) o, si usa esperas y no lo declaro, el del dispositivo.
    pub clock_hz: Option<u32>,
    /// `clock_hz` es el reloj por defecto del dispositivo (no se declaro `Clock` ni `CLOCK_HZ`).
    pub clock_defaulted: bool,
    pub clock_source: Option<ClockSource>,
    /// Llamadas a metodos de `pyxpic.periph.timer`, por id de expresion.
    pub timer_calls: BTreeMap<u32, TimerCall>,
    /// Llamadas a metodos de `pyxpic.regs`, por id de expresion.
    pub reg_calls: BTreeMap<u32, RegCall>,
    /// Llamadas a `pyxpic.bits`, por id de expresion.
    pub bits_calls: BTreeMap<u32, BitsOp>,
    /// Tablas constantes (nombre y entradas) y las lecturas con indice variable, por id de expresion.
    pub roms: Vec<(String, Vec<u8>)>,
    pub rom_gets: BTreeMap<u32, usize>,
    /// Arreglos de RAM (`buf: uint8[16]`): nombre y capacidad; y las lecturas `buf[i]` con indice variable, por id de expresion.
    pub arrays: Vec<(String, usize)>,
    /// Los valores iniciales de cada arreglo (`buf: uint8[4] = [1, 2, 3, 4]`); vacio si empieza en cero.
    pub array_inits: Vec<Vec<u8>>,
    pub array_gets: BTreeMap<u32, usize>,
    /// Las llamadas a `parse_dec` / `parse_hex`, por id de expresion.
    pub text_calls: BTreeMap<u32, TextCall>,
    /// Llamadas a metodos de `pyxpic.periph.adc`, por id de expresion.
    pub adc_calls: BTreeMap<u32, AdcCall>,
    /// Condiciones electricas que asume el programa para el ADC (adquisicion y alimentacion).
    pub adc_config: adc::AdcConfig,
    /// Llamadas a metodos de `pyxpic.periph.pwm`, por id de expresion.
    pub pwm_calls: BTreeMap<u32, PwmCall>,
    /// Llamadas a metodos de `pyxpic.periph.eeprom`, por id de expresion.
    pub eeprom_calls: BTreeMap<u32, EepromCall>,
    /// Llamadas a metodos de `pyxpic.periph.i2c`, por id de expresion.
    pub i2c_calls: BTreeMap<u32, I2cCall>,
    /// Llamadas a metodos de `pyxpic.periph.spi`, por id de expresion.
    pub spi_calls: BTreeMap<u32, SpiCall>,
    /// Llamadas a metodos de `pyxpic.periph.comparator` y `pyxpic.periph.vref`, por id de expresion.
    pub comp_calls: BTreeMap<u32, CompCall>,
    /// Niveles de la pila de hardware en el peor caso: (contexto principal, interrupcion), con la llamada de arranque y la entrada a la interrupcion incluidas.
    /// `None` si el destino no declara su profundidad de pila.
    pub stack_use: Option<(u32, u32)>,
    /// Llamadas a metodos de `pyxpic.periph.uart`, por id de expresion.
    pub uart_calls: BTreeMap<u32, UartCall>,
    /// Las partes de cada `UART.write_text(f"...")`, por id de expresion.
    pub uart_fmt: BTreeMap<u32, Vec<FmtPart>>,
    /// Operaciones de punto fijo que la IR expande, por id de expresion.
    pub fixed_ops: BTreeMap<u32, FixedOp>,
}

pub struct Checked {
    pub info: Vec<ExprInfo>,
    pub globals: BTreeMap<String, Symbol>,
    pub functions: Vec<FuncInfo>,
    pub pins: Vec<PinDecl>,
    pub device: Option<String>,
    pub clock_hz: Option<u32>,
    pub clock_defaulted: bool,
    pub clock_source: Option<ClockSource>,
    pub timer_calls: BTreeMap<u32, TimerCall>,
    pub reg_calls: BTreeMap<u32, RegCall>,
    pub bits_calls: BTreeMap<u32, BitsOp>,
    pub roms: Vec<(String, Vec<u8>)>,
    pub rom_gets: BTreeMap<u32, usize>,
    pub arrays: Vec<(String, usize)>,
    /// Los valores iniciales de cada arreglo (`buf: uint8[4] = [1, 2, 3, 4]`); vacio si empieza en cero.
    pub array_inits: Vec<Vec<u8>>,
    pub array_gets: BTreeMap<u32, usize>,
    /// Las llamadas a `parse_dec` / `parse_hex`, por id de expresion.
    pub text_calls: BTreeMap<u32, TextCall>,
    pub adc_calls: BTreeMap<u32, AdcCall>,
    pub adc_config: adc::AdcConfig,
    /// Llamadas a metodos de `pyxpic.periph.pwm`, por id de expresion.
    pub pwm_calls: BTreeMap<u32, PwmCall>,
    /// Llamadas a metodos de `pyxpic.periph.eeprom`, por id de expresion.
    pub eeprom_calls: BTreeMap<u32, EepromCall>,
    /// Llamadas a metodos de `pyxpic.periph.i2c`, por id de expresion.
    pub i2c_calls: BTreeMap<u32, I2cCall>,
    /// Llamadas a metodos de `pyxpic.periph.spi`, por id de expresion.
    pub spi_calls: BTreeMap<u32, SpiCall>,
    pub comp_calls: BTreeMap<u32, CompCall>,
    pub stack_use: Option<(u32, u32)>,
    pub uart_calls: BTreeMap<u32, UartCall>,
    pub uart_fmt: BTreeMap<u32, Vec<FmtPart>>,
    pub fixed_ops: BTreeMap<u32, FixedOp>,
}

pub fn check(module: &Module, target: &TargetProfile, d: &mut Diagnostics) -> Checked {
    let mut c = Checker {
        d,
        target,
        info: vec![ExprInfo { ty: Type::Error, konst: None }; module.expr_count as usize],
        dims: vec![None; module.expr_count as usize],
        rats: vec![None; module.expr_count as usize],
        decimal_consts: HashMap::new(),
        quantity_pending: Vec::new(),
        clock_resolved: false,
        clock_decl: None,
        frequency_decl: None,
        osc_mode_decl: None,
        clock_settings: clock::ClockSettings::default(),
        clock_setting_spans: BTreeMap::new(),
        globals: BTreeMap::new(),
        functions: Vec::new(),
        fidx: HashMap::new(),
        cur: None,
        locals: HashMap::new(),
        local_decls: Vec::new(),
        read_names: HashSet::new(),
        global_decls: HashSet::new(),
        loop_depth: 0,
        fn_labels: HashSet::new(),
        warned: HashSet::new(),
        pins: Vec::new(),
        pin_names: HashMap::new(),
        pin_uses: Vec::new(),
        declared_device: None,
        clock_hz: None,
        clock_defaulted: false,
        clock_source: None,
        in_const_decl: false,
        time_fns: HashMap::new(),
        time_uses: Vec::new(),
        timer_names: HashMap::new(),
        timer_consts: HashSet::new(),
        timer_calls: BTreeMap::new(),
        timer_started: BTreeSet::new(),
        timer_needs: BTreeMap::new(),
        timer_every: Vec::new(),
        timer_pins: Vec::new(),
        reg_names: HashMap::new(),
        reg_calls: BTreeMap::new(),
        bits_fns: HashSet::new(),
        claims: Vec::new(),
        rt_warned: BTreeSet::new(),
        adc_name: false,
        adc_consts: HashSet::new(),
        adc_calls: BTreeMap::new(),
        adc_started: false,
        adc_needs: None,
        adc_pending: Vec::new(),
        adc_config: adc::AdcConfig::default(),
        pwm_names: HashSet::new(),
        eeprom_name: false,
        eeprom_calls: BTreeMap::new(),
        i2c_name: false,
        i2c_calls: BTreeMap::new(),
        i2c_pending: Vec::new(),
        i2c_needs: None,
        spi_name: false,
        spi_calls: BTreeMap::new(),
        spi_pending: Vec::new(),
        spi_needs: None,
        comp_names: HashSet::new(),
        comp_consts: HashSet::new(),
        cvref_name: false,
        comp_calls: BTreeMap::new(),
        stack_use: None,
        cvref_started: false,
        cvref_needed: None,
        pwm_calls: BTreeMap::new(),
        pwm_pending: Vec::new(),
        pwm_uses: Vec::new(),
        uart_name: false,
        uart_calls: BTreeMap::new(),
        uart_fmt: BTreeMap::new(),
        fixed_ops: BTreeMap::new(),
        uart_started: false,
        uart_needs: None,
        uart_pending: Vec::new(),
        uart_uses8: None,
        uart_uses9: None,
        roms: Vec::new(),
        rom_idx: HashMap::new(),
        rom_gets: BTreeMap::new(),
        arrays: Vec::new(),
        array_inits: Vec::new(),
        array_idx: HashMap::new(),
        array_gets: BTreeMap::new(),
        text_calls: BTreeMap::new(),
        bits_calls: BTreeMap::new(),
        carry_ok: false,
        carry_prev: false,
        carry_fn: None,
        stmt_top: None,
    };
    c.run(module);
    c.flush_quantities();
    // el punto fijo es un entero para la IR y el PIC: los tipos se llevan a su entero (las operaciones que lo exigen estan en `fixed_ops`)
    for i in c.info.iter_mut() {
        i.ty = i.ty.to_ir();
    }
    for g in c.globals.values_mut() {
        g.ty = g.ty.to_ir();
    }
    for f in c.functions.iter_mut() {
        f.ret = f.ret.to_ir();
        for p in f.params.iter_mut() {
            p.1 = p.1.to_ir();
        }
    }
    Checked {
        info: c.info,
        globals: c.globals,
        functions: c.functions,
        pins: c.pins,
        device: c.declared_device,
        clock_hz: c.clock_hz,
        clock_defaulted: c.clock_defaulted,
        clock_source: c.clock_source,
        timer_calls: c.timer_calls,
        reg_calls: c.reg_calls,
        bits_calls: c.bits_calls,
        roms: c.roms,
        rom_gets: c.rom_gets,
        arrays: c.arrays,
        array_inits: c.array_inits,
        array_gets: c.array_gets,
        text_calls: c.text_calls,
        adc_calls: c.adc_calls,
        adc_config: c.adc_config,
        pwm_calls: c.pwm_calls,
        eeprom_calls: c.eeprom_calls,
        i2c_calls: c.i2c_calls,
        spi_calls: c.spi_calls,
        comp_calls: c.comp_calls,
        stack_use: c.stack_use,
        uart_calls: c.uart_calls,
        uart_fmt: c.uart_fmt,
        fixed_ops: c.fixed_ops,
    }
}

/// Una operacion de punto fijo que la IR expande con un intermedio de 16 bits (`docs/punto-fijo.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixedOp {
    /// `a * b` del mismo tipo: producto de 32 bits y reescala (`Fixed` divide entre 10^scale; `QFixed` desplaza `scale` bits).
    Mul { binary: bool, scale: u8, signed: bool },
    /// `raw * num / den` (conversiones y multiplicar o dividir por una constante); `child` es el operando que se escala: 0 = el izquierdo
    /// (o el unico), 1 = el derecho. La division por una potencia de dos sin signo es un desplazamiento.
    Scale { num: i128, den: i128, child: u8, signed: bool },
}

/// ¿`valor * num` cabe en el intermedio de 16 bits (con signo si `signed`) para todo valor del tipo `src`? El backend PIC14 llega a 16 bits
/// (#32: sin 32 bits todavia), asi que una operacion cuyo intermedio no cabe se rechaza al compilar.
fn fits_wide(src: Type, num: i128, signed: bool) -> bool {
    let m = src.max().max(-src.min());
    let lim = if signed { 32_768 } else { 65_535 };
    m.checked_mul(num.abs()).is_some_and(|p| p <= lim)
}

const WIDE_MSG: &str = "el intermedio pasa de 16 bits y el backend PIC14 todavia no tiene 32 bits (#32)";

/// `num/den` como decimal legible (solo para los mensajes).
fn show_ratio(num: i128, den: i128) -> String {
    let v = num as f64 / den as f64;
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn fold(op: BinOp, a: i128, b: i128) -> Option<i128> {
    match op {
        BinOp::Add => a.checked_add(b),
        BinOp::Sub => a.checked_sub(b),
        BinOp::Mul => a.checked_mul(b),
        BinOp::Div => a.checked_div(b), // trunca hacia cero
        BinOp::TrueDiv => None,         // solo entre decimales (decimal_binary)
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
    /// La magnitud fisica de cada expresion (`8MHz`, `500ms`...): `None` para un numero sin unidad.
    dims: Vec<Option<Dim>>,
    /// El valor exacto (num/den) de cada expresion decimal no entera.
    rats: Vec<Option<(i128, i128)>>,
    /// `FACTOR: Const = 0.5`: constantes decimales con nombre (solo a nivel de modulo).
    decimal_consts: HashMap<String, (i128, i128)>,
    /// Literales con unidad que ningun parametro ha usado todavia: los que sobren son un error (`PYP2230`).
    quantity_pending: Vec<(u32, Span, Dim)>,
    clock_resolved: bool,
    clock_decl: Option<(ClockKind, Span)>,
    frequency_decl: Option<(u32, Span)>,
    osc_mode_decl: Option<(String, Span)>,
    clock_settings: clock::ClockSettings,
    clock_setting_spans: BTreeMap<String, Span>,
    globals: BTreeMap<String, Symbol>,
    functions: Vec<FuncInfo>,
    fidx: HashMap<String, usize>,
    cur: Option<usize>,
    locals: HashMap<String, Symbol>,
    /// Las variables locales declaradas en la funcion actual (nombre y posicion) y los nombres que se leen: para avisar de las que no se usan.
    local_decls: Vec<(String, Span)>,
    read_names: HashSet<String>,
    global_decls: HashSet<String>,
    loop_depth: u32,
    /// Etiquetas de la funcion en curso (`GOTO`, solo BASIC).
    fn_labels: HashSet<String>,
    /// Tipos emulados ya avisados (un aviso por tipo).
    warned: HashSet<Type>,
    /// Pines importados de `pyxpic.periph.gpio`, en orden de importacion.
    pins: Vec<PinDecl>,
    pin_names: HashMap<String, usize>,
    pin_uses: Vec<PinUse>,
    declared_device: Option<String>,
    clock_hz: Option<u32>,
    /// El reloj no se declaro y se uso el del dispositivo por defecto.
    clock_defaulted: bool,
    clock_source: Option<ClockSource>,
    /// Verdadero mientras se resuelve el tipo de una constante `Const`: no cuesta nada en el dispositivo.
    in_const_decl: bool,
    /// Funciones de `pyxpic.system.time` importadas.
    time_fns: HashMap<String, DelayUnit>,
    time_uses: Vec<TimeUse>,
    /// Temporizadores importados de `pyxpic.periph.timer` y sus constantes (`INTERNAL`, `T1OSC`...).
    timer_names: HashMap<String, TimerId>,
    timer_consts: HashSet<String>,
    timer_calls: BTreeMap<u32, TimerCall>,
    /// Temporizadores con una llamada a `start` o `every_*`.
    timer_started: BTreeSet<TimerId>,
    /// Primer uso de cada temporizador que exige haberlo iniciado.
    timer_needs: BTreeMap<TimerId, Span>,
    /// `every_us`/`every_ms` pendientes de resolver cuando se conoce el reloj.
    timer_every: Vec<TimerEvery>,
    /// Pines que un temporizador reclama (`T0CKI` en RA4...).
    timer_pins: Vec<(&'static str, &'static str, Span)>,
    /// Registros importados de `pyxpic.regs`.
    reg_names: HashMap<String, RegInfo>,
    reg_calls: BTreeMap<u32, RegCall>,
    /// Pines que un periferico toma al encender un bit (por registros o por el reloj externo de un timer).
    claims: Vec<Claim>,
    /// Rutinas de runtime ya avisadas (`PYP3011`): una vez por rutina y programa.
    rt_warned: BTreeSet<String>,
    /// ADC: importado, constantes (AN0.., FOSC_8...), llamadas, si se inicio y los `start` por resolver (reloj automatico).
    adc_name: bool,
    adc_consts: HashSet<String>,
    adc_calls: BTreeMap<u32, AdcCall>,
    adc_started: bool,
    adc_needs: Option<Span>,
    adc_pending: Vec<AdcStart>,
    adc_config: adc::AdcConfig,
    /// PWM (CCP): modulos importados, llamadas, `start` por resolver (frecuencia) y todos los usos.
    pwm_names: HashSet<String>,
    /// EEPROM de datos: importada y sus llamadas.
    eeprom_name: bool,
    eeprom_calls: BTreeMap<u32, EepromCall>,
    i2c_name: bool,
    i2c_calls: BTreeMap<u32, I2cCall>,
    i2c_pending: Vec<I2cPending>,
    i2c_needs: Option<Span>,
    spi_name: bool,
    spi_calls: BTreeMap<u32, SpiCall>,
    spi_pending: Vec<SpiPending>,
    spi_needs: Option<Span>,
    /// Comparadores (`CMP1`, `CMP2`), sus constantes (`C12IN0`.., `REF_PIN`..), CVREF, las llamadas y si algun comparador pide CVREF.
    comp_names: HashSet<String>,
    comp_consts: HashSet<String>,
    cvref_name: bool,
    comp_calls: BTreeMap<u32, CompCall>,
    stack_use: Option<(u32, u32)>,
    cvref_started: bool,
    cvref_needed: Option<Span>,
    pwm_calls: BTreeMap<u32, PwmCall>,
    pwm_pending: Vec<PwmStart>,
    pwm_uses: Vec<PwmUse>,
    /// UART: importada, llamadas, si se inicio y los `start` por resolver (necesitan el reloj).
    uart_name: bool,
    uart_calls: BTreeMap<u32, UartCall>,
    uart_fmt: BTreeMap<u32, Vec<FmtPart>>,
    fixed_ops: BTreeMap<u32, FixedOp>,
    uart_started: bool,
    uart_needs: Option<Span>,
    uart_pending: Vec<UartPending>,
    /// Primer uso de una operacion de 8 bits (`write`, `read`...) y de una de 9 bits (`write9`...): no pueden mezclarse.
    uart_uses8: Option<Span>,
    uart_uses9: Option<Span>,
    /// Tablas constantes declaradas, su indice por nombre y las lecturas variables.
    roms: Vec<(String, Vec<u8>)>,
    rom_idx: HashMap<String, usize>,
    rom_gets: BTreeMap<u32, usize>,
    arrays: Vec<(String, usize)>,
    array_inits: Vec<Vec<u8>>,
    array_idx: HashMap<String, usize>,
    array_gets: BTreeMap<u32, usize>,
    text_calls: BTreeMap<u32, TextCall>,
    /// Funciones importadas de `pyxpic.bits` y sus llamadas por id de expresion.
    bits_fns: HashSet<String>,
    bits_calls: BTreeMap<u32, BitsOp>,
    /// El acarreo lo dejo definido la sentencia que acaba de terminar / la anterior a la que se comprueba.
    carry_ok: bool,
    carry_prev: bool,
    carry_fn: Option<usize>,
    /// Expresion completa de la sentencia en curso (asignacion, declaracion o expresion).
    stmt_top: Option<u32>,
}

/// Un periferico encendido que toma un pin.
struct Claim {
    pin: String,
    function: String,
    peripheral: String,
    span: Span,
    /// Viene de `TMRx.start(...)`: esos pines ya los comprueba `check_timer_usage` (PYP2125).
    via_timer: bool,
}

/// Como se pidio el periodo de un `PWMn.start(...)`: una frecuencia (se resuelve con el reloj) o prescaler y PR2 a mano.
enum PwmSpec {
    Frequency(u32),
    Raw(PwmTiming),
}

/// Una llamada a `PWMn.start(...)` por resolver.
struct PwmStart {
    id: u32,
    span: Span,
    pwm: PwmId,
    spec: Option<PwmSpec>,
    duty: u16,
    /// `duty=50pct`, en milesimas de punto porcentual: se convierte en cuentas con el periodo (PR2).
    duty_pct: Option<i128>,
    func: Option<usize>,
}

/// Un uso de un modulo PWM (`start`, `duty`, `stop`), con el ciclo de trabajo si es una constante.
struct PwmUse {
    pwm: PwmId,
    span: Span,
    duty: Option<i128>,
}

/// `CLOCK: Internal` y `FREQUENCY: 4MHz` para una frecuencia: la declaracion que se sugiere en los mensajes.
fn internal_decl(hz: u32) -> String {
    let f = if hz.is_multiple_of(1_000_000) {
        format!("{}MHz", hz / 1_000_000)
    } else if hz.is_multiple_of(1_000) {
        format!("{}kHz", hz / 1_000)
    } else {
        format!("{hz}Hz")
    };
    format!("'CLOCK: Internal' y 'FREQUENCY: {f}'")
}

/// Una llamada a `ADC.start(...)` por resolver (reloj automatico) con las condiciones que pide.
struct AdcStart {
    id: u32,
    span: Span,
    clock: Option<adc::AdcClock>,
    cfg: adc::AdcConfig,
    func: Option<usize>,
}

/// Una llamada a `I2C.start(freq=...)` por resolver (necesita el reloj del sistema).
struct I2cPending {
    id: u32,
    span: Span,
    freq: u32,
    func: Option<usize>,
}

/// Una llamada a `SPI.start(freq=..., mode=...)` por resolver (necesita el reloj del sistema).
struct SpiPending {
    id: u32,
    span: Span,
    freq: u32,
    mode: u8,
    func: Option<usize>,
}

/// Una llamada a `UART.start(baud=...)` por resolver (necesita el reloj del sistema).
struct UartPending {
    id: u32,
    span: Span,
    baud: u32,
    bits9: bool,
    func: Option<usize>,
}

struct TimerEvery {
    expr: u32,
    span: Span,
    name: &'static str,
    amount: i128,
    /// Divisor de la unidad: 1_000_000 (us) o 1_000 (ms).
    div: u128,
}

impl<'a> Checker<'a> {
    fn run(&mut self, m: &Module) {
        // las unidades de frecuencia: constantes predeclaradas (4 * MHz), igual que los tipos
        for (name, v) in [("MHz", 1_000_000), ("kHz", 1_000), ("Hz", 1)] {
            self.globals.insert(name.to_string(), Symbol { name: name.to_string(), ty: U32, kind: SymKind::Global, konst: Some(v), is_final: true });
        }
        for (i, it) in m.items.iter().enumerate() {
            if let Item::Import { span, module, names } = it {
                if module == "pyxpic.device" {
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
        self.resolve_clock_decls();
        // el reloj del perfil de proyecto: solo si el programa no declaro el suyo
        if self.clock_source.is_none() && self.clock_hz.is_none() {
            if let Some(src) = self.target.project_clock.clone() {
                self.apply_clock_source(src, Span::new(0, 0));
            }
        }
        self.define_clock_constants();
        for it in &m.items {
            if let Item::Func(f) = it {
                self.check_function(f);
            }
        }
        self.check_recursion();
        self.check_interrupt_sharing();
        self.check_stack_depth();
        self.check_pin_usage();
        self.check_timer_usage();
        self.check_pin_claims();
        self.check_adc_usage();
        self.check_comparator_usage();
        self.check_pwm_usage();
        self.check_uart_usage();
        self.check_spi_usage();
        self.check_i2c_usage();
        self.check_time_usage();
        self.check_asm_usage();
    }

    /// `from pyxpic.device import PIC16F887`: declara el dispositivo destino; debe ser la primera linea.
    fn check_device_decl(&mut self, index: usize, span: Span, names: &[Ident]) {
        if self.declared_device.is_some() {
            self.d.error("PYP2091", span, "el dispositivo ya se declaro: un programa tiene un solo 'from pyxpic.device import ...'");
            return;
        }
        let [name] = names else {
            self.d.error("PYP2092", span, "se declara un solo dispositivo, p. ej. 'from pyxpic.device import PIC16F887'");
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

    /// `CLOCK_HZ: Const[u32] = 4_000_000`: la forma manual del reloj (el oscilador lo configura el usuario); la recomendada es `CLOCK` + `FREQUENCY`.
    fn check_clock(&mut self, v: &VarDecl, ty: Type, is_final: bool, konst: Option<i128>) {
        if ty == Type::Error {
            return;
        }
        if self.clock_source.is_some() {
            self.d.error("PYP2192", v.name.span, "el reloj ya se declaro con 'Clock: ...': usa solo una de las dos formas (Clock o CLOCK_HZ)");
            return;
        }
        if !is_final || ty != U32 {
            self.d.error("PYP2093", v.name.span, "CLOCK_HZ debe declararse como 'CLOCK_HZ: Const[uint32] = 4_000_000' (frecuencia del reloj del sistema en hercios)");
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
        if module == "pyxpic.system.time" {
            for n in names {
                if let Some(u) = time_unit(&n.name) {
                    self.time_fns.insert(n.name.clone(), u);
                } else if n.name == "delay_ms" || n.name == "delay" {
                    self.d.error(
                        "PYP2104",
                        n.span,
                        format!(
                            "'{}' no existe: PyxPic tiene una sola forma por operacion. Usa 'pause' (milisegundos), 'wait' (segundos), 'delay_us' (microsegundos) o 'delay_cycles' (ciclos)",
                            n.name
                        ),
                    );
                } else if n.name == "sleep" || n.name == "nap" {
                    self.d.error("PYP2103", n.span, format!("'{}' (espera de bajo consumo con el watchdog, aproximada) todavia no esta implementada; para una espera exacta usa pause o wait", n.name));
                } else {
                    self.d.error("PYP1002", n.span, format!("'pyxpic.system.time' no exporta '{}' (exporta: delay_cycles, delay_us, pause, wait; reservadas: nap, sleep)", n.name));
                }
            }
            return;
        }
        if module == "pyxpic.clock" {
            for n in names {
                // los nombres ya estan predeclarados (no hace falta importarlos); importarlos es valido y no hace nada
                if !matches!(n.name.as_str(), "Internal" | "Crystal" | "MHz" | "kHz" | "Hz") {
                    self.d.error("PYP2190", n.span, format!("'pyxpic.clock' no exporta '{}' (exporta: Internal, Crystal, MHz, kHz, Hz)", n.name));
                }
            }
            return;
        }
        if module == "pyxpic.bits" {
            for n in names {
                if bits::FUNCTIONS.contains(&n.name.as_str()) {
                    self.bits_fns.insert(n.name.clone());
                } else {
                    self.d.error("PYP2140", n.span, format!("'pyxpic.bits' no exporta '{}' (exporta: {})", n.name, bits::FUNCTIONS.join(", ")));
                }
            }
            return;
        }
        if module == "pyxpic.periph.uart" {
            for n in names {
                if n.name == "UART" {
                    if let Some(set) = &self.target.peripherals {
                        if !set.contains("EUSART") {
                            self.d.error("PYP2180", n.span, format!("'{}' no tiene UART: el dispositivo no trae el EUSART", self.target.name));
                            continue;
                        }
                    }
                    self.uart_name = true;
                } else {
                    self.d.error("PYP2180", n.span, format!("'pyxpic.periph.uart' no exporta '{}' (exporta: UART)", n.name));
                }
            }
            return;
        }
        if module == "pyxpic.periph.adc" {
            for n in names {
                if n.name == "ADC" {
                    if let Some(set) = &self.target.peripherals {
                        if !set.contains("ADC") {
                            self.d.error("PYP2170", n.span, format!("'{}' no tiene ADC: el dispositivo no lo trae", self.target.name));
                            continue;
                        }
                    }
                    self.adc_name = true;
                } else if let Some(ch) = adc::channel(&n.name) {
                    // el canal existe si algun pin del dispositivo tiene esa funcion (ANSn lo reclama)
                    if self.target.pin_claims.as_ref().is_some_and(|c| !c.contains_key(&format!("ANS{ch}"))) {
                        self.d.error("PYP2170", n.span, format!("'{}' no tiene el canal {}: ningun pin del dispositivo es AN{ch}", self.target.name, n.name));
                        continue;
                    }
                    self.adc_consts.insert(n.name.clone());
                } else if adc::CLOCKS.iter().any(|(c, _)| *c == n.name) {
                    self.adc_consts.insert(n.name.clone());
                } else {
                    let all: Vec<String> = std::iter::once("ADC".to_string()).chain((0..adc::CHANNELS).map(|c| format!("AN{c}"))).chain(adc::CLOCKS.iter().map(|(c, _)| c.to_string())).collect();
                    self.d.error("PYP2170", n.span, format!("'pyxpic.periph.adc' no exporta '{}' (exporta: {})", n.name, all.join(", ")));
                }
            }
            return;
        }
        if module == "pyxpic.periph.comparator" {
            for n in names {
                let has = |this: &Self, p: &str| this.target.peripherals.as_ref().is_none_or(|set| set.contains(p));
                if CmpId::from_name(&n.name).is_some() {
                    if !has(self, "COMPARATORS") {
                        self.d.error("PYP2214", n.span, format!("'{}' no tiene comparadores: el dispositivo no trae el modulo COMPARATORS", self.target.name));
                        continue;
                    }
                    self.comp_names.insert(n.name.clone());
                } else if let Some(&(_, an)) = comparator::NEG_INPUTS.iter().find(|(c, _)| *c == n.name) {
                    if self.target.pin_claims.as_ref().is_some_and(|c| !c.contains_key(&format!("ANS{an}"))) {
                        self.d.error("PYP2214", n.span, format!("'{}' no tiene la entrada {}: ningun pin del dispositivo es AN{an}", self.target.name, n.name));
                        continue;
                    }
                    self.comp_consts.insert(n.name.clone());
                } else if comparator::REFS.iter().any(|(c, _)| *c == n.name) {
                    self.comp_consts.insert(n.name.clone());
                } else {
                    let all: Vec<String> = ["CMP1", "CMP2"]
                        .iter()
                        .map(|s| s.to_string())
                        .chain(comparator::NEG_INPUTS.iter().map(|(c, _)| c.to_string()))
                        .chain(comparator::REFS.iter().map(|(c, _)| c.to_string()))
                        .collect();
                    self.d.error("PYP2214", n.span, format!("'pyxpic.periph.comparator' no exporta '{}' (exporta: {})", n.name, all.join(", ")));
                }
            }
            return;
        }
        if module == "pyxpic.periph.vref" {
            for n in names {
                if n.name == "CVREF" {
                    if let Some(set) = &self.target.peripherals {
                        if !set.contains("VREF") {
                            self.d.error("PYP2220", n.span, format!("'{}' no tiene CVREF: el dispositivo no trae el modulo VREF", self.target.name));
                            continue;
                        }
                    }
                    self.cvref_name = true;
                } else {
                    self.d.error("PYP2220", n.span, format!("'pyxpic.periph.vref' no exporta '{}' (exporta: CVREF)", n.name));
                }
            }
            return;
        }
        if module == "pyxpic.periph.i2c" {
            for n in names {
                if n.name == "I2C" {
                    if let Some(set) = &self.target.peripherals {
                        if !set.contains("MSSP") {
                            self.d.error("PYP2224", n.span, format!("'{}' no tiene I2C: el dispositivo no trae el modulo MSSP", self.target.name));
                            continue;
                        }
                    }
                    self.i2c_name = true;
                } else {
                    self.d.error("PYP2224", n.span, format!("'pyxpic.periph.i2c' no exporta '{}' (exporta: I2C)", n.name));
                }
            }
            return;
        }
        if module == "pyxpic.periph.spi" {
            for n in names {
                if n.name == "SPI" {
                    if let Some(set) = &self.target.peripherals {
                        if !set.contains("MSSP") {
                            self.d.error("PYP2233", n.span, format!("'{}' no tiene SPI: el dispositivo no trae el modulo MSSP", self.target.name));
                            continue;
                        }
                    }
                    self.spi_name = true;
                } else {
                    self.d.error("PYP2233", n.span, format!("'pyxpic.periph.spi' no exporta '{}' (exporta: SPI)", n.name));
                }
            }
            return;
        }
        if module == "pyxpic.periph.eeprom" {
            for n in names {
                if n.name == "EEPROM" {
                    if let Some(set) = &self.target.peripherals {
                        if !set.contains("EEPROM") {
                            self.d.error("PYP2210", n.span, format!("'{}' no tiene EEPROM de datos: el dispositivo no trae el modulo EEPROM", self.target.name));
                            continue;
                        }
                    }
                    self.eeprom_name = true;
                } else {
                    self.d.error("PYP2210", n.span, format!("'pyxpic.periph.eeprom' no exporta '{}' (exporta: EEPROM)", n.name));
                }
            }
            return;
        }
        if module == "pyxpic.periph.pwm" {
            for n in names {
                match PwmId::from_name(&n.name) {
                    Some(id) => {
                        if let Some(set) = &self.target.peripherals {
                            if !set.contains(id.peripheral()) {
                                self.d.error("PYP2200", n.span, format!("'{}' no tiene {}: el dispositivo no trae el modulo {}", self.target.name, id.name(), id.peripheral()));
                                continue;
                            }
                        }
                        self.pwm_names.insert(n.name.clone());
                    }
                    None => self.d.error("PYP2200", n.span, format!("'pyxpic.periph.pwm' no exporta '{}' (exporta: PWM1, PWM2)", n.name)),
                }
            }
            return;
        }
        if module == "pyxpic.regs" {
            for n in names {
                let Some(regs) = &self.target.registers else {
                    self.d.error("PYP2130", n.span, "'pyxpic.regs' necesita un dispositivo: declara 'from pyxpic.device import PIC16F887'");
                    continue;
                };
                match regs.get(&n.name) {
                    None => {
                        let all: Vec<&str> = regs.iter().filter(|(_, r)| r.physical && !r.shared).map(|(k, _)| k.as_str()).collect();
                        self.d.error("PYP2130", n.span, format!("'{}' no tiene el registro '{}' (registros: {})", self.target.name, n.name, all.join(", ")));
                    }
                    Some(r) if !r.physical => self.d.error("PYP2130", n.span, format!("'{}' no es un registro fisico: no se lee ni se escribe como un byte", n.name)),
                    Some(r) if r.shared => {
                        self.d.error("PYP2130", n.span, format!("'{}' comparte direccion con otro registro y depende de otros bits para saber cual se toca; usa @asm_function", n.name))
                    }
                    Some(r) => {
                        self.reg_names.insert(n.name.clone(), r.clone());
                    }
                }
            }
            return;
        }
        if module == "pyxpic.periph.timer" {
            for n in names {
                if let Some(t) = TimerId::from_name(&n.name) {
                    if let Some(set) = &self.target.peripherals {
                        if !set.contains(t.peripheral()) {
                            self.d.error("PYP2120", n.span, format!("'{}' no tiene {} ({}): el dispositivo no lo trae", self.target.name, n.name, t.peripheral()));
                            continue;
                        }
                    }
                    self.timer_names.insert(n.name.clone(), t);
                } else if timer::constant(&n.name).is_some() {
                    self.timer_consts.insert(n.name.clone());
                } else {
                    let all: Vec<&str> = ["TMR0", "TMR1", "TMR2"].into_iter().chain(timer::CONSTANTS.iter().map(|(n, _)| *n)).collect();
                    self.d.error("PYP2120", n.span, format!("'pyxpic.periph.timer' no exporta '{}' (exporta: {})", n.name, all.join(", ")));
                }
            }
            return;
        }
        if module == "pyxpic.periph.gpio" {
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
                            Some(map) => {
                                format!("'pyxpic.periph.gpio' no exporta '{}' en '{}' (pines de E/S: {})", n.name, self.target.name, map.keys().map(String::as_str).collect::<Vec<_>>().join(", "))
                            }
                            None => format!("'{}' no es un nombre de pin valido (se espera algo como RD0)", n.name),
                        };
                        self.d.error("PYP2074", n.span, msg);
                    }
                }
            }
            return;
        }
        let ok = module == "pyxpic" && names.iter().all(|n| PREDECLARED.contains(&n.name.as_str()));
        if !ok {
            self.d.error(
                "PYP1002",
                span,
                format!("import de '{module}' no soportado; modulos disponibles: 'pyxpic' (tipos), 'pyxpic.device', y por categoria 'pyxpic.periph.*' (hardware), 'pyxpic.system.*' (CPU, reloj, energia) y 'pyxpic.soft.*' (software)"),
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
        if self.dim(a).is_some() {
            match unit {
                DelayUnit::Cycles => {
                    self.consume_quantities(a);
                    self.dims[a.id as usize] = None;
                    self.d.error("PYP2240", a.span, format!("{func}() recibe ciclos de instruccion (un numero); para un tiempo usa delay_us, pause o wait"));
                }
                DelayUnit::Us => self.unit_arg(a, Dim::Time, 1_000, "us", func),
                DelayUnit::Ms => self.unit_arg(a, Dim::Time, 1_000_000, "ms", func),
                DelayUnit::S => self.unit_arg(a, Dim::Time, 1_000_000_000, "s", func),
            }
        }
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
                _ => {
                    self.d.error("PYP2100", a.span, format!("{func}() necesita una constante: el tiempo se calcula al compilar, al ciclo; para una cantidad calculada usa pause o wait con un uint16"))
                }
            },
            DelayUnit::Ms | DelayUnit::S => match t {
                Type::Error => {}
                Type::Literal => {
                    self.coerce_literal(a, U16);
                }
                t if t == U16 => {}
                t => self.d.error("PYP2041", a.span, format!("{func}() recibe un uint16 o una constante, no {t}")),
            },
        }
        if let Some(cur) = self.cur {
            self.time_uses.push(TimeUse { func: cur, span: e.span, name: func.to_string(), unit, amount: konst });
        }
        self.set(e, Type::Void, None)
    }

    /// Reglas globales de las esperas: necesitan el reloj (Clock o CLOCK_HZ), no van en la interrupcion y avisan si el
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
                        format!("no se declaro el reloj: se usa el reloj por defecto de {} ({def} Hz); declara {} para fijarlo y quitar este aviso", self.target.name, internal_decl(def)),
                    );
                    self.clock_hz = Some(def);
                    self.clock_defaulted = true;
                    def
                }
                None => {
                    self.d.error(
                        "PYP2101",
                        first.span,
                        format!(
                            "{} necesita el reloj del sistema y no hay dispositivo declarado con reloj por defecto: declara 'CLOCK: Internal' y 'FREQUENCY: 4MHz' (o 'CLOCK: Crystal' y 'FREQUENCY: 16MHz')",
                            first.name
                        ),
                    );
                    return;
                }
            },
        };
        // con `Clock: ...` el compilador si configura el oscilador (FOSC y OSCCON): el aviso solo vale para CLOCK_HZ a secas
        if let (false, Some(def), None) = (self.clock_defaulted, self.target.default_clock_hz, &self.clock_source) {
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
                    if !num.is_multiple_of(den) {
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
                None if !(hz as u128).is_multiple_of(den) && warned_var.insert(if u.unit == DelayUnit::Ms { "ms" } else { "s" }) => {
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

    /// Las funciones de ensamblador comparten memoria estatica con el contexto principal: no se llaman desde la interrupcion.
    fn check_asm_usage(&mut self) {
        let asm_fns: Vec<usize> = self.functions.iter().enumerate().filter(|(_, f)| f.is_asm).map(|(i, _)| i).collect();
        if asm_fns.is_empty() {
            return;
        }
        let mut reach: BTreeSet<String> = BTreeSet::new();
        let mut stack: Vec<String> = self.functions.iter().filter(|f| f.is_interrupt).map(|f| f.name.clone()).collect();
        while let Some(n) = stack.pop() {
            if let Some(&i) = self.fidx.get(&n) {
                if reach.insert(n) {
                    stack.extend(self.functions[i].calls.iter().cloned());
                }
            }
        }
        for i in asm_fns {
            let f = &self.functions[i];
            if reach.contains(&f.name) {
                self.d.error(
                    "PYP2111",
                    f.span,
                    format!("la funcion @asm_function '{}' se usa desde la interrupcion: por ahora solo se llama desde el contexto principal (comparte memoria estatica)", f.name),
                );
            }
        }
    }

    fn pin_info(&self, name: &str) -> Option<PinInfo> {
        match &self.target.pins {
            Some(map) => map.get(name).cloned(),
            None => parse_pin_name(name).map(|(port, bit)| PinInfo { port, bit, input_only: false, analog: None }),
        }
    }

    // ------------------------------------------------------ temporizadores
    /// Evalua un argumento con nombre que debe ser una constante entera, booleana o de `pyxpic.periph.timer`.
    fn timer_const(&mut self, k: &Kwarg) -> Option<i128> {
        if let ExprKind::Name(n) = &k.value.kind {
            if self.timer_consts.contains(n) {
                let v = timer::constant(n);
                self.set(&k.value, Type::Literal, v);
                return v;
            }
        }
        let t = self.expr(&k.value);
        if t == Type::Error {
            return None;
        }
        match self.konst(&k.value) {
            Some(v) => Some(v),
            None => {
                self.d.error("PYP2122", k.value.span, format!("el argumento '{}' debe ser una constante: se escribe en un registro al compilar", k.name));
                None
            }
        }
    }

    fn timer_start(&mut self, e: &Expr, tid: TimerId, kwargs: &[Kwarg]) -> Option<TimerStart> {
        let allowed: &[&str] = match tid {
            TimerId::T0 => &["prescaler", "source", "edge"],
            TimerId::T1 => &["prescaler", "source", "sync", "gate"],
            TimerId::T2 => &["prescaler", "period", "postscaler"],
        };
        let name = tid.name();
        let mut vals: HashMap<&str, (i128, Span)> = HashMap::new();
        let mut ok = true;
        for k in kwargs {
            let v = self.timer_const(k);
            if !allowed.contains(&k.name.as_str()) {
                self.d.error("PYP2122", k.name_span, format!("{name}.start no tiene el argumento '{}' (admite: {})", k.name, allowed.join(", ")));
                ok = false;
            } else if let Some(v) = v {
                vals.insert(allowed.iter().find(|a| **a == k.name).unwrap(), (v, k.value.span));
            } else {
                ok = false;
            }
        }
        let bad = |this: &mut Self, span: Span, msg: String| {
            this.d.error("PYP2122", span, msg);
            false
        };
        let mut st = TimerStart::new();
        match vals.get("prescaler") {
            Some(&(p, sp)) => {
                let valid: &[i128] = match tid {
                    TimerId::T0 => &[1, 2, 4, 8, 16, 32, 64, 128, 256],
                    TimerId::T1 => &[1, 2, 4, 8],
                    TimerId::T2 => &[1, 4, 16],
                };
                if valid.contains(&p) {
                    st.prescaler = p as u32;
                } else {
                    let list = valid.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ");
                    ok = bad(self, sp, format!("{name}: prescaler={p} no existe; el hardware admite {list}")) && ok;
                }
            }
            None if tid == TimerId::T1 => {}
            None => ok = bad(self, e.span, format!("{name}.start necesita prescaler=...")) && ok,
        }
        match tid {
            TimerId::T0 => {
                match vals.get("source").map(|x| x.0) {
                    None | Some(0) => {}
                    Some(1) => st.source = TimerSource::T0cki,
                    Some(_) => ok = bad(self, vals["source"].1, "TMR0: source debe ser INTERNAL o T0CKI".into()) && ok,
                }
                if let Some(&(edge, sp)) = vals.get("edge") {
                    if st.source != TimerSource::T0cki {
                        ok = bad(self, sp, "TMR0: edge solo tiene sentido con source=T0CKI".into()) && ok;
                    } else if edge == 5 {
                        st.falling = true;
                    } else if edge != 4 {
                        ok = bad(self, sp, "TMR0: edge debe ser RISING o FALLING".into()) && ok;
                    }
                }
            }
            TimerId::T1 => {
                match vals.get("source").map(|x| x.0) {
                    None | Some(0) => {}
                    Some(2) => st.source = TimerSource::T1cki,
                    Some(3) => st.source = TimerSource::T1osc,
                    Some(_) => ok = bad(self, vals["source"].1, "TMR1: source debe ser INTERNAL, T1CKI o T1OSC".into()) && ok,
                }
                if let Some(&(s, sp)) = vals.get("sync") {
                    if st.source == TimerSource::Internal {
                        ok = bad(self, sp, "TMR1: sync solo tiene sentido con un reloj externo (source=T1CKI o T1OSC)".into()) && ok;
                    } else if s == 0 || s == 1 {
                        st.sync = s == 1;
                    } else {
                        ok = bad(self, sp, "TMR1: sync es True o False".into()) && ok;
                    }
                }
                if let Some(&(g, sp)) = vals.get("gate") {
                    st.gate = match g {
                        6 => Some(Gate { source: GateSource::T1g, active_high: true }),
                        7 => Some(Gate { source: GateSource::T1g, active_high: false }),
                        8 => Some(Gate { source: GateSource::C2out, active_high: true }),
                        9 => Some(Gate { source: GateSource::C2out, active_high: false }),
                        _ => {
                            ok = bad(self, sp, "TMR1: gate debe ser T1G_HIGH, T1G_LOW, C2_HIGH o C2_LOW".into()) && ok;
                            None
                        }
                    };
                }
            }
            TimerId::T2 => {
                match vals.get("period") {
                    Some(&(p, _)) if (0..=255).contains(&p) => st.period = p as u32,
                    Some(&(p, sp)) => ok = bad(self, sp, format!("TMR2: period={p} fuera de 0..=255 (es el valor de PR2)")) && ok,
                    None => ok = bad(self, e.span, "TMR2.start necesita period=... (el valor de PR2)".into()) && ok,
                }
                match vals.get("postscaler") {
                    None => {}
                    Some(&(p, _)) if (1..=16).contains(&p) => st.postscaler = p as u32,
                    Some(&(p, sp)) => ok = bad(self, sp, format!("TMR2: postscaler={p} fuera de 1..=16")) && ok,
                }
            }
        }
        ok.then_some(st)
    }

    fn timer_method(&mut self, e: &Expr, tid: TimerId, method: &str, args: &[Expr], kwargs: &[Kwarg]) -> Type {
        let name = tid.name();
        let ret_ty = |t: TimerId| if t == TimerId::T1 { U16 } else { U8 };
        let void = |this: &mut Self| this.set(e, Type::Void, None);
        let note = |this: &mut Self, started: bool| {
            if started {
                this.timer_started.insert(tid);
            } else {
                this.timer_needs.entry(tid).or_insert(e.span);
            }
        };
        // los argumentos con nombre solo los admite start
        if method != "start" {
            if let Some(k) = kwargs.first() {
                self.d.error("PYP2122", k.name_span, format!("{name}.{method} no acepta argumentos con nombre"));
                for k in kwargs {
                    self.expr(&k.value);
                }
            }
        }
        let arity = |this: &mut Self, n: usize| -> bool {
            if args.len() != n {
                this.d.error("PYP2055", e.span, format!("'{name}.{method}' recibe {n} argumento(s)"));
                for a in args {
                    this.expr(a);
                }
                return false;
            }
            true
        };
        match method {
            "every" => {
                for a in args {
                    self.expr(a);
                }
                if tid != TimerId::T2 {
                    self.d.error("PYP2122", e.span, format!("{name}.every no existe: solo Timer2 tiene un periodo automatico (PR2); usa {name}.start(prescaler=...)"));
                    return void(self);
                }
                if args.len() != 1 {
                    self.d.error("PYP2055", e.span, "'TMR2.every' recibe un tiempo: TMR2.every(20ms)");
                    return void(self);
                }
                let a = &args[0];
                if self.dim(a) != Some(Dim::Time) {
                    self.consume_quantities(a);
                    self.d.error("PYP2240", a.span, "TMR2.every recibe un tiempo con unidad: TMR2.every(20ms)");
                    return void(self);
                }
                self.consume_quantities(a);
                self.dims[a.id as usize] = None;
                let (Some(ns), Some(hz)) = (self.konst(a), self.clock_hz.or(self.target.default_clock_hz)) else {
                    self.d.error("PYP2243", e.span, "TMR2.every necesita el reloj para convertir el tiempo en ciclos: declara 'CLOCK: Internal' y 'FREQUENCY: 4MHz'");
                    return void(self);
                };
                // ciclos = tiempo * FOSC / (relojes por ciclo * 1e9), exacto o error
                let per = self.target.clocks_per_cycle.unwrap_or(4) as i128;
                let (num, den) = (ns * hz as i128, per * 1_000_000_000);
                if num % den != 0 {
                    self.d.error("PYP2243", a.span, format!("TMR2.every: ese tiempo no cae en un numero entero de ciclos de instruccion a {} (un ciclo dura {} ns); el periodo de Timer2 es un multiplo exacto del ciclo", clock::fmt_hz(hz), per as f64 * 1e9 / hz as f64));
                    return void(self);
                }
                let cycles = (num / den) as u64;
                match timer::solve_t2(cycles) {
                    Some(st) => {
                        self.timer_calls.insert(e.id, TimerCall { timer: tid, op: TimerOp::Start(st) });
                        note(self, true);
                    }
                    None => {
                        let (below, above) = timer::nearest_t2(cycles);
                        let near: Vec<String> = [below, above].iter().flatten().map(|c| format!("{c} ciclos")).collect();
                        self.d.error("PYP2243", a.span, format!("TMR2.every: {cycles} ciclos no se pueden formar con prescaler (1, 4, 16) x PR2+1 (1 a 256) x postscaler (1 a 16); los mas cercanos: {}", near.join(" y ")));
                    }
                }
                void(self)
            }
            "start" => {
                if !args.is_empty() {
                    self.d.error("PYP2127", args[0].span, format!("{name}.start se escribe con argumentos con nombre, p. ej. {name}.start(prescaler=...)"));
                    for a in args {
                        self.expr(a);
                    }
                }
                if let Some(st) = self.timer_start(e, tid, kwargs) {
                    self.timer_calls.insert(e.id, TimerCall { timer: tid, op: TimerOp::Start(st) });
                    let (src, pin) = match (tid, st.source) {
                        (TimerId::T0, TimerSource::T0cki) => ("T0CKI", Some("RA4")),
                        (TimerId::T1, TimerSource::T1cki) => ("T1CKI", Some("RC0")),
                        (TimerId::T1, TimerSource::T1osc) => ("T1OSC", Some("RC0")),
                        _ => ("", None),
                    };
                    let per = if tid == TimerId::T0 { "TIMER0" } else { "TIMER1" };
                    if let Some(pin) = pin {
                        self.timer_pins.push((src, pin, e.span));
                        self.claims.push(Claim { pin: pin.into(), function: src.into(), peripheral: per.into(), span: e.span, via_timer: true });
                        if src == "T1OSC" {
                            self.timer_pins.push((src, "RC1", e.span));
                            self.claims.push(Claim { pin: "RC1".into(), function: src.into(), peripheral: per.into(), span: e.span, via_timer: true });
                        }
                    }
                    if matches!(st.gate, Some(Gate { source: GateSource::T1g, .. })) {
                        self.timer_pins.push(("T1G", "RB5", e.span));
                        self.claims.push(Claim { pin: "RB5".into(), function: "T1G".into(), peripheral: per.into(), span: e.span, via_timer: true });
                    }
                }
                note(self, true);
                void(self)
            }
            "every_us" | "every_ms" => {
                if tid != TimerId::T2 {
                    self.d.error("PYP2123", e.span, format!("{name}.{method}: solo Timer2 tiene registro de periodo (PR2); con {name} usa start(...) y set(...) para precargar la cuenta"));
                    for a in args {
                        self.expr(a);
                    }
                    return void(self);
                }
                if !arity(self, 1) {
                    return void(self);
                }
                let a = &args[0];
                let t = self.expr(a);
                match (t, self.konst(a)) {
                    (Type::Error, _) => {}
                    (t, Some(v)) if matches!(t, Type::Literal | Type::Int { .. }) && v > 0 && v <= u32::MAX as i128 => {
                        let (n, div) = if method == "every_us" { ("every_us", 1_000_000) } else { ("every_ms", 1_000) };
                        self.timer_every.push(TimerEvery { expr: e.id, span: e.span, name: n, amount: v, div });
                        if t == Type::Literal {
                            self.coerce_literal(a, U32);
                        }
                    }
                    _ => self.d.error("PYP2122", a.span, format!("{name}.{method} necesita una constante mayor que 0: el periodo se resuelve al compilar, a prescaler/PR2/postscaler exactos")),
                }
                note(self, true);
                void(self)
            }
            "stop" => {
                if tid == TimerId::T0 {
                    self.d.error("PYP2123", e.span, "TMR0 no se puede detener: cuenta mientras haya reloj. Para dejar de usarlo cambia a un reloj externo parado o ignora la bandera");
                    return void(self);
                }
                if arity(self, 0) {
                    self.timer_calls.insert(e.id, TimerCall { timer: tid, op: TimerOp::Stop });
                }
                note(self, false);
                void(self)
            }
            "value" => {
                if arity(self, 0) {
                    self.timer_calls.insert(e.id, TimerCall { timer: tid, op: TimerOp::Value });
                }
                note(self, false);
                self.set(e, ret_ty(tid), None)
            }
            "set" => {
                if arity(self, 1) {
                    self.expect(&args[0], ret_ty(tid), Some(&format!("{name}.set recibe un {}", ret_ty(tid))));
                    self.timer_calls.insert(e.id, TimerCall { timer: tid, op: TimerOp::Set });
                }
                note(self, false);
                void(self)
            }
            "expired" => {
                if arity(self, 0) {
                    self.timer_calls.insert(e.id, TimerCall { timer: tid, op: TimerOp::Expired });
                }
                note(self, false);
                self.set(e, Type::Bool, None)
            }
            "clear" | "interrupt_on" | "interrupt_off" => {
                if arity(self, 0) {
                    let op = match method {
                        "clear" => TimerOp::Clear,
                        "interrupt_on" => TimerOp::InterruptOn,
                        _ => TimerOp::InterruptOff,
                    };
                    self.timer_calls.insert(e.id, TimerCall { timer: tid, op });
                }
                note(self, false);
                void(self)
            }
            other => {
                self.d.error(
                    "PYP2124",
                    e.span,
                    format!("un temporizador no tiene el metodo '{other}' (disponibles: start, every_us, every_ms, stop, value, set, expired, clear, interrupt_on, interrupt_off)"),
                );
                for a in args {
                    self.expr(a);
                }
                self.set(e, Type::Error, None)
            }
        }
    }

    /// Reloj efectivo para resolver tiempos: el declarado o, con aviso, el del dispositivo.
    fn effective_clock(&mut self, span: Span, who: &str) -> Option<u32> {
        if let Some(hz) = self.clock_hz {
            return Some(hz);
        }
        match self.target.default_clock_hz {
            Some(def) => {
                self.d.warning(
                    "PYP3004",
                    span,
                    format!("no se declaro el reloj: se usa el reloj por defecto de {} ({def} Hz); declara {} para fijarlo y quitar este aviso", self.target.name, internal_decl(def)),
                );
                self.clock_hz = Some(def);
                self.clock_defaulted = true;
                Some(def)
            }
            None => {
                self.d.error(
                    "PYP2101",
                    span,
                    format!("{who} necesita el reloj del sistema y no hay dispositivo declarado con reloj por defecto: declara 'CLOCK: Internal' y 'FREQUENCY: 4MHz' (o 'CLOCK: Crystal' y 'FREQUENCY: 16MHz')"),
                );
                None
            }
        }
    }

    fn note_claims(&mut self, bit: &str, span: Span) {
        let Some(list) = self.target.pin_claims.as_ref().and_then(|m| m.get(bit)).cloned() else { return };
        for c in list {
            for pin in &c.pins {
                self.claims.push(Claim { pin: pin.clone(), function: c.function.clone(), peripheral: c.peripheral.clone(), span, via_timer: false });
            }
        }
    }

    /// Pines que toma un periferico encendido: no pueden ser E/S del programa (PYP2150) ni de dos perifericos a la vez (PYP3010).
    fn check_pin_claims(&mut self) {
        let claims = std::mem::take(&mut self.claims);
        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
        for c in &claims {
            if !c.via_timer && self.pin_names.contains_key(&c.pin) && seen.insert((c.pin.clone(), "gpio".into())) {
                self.d.error(
                    "PYP2150",
                    c.span,
                    format!(
                        "el pin {0} es la funcion {1} de {2}, que este programa enciende aqui, y tambien se importa como pin de E/S ({0}.output()...): un pin no puede ser las dos cosas a la vez",
                        c.pin, c.function, c.peripheral
                    ),
                );
            }
        }
        // dos perifericos distintos sobre el mismo pin
        let mut by_pin: BTreeMap<&str, Vec<&Claim>> = BTreeMap::new();
        for c in &claims {
            by_pin.entry(c.pin.as_str()).or_default().push(c);
        }
        for (pin, list) in by_pin {
            let first = list[0];
            if let Some(other) = list.iter().find(|c| c.peripheral != first.peripheral) {
                self.d.warning(
                    "PYP3010",
                    other.span,
                    format!("el pin {pin} lo reclaman dos perifericos: {} ({}) y {} ({}); solo uno puede usarlo", first.peripheral, first.function, other.peripheral, other.function),
                );
            }
        }
    }

    /// Reglas globales de los temporizadores: iniciarlos, pines que reclaman y periodos `every_*`.
    fn check_timer_usage(&mut self) {
        for (t, sp) in std::mem::take(&mut self.timer_needs) {
            if !self.timer_started.contains(&t) {
                self.d.error("PYP2121", sp, format!("{0} se usa pero nunca se inicia: llama a {0}.start(...) (o {0}.every_ms(...)) en algun punto del programa", t.name()));
            }
        }
        for (func, pin, sp) in std::mem::take(&mut self.timer_pins) {
            if self.pin_names.contains_key(pin) {
                self.d.error("PYP2125", sp, format!("el pin {pin} es la entrada {func} del temporizador y tambien se importa como pin de E/S: un pin no puede ser las dos cosas a la vez"));
            }
        }
        let pend = std::mem::take(&mut self.timer_every);
        let Some(first) = pend.first() else { return };
        let Some(hz) = self.effective_clock(first.span, first.name) else { return };
        let Some(cpc) = self.target.clocks_per_cycle else {
            self.d.error("PYP2101", first.span, format!("{} necesita un dispositivo declarado: los ciclos de instruccion dependen del dispositivo", first.name));
            return;
        };
        for p in pend {
            let den = cpc as u128 * p.div;
            let num = p.amount as u128 * hz as u128;
            if !num.is_multiple_of(den) {
                self.d.error(
                    "PYP2126",
                    p.span,
                    format!(
                        "{}({}) no cae en un numero entero de ciclos de instruccion a {hz} Hz (un ciclo dura {:.1} ns): el periodo de Timer2 es un multiplo exacto del ciclo",
                        p.name,
                        p.amount,
                        cpc as f64 * 1e9 / hz as f64
                    ),
                );
                continue;
            }
            let cycles = (num / den) as u64;
            match timer::solve_t2(cycles) {
                Some(st) => {
                    self.timer_calls.insert(p.expr, TimerCall { timer: TimerId::T2, op: TimerOp::Start(st) });
                }
                None => {
                    let (lo, hi) = timer::nearest_t2(cycles);
                    let near = match (lo, hi) {
                        (Some(l), Some(h)) => format!("los periodos exactos mas cercanos son {l} y {h} ciclos"),
                        (Some(l), None) => format!("el periodo exacto mas largo es {l} ciclos"),
                        (None, Some(h)) => format!("el periodo exacto mas corto es {h} ciclos"),
                        (None, None) => String::new(),
                    };
                    self.d.error(
                        "PYP2126",
                        p.span,
                        format!("{}({}) son {cycles} ciclos y Timer2 no puede dar ese periodo exacto (prescaler 1/4/16 x PR2+1 de 1 a 256 x postscaler 1 a 16 = {} ciclos como maximo); {near}; usa TMR2.start(...) con otros valores", p.name, p.amount, timer::T2_MAX_CYCLES),
                    );
                }
            }
        }
    }

    /// `REG.read()`, `REG.write(v)` y `REG.BIT.set()/clear()/toggle()/value()`.
    fn reg_method(&mut self, e: &Expr, obj: &str, attr: Option<&Ident>, method: &str, args: &[Expr], kwargs: &[Kwarg]) -> Type {
        let info = self.reg_names[obj].clone();
        if let Some(k) = kwargs.first() {
            self.d.error("PYP2132", k.name_span, format!("los metodos de un registro no aceptan argumentos con nombre ('{}')", k.name));
            for k in kwargs {
                self.expr(&k.value);
            }
        }
        let fail = |this: &mut Self, args: &[Expr]| {
            for a in args {
                this.expr(a);
            }
            this.set(e, Type::Error, None)
        };
        let (bit, label) = match attr {
            Some(b) => match info.bits.iter().position(|x| x.as_deref() == Some(b.name.as_str())) {
                Some(i) => (Some(i as u8), format!("{obj}.{}", b.name)),
                None => {
                    let names: Vec<&str> = info.bits.iter().flatten().map(|s| s.as_str()).collect();
                    let list = if names.is_empty() { "no tiene bits con nombre".to_string() } else { format!("bits: {}", names.join(", ")) };
                    self.d.error("PYP2133", b.span, format!("{obj} no tiene el bit '{}' ({list})", b.name));
                    return fail(self, args);
                }
            },
            None => (None, obj.to_string()),
        };
        let (want_args, op) = match (bit.is_some(), method) {
            (false, "read") => (0, RegOp::Read),
            (false, "write") => (1, RegOp::Write),
            (true, "set") => (0, RegOp::Set),
            (true, "clear") => (0, RegOp::Clear),
            (true, "toggle") => (0, RegOp::Toggle),
            (true, "value") => (0, RegOp::Value),
            (false, m) => {
                self.d.error("PYP2134", e.span, format!("el registro {obj} no tiene el metodo '{m}' (disponibles: read, write; un bit se usa asi: {obj}.BIT.set())"));
                return fail(self, args);
            }
            (true, m) => {
                self.d.error("PYP2134", e.span, format!("el bit {label} no tiene el metodo '{m}' (disponibles: set, clear, toggle, value)"));
                return fail(self, args);
            }
        };
        if args.len() != want_args {
            self.d.error("PYP2055", e.span, format!("'{label}.{method}' recibe {want_args} argumento(s)"));
            return fail(self, args);
        }
        if op == RegOp::Write && WRITE_PROTECTED.contains(&obj) {
            self.d.error(
                "PYP2135",
                e.span,
                format!("{obj} no se escribe entero: lo usa el compilador (banco, pagina, PC). Para sus banderas usa {obj}.C, {obj}.Z, {obj}.DC; para algo mas, @asm_function"),
            );
            return fail(self, args);
        }
        if matches!(op, RegOp::Set | RegOp::Clear | RegOp::Toggle) {
            if let Some(b) = attr.filter(|b| BANK_BITS.contains(&b.name.as_str())) {
                self.d.error(
                    "PYP2135",
                    e.span,
                    format!("{obj}.{} elige el banco de datos y lo gestiona el compilador: el codigo del programa corre en el banco 0. Para cambiarlo a mano usa @asm_function", b.name),
                );
                return fail(self, args);
            }
        }
        if op == RegOp::Write {
            self.expect(&args[0], U8, Some(&format!("{obj}.write recibe un uint8")));
        }
        // encender un bit puede hacer que un periferico tome un pin: se anota para PYP2150/PYP3010
        match (op, attr, self.konst(args.first().unwrap_or(e))) {
            (RegOp::Set, Some(b), _) => self.note_claims(&b.name, e.span),
            (RegOp::Write, _, Some(v)) => {
                for (i, name) in info.bits.iter().enumerate() {
                    if let (true, Some(n)) = (v >> i & 1 == 1, name) {
                        self.note_claims(n, e.span);
                    }
                }
            }
            _ => {}
        }
        self.reg_calls.insert(e.id, RegCall { reg: obj.to_string(), addr: info.addr, bit, op });
        match op {
            RegOp::Read | RegOp::Value => self.set(e, U8, None),
            _ => self.set(e, Type::Void, None),
        }
    }

    // ------------------------------------------------------ convertidor analogico-digital (pyxpic.periph.adc)
    fn adc_method(&mut self, e: &Expr, method: &str, args: &[Expr], kwargs: &[Kwarg]) -> Type {
        let void = |this: &mut Self| this.set(e, Type::Void, None);
        if method != "start" {
            if let Some(k) = kwargs.first() {
                self.d.error("PYP2172", k.name_span, format!("ADC.{method} no acepta argumentos con nombre"));
            }
        }
        let channel = |this: &mut Self, a: &Expr| -> Option<u8> {
            let ExprKind::Name(n) = &a.kind else {
                this.expr(a);
                this.d.error("PYP2174", a.span, format!("ADC.{method} recibe un canal (AN0..AN13): se escribe en el registro al compilar"));
                return None;
            };
            let Some(ch) = adc::channel(n).filter(|_| this.adc_consts.contains(n)) else {
                let hint = if adc::channel(n).is_some() { format!("; importalo con 'from pyxpic.periph.adc import {n}'") } else { String::new() };
                this.d.error("PYP2174", a.span, format!("'{n}' no es un canal del ADC (AN0..AN13){hint}"));
                return None;
            };
            this.set(a, Type::Literal, Some(ch as i128));
            this.note_claims(&format!("ANS{ch}"), a.span);
            Some(ch)
        };
        match method {
            "start" => {
                if !args.is_empty() {
                    self.d.error("PYP2127", args[0].span, "ADC.start se escribe con argumentos con nombre: ADC.start(), ADC.start(clock=FOSC_8, acquisition_us=20, low_voltage=True)");
                }
                let (mut clock, mut cfg) = (None, adc::AdcConfig::default());
                let mut reference_uv: Option<i128> = None;
                for k in kwargs {
                    match (k.name.as_str(), &k.value.kind) {
                        ("clock", ExprKind::Name(n)) if self.adc_consts.contains(n) && adc::CLOCKS.iter().any(|(c, _)| c == n) => {
                            clock = adc::CLOCKS.iter().find(|(c, _)| c == n).map(|(_, c)| *c);
                            self.set(&k.value, Type::Literal, None);
                        }
                        ("clock", _) => {
                            self.expr(&k.value);
                            self.d.error("PYP2172", k.value.span, "ADC.start: clock es FOSC_2, FOSC_8, FOSC_32 o FRC (importados de pyxpic.periph.adc)");
                        }
                        ("acquisition_us", _) => {
                            self.expr(&k.value);
                            self.unit_arg(&k.value, Dim::Time, 1_000, "us", "ADC.start: acquisition_us");
                            match self.konst(&k.value) {
                                Some(us) if (1..=1000).contains(&us) && self.ty(&k.value) != Type::Bool => cfg.acq_ns = (us * 1000) as u32,
                                _ => self.d.error("PYP2172", k.value.span, "ADC.start: acquisition_us es una constante de 1 a 1000 (microsegundos de adquisicion tras cambiar de canal)"),
                            }
                        }
                        ("low_voltage", _) => {
                            self.expr(&k.value);
                            match (self.konst(&k.value), self.ty(&k.value)) {
                                (Some(v), Type::Bool) => cfg.low_voltage = v != 0,
                                _ => self.d.error("PYP2172", k.value.span, "ADC.start: low_voltage es True o False (VDD menor de 3,0 V)"),
                            }
                        }
                        ("reference", _) => {
                            // la tension de alimentacion (VDD) que sirve de referencia: no la genera el PIC, solo decide low_voltage
                            self.expr(&k.value);
                            if self.dim(&k.value).is_none() {
                                self.d.error("PYP2240", k.value.span, "ADC.start: reference lleva una tension con unidad: reference=5V");
                            } else {
                                self.unit_arg(&k.value, Dim::Voltage, 1, "uV", "ADC.start: reference");
                                if let Some(uv) = self.konst(&k.value) {
                                    match self.target.supply_uv {
                                        Some((lo, hi)) if uv < lo as i128 || uv > hi as i128 => self.d.error("PYP2242", k.value.span, format!("ADC.start: reference={} V esta fuera de la alimentacion de {} ({} V a {} V)", uv as f64 / 1e6, self.target.name, lo as f64 / 1e6, hi as f64 / 1e6)),
                                        _ => reference_uv = Some(uv),
                                    }
                                }
                            }
                        }
                        _ => {
                            self.expr(&k.value);
                            self.d.error("PYP2172", k.name_span, format!("ADC.start no tiene el argumento '{}' (admite: clock, acquisition_us, low_voltage, reference)", k.name));
                        }
                    }
                }
                if let Some(uv) = reference_uv {
                    // VDD menor de 3,0 V: el mismo criterio de low_voltage
                    let low = uv < 3_000_000;
                    match kwargs.iter().find(|k| k.name == "low_voltage") {
                        Some(k) if cfg.low_voltage != low => self.d.error("PYP2172", k.name_span, format!("low_voltage={} contradice reference: con {} V la alimentacion {} menor de 3,0 V", if cfg.low_voltage { "True" } else { "False" }, uv as f64 / 1e6, if low { "es" } else { "no es" })),
                        _ => cfg.low_voltage = low,
                    }
                }
                self.adc_started = true;
                self.adc_pending.push(AdcStart { id: e.id, span: e.span, clock, cfg, func: self.cur });
                void(self)
            }
            "select" | "read" => {
                if args.len() != 1 {
                    self.d.error("PYP2055", e.span, format!("'ADC.{method}' recibe un canal (AN0..AN13)"));
                    for a in args {
                        self.expr(a);
                    }
                    return self.set(e, Type::Error, None);
                }
                self.adc_needs.get_or_insert(e.span);
                if let Some(ch) = channel(self, &args[0]) {
                    let op = if method == "select" { AdcOp::Select(ch) } else { AdcOp::Read(ch) };
                    self.adc_calls.insert(e.id, AdcCall { op });
                }
                if method == "select" {
                    void(self)
                } else {
                    self.set(e, U16, None)
                }
            }
            "go" | "done" | "result" => {
                if !args.is_empty() {
                    self.d.error("PYP2055", e.span, format!("'ADC.{method}' no recibe argumentos"));
                    for a in args {
                        self.expr(a);
                    }
                }
                self.adc_needs.get_or_insert(e.span);
                let op = match method {
                    "go" => AdcOp::Go,
                    "done" => AdcOp::Done,
                    _ => AdcOp::Result,
                };
                self.adc_calls.insert(e.id, AdcCall { op });
                match op {
                    AdcOp::Done => self.set(e, Type::Bool, None),
                    AdcOp::Result => self.set(e, U16, None),
                    _ => void(self),
                }
            }
            other => {
                self.d.error("PYP2173", e.span, format!("ADC no tiene el metodo '{other}' (disponibles: start, select, go, done, result, read)"));
                for a in args {
                    self.expr(a);
                }
                self.set(e, Type::Error, None)
            }
        }
    }

    // ------------------------------------------------------ EUSART asincrono 8N1 (pyxpic.periph.uart)
    fn uart_method(&mut self, e: &Expr, method: &str, args: &[Expr], kwargs: &[Kwarg]) -> Type {
        let void = |this: &mut Self| this.set(e, Type::Void, None);
        if method != "start" {
            if let Some(k) = kwargs.first() {
                self.d.error("PYP2182", k.name_span, format!("UART.{method} no acepta argumentos con nombre"));
            }
        }
        if method == "write_text" {
            return self.uart_write_text(e, args);
        }
        if method == "read_line" || method == "write_buf" {
            return self.uart_buffer_op(e, method, args);
        }
        let (want, op, ret): (usize, UartOp, Type) = match method {
            "start" => {
                if !args.is_empty() {
                    self.d.error("PYP2127", args[0].span, "UART.start se escribe con un argumento con nombre: UART.start(baud=9600)");
                }
                let mut baud = None;
                let mut bits9 = false;
                for k in kwargs {
                    if k.name == "bits" {
                        self.expr(&k.value);
                        match self.konst(&k.value) {
                            Some(8) if self.ty(&k.value) != Type::Bool => {}
                            Some(9) if self.ty(&k.value) != Type::Bool => bits9 = true,
                            _ => self.d.error("PYP2182", k.value.span, "UART.start: bits es 8 o 9 (constante)"),
                        }
                        continue;
                    }
                    if k.name != "baud" {
                        self.expr(&k.value);
                        self.d.error("PYP2182", k.name_span, format!("UART.start no tiene el argumento '{}' (admite: baud, bits)", k.name));
                        continue;
                    }
                    self.expr(&k.value);
                    self.unit_arg(&k.value, Dim::Baud, 1, "baud", "UART.start: baud");
                    match self.konst(&k.value) {
                        Some(b) if (50..=3_000_000).contains(&b) && self.ty(&k.value) != Type::Bool => baud = Some(b as u32),
                        _ => self.d.error("PYP2182", k.value.span, "UART.start: baud es una constante de 50 a 3000000 (baudios): se calcula al compilar"),
                    }
                }
                let Some(baud) = baud else {
                    if kwargs.iter().all(|k| k.name != "baud") {
                        self.d.error("PYP2182", e.span, "UART.start necesita baud=...: UART.start(baud=9600)");
                    }
                    return void(self);
                };
                self.note_claims("SPEN", e.span);
                self.uart_started = true;
                self.uart_pending.push(UartPending { id: e.id, span: e.span, baud, bits9, func: self.cur });
                return void(self);
            }
            "write" => (1, UartOp::Write, Type::Void),
            "send" => (1, UartOp::Send, Type::Void),
            "read" => (0, UartOp::Read, U8),
            "recv" => (0, UartOp::Recv, U8),
            "tx_ready" => (0, UartOp::TxReady, Type::Bool),
            "rx_ready" => (0, UartOp::RxReady, Type::Bool),
            "sending" => (0, UartOp::Sending, Type::Bool),
            "overrun" => (0, UartOp::Overrun, Type::Bool),
            "framing_error" => (0, UartOp::FramingError, Type::Bool),
            "clear_overrun" => (0, UartOp::ClearOverrun, Type::Void),
            "write9" => (1, UartOp::Write9, Type::Void),
            "send9" => (1, UartOp::Send9, Type::Void),
            "read9" => (0, UartOp::Read9, U16),
            "recv9" => (0, UartOp::Recv9, U16),
            other => {
                self.d.error("PYP2183", e.span, format!("UART no tiene el metodo '{other}' (disponibles: start, write, read, send, recv, write_text, write_buf, read_line, write9, read9, send9, recv9, tx_ready, rx_ready, sending, overrun, framing_error, clear_overrun)"));
                for a in args {
                    self.expr(a);
                }
                return self.set(e, Type::Error, None);
            }
        };
        if args.len() != want {
            self.d.error("PYP2055", e.span, format!("'UART.{method}' recibe {want} argumento(s)"));
            for a in args {
                self.expr(a);
            }
            return self.set(e, Type::Error, None);
        }
        if want == 1 {
            if op.is_9bit() {
                self.expect(&args[0], U16, Some(&format!("UART.{method} envia un uint16 de 9 bits (0 a 511): convierte con uint16(...)")));
            } else {
                self.expect(&args[0], U8, Some(&format!("UART.{method} envia un uint8: convierte con uint8(...)")));
            }
        }
        if op.is_9bit() {
            self.uart_uses9.get_or_insert(e.span);
        } else if op.is_8bit_data() {
            self.uart_uses8.get_or_insert(e.span);
        }
        self.uart_needs.get_or_insert(e.span);
        self.uart_calls.insert(e.id, UartCall { op });
        self.set(e, ret, None)
    }

    /// `UART.write_text("Hola\r\n")` o `UART.write_text(TABLA)`: el texto se resuelve al compilar y vive en la memoria de programa (una
    /// `Rom` anonima, compartida entre literales iguales); no se reserva ningun bufer en RAM.
    fn uart_write_text(&mut self, e: &Expr, args: &[Expr]) -> Type {
        if args.len() != 1 {
            self.d.error("PYP2055", e.span, "'UART.write_text' recibe 1 argumento: un texto entre comillas o una tabla Rom[uint8]");
            for a in args {
                if !matches!(a.kind, ExprKind::Str(_)) {
                    self.expr(a);
                }
            }
            return self.set(e, Type::Error, None);
        }
        let a = &args[0];
        if let ExprKind::FStr(parts) = &a.kind {
            return self.uart_write_fmt(e, a, parts);
        }
        let rom = match &a.kind {
            ExprKind::Str(raw) => match uart::decode_text(raw) {
                Ok(b) if b.is_empty() => {
                    self.d.error("PYP2165", a.span, "UART.write_text con un texto vacio no envia nada: quita la llamada");
                    None
                }
                Ok(b) => Some(self.text_rom(b)),
                Err(msg) => {
                    self.d.error("PYP2165", a.span, msg);
                    None
                }
            },
            ExprKind::Name(n) if self.rom_idx.contains_key(n) => Some(self.rom_idx[n]),
            _ => {
                if !matches!(a.kind, ExprKind::Str(_)) {
                    self.expr(a);
                }
                self.d.error(
                    "PYP2166",
                    a.span,
                    "UART.write_text recibe un texto entre comillas o el nombre de una tabla Rom[uint8]: el texto se conoce al compilar y se guarda en la memoria de programa",
                );
                None
            }
        };
        let Some(rom) = rom else { return self.set(e, Type::Error, None) };
        let len = self.roms[rom].1.len();
        if len > 255 {
            self.d.error("PYP2165", a.span, format!("UART.write_text envia hasta 255 bytes de una vez, este texto tiene {len}: divídelo en varias llamadas"));
            return self.set(e, Type::Error, None);
        }
        if let Some(cur) = self.cur {
            self.functions[cur].uses_rom = true;
        }
        self.uart_uses8.get_or_insert(e.span);
        self.uart_needs.get_or_insert(e.span);
        self.uart_calls.insert(e.id, UartCall { op: UartOp::WriteText(rom) });
        self.set(e, Type::Void, None)
    }

    /// `UART.write_text(f"ADC: {valor:04X}\r\n")`: el texto y los campos de valor constante se resuelven al compilar (a una `Rom`);
    /// los campos de variable se convierten con codigo nativo en el PIC. Solo enteros sin signo (uint8 y uint16).
    fn uart_write_fmt(&mut self, e: &Expr, a: &Expr, parts: &[crate::ast::FPart]) -> Type {
        use crate::ast::{FKind, FPart};
        let mut out: Vec<FmtPart> = Vec::new();
        let mut pending: Vec<u8> = Vec::new();
        let mut ok = true;
        let mut has_field = false;
        let mut hex_tables: [Option<usize>; 2] = [None, None];
        for (i, p) in parts.iter().enumerate() {
            match p {
                FPart::Lit(raw) => match uart::decode_text(raw) {
                    Ok(b) => pending.extend(b),
                    Err(msg) => {
                        self.d.error("PYP2165", a.span, msg);
                        ok = false;
                    }
                },
                FPart::Field { expr, spec } => {
                    let t = self.expr(expr);
                    let bits = match t {
                        Type::Error => {
                            ok = false;
                            continue;
                        }
                        Type::Int { bits: b @ (8 | 16), signed: false } => b,
                        other => {
                            self.d.error("PYP2167", expr.span, format!("el campo es de tipo {other}: write_text formatea uint8 y uint16 por ahora (convierte con uint8(...) o uint16(...))"));
                            ok = false;
                            continue;
                        }
                    };
                    let natural = if spec.kind == FKind::Dec {
                        0
                    } else if matches!(spec.kind, FKind::Bin) {
                        bits
                    } else {
                        bits / 4
                    };
                    if spec.kind != FKind::Dec && spec.width != 0 && spec.width < natural {
                        let (unit, tipo) = if spec.kind == FKind::Bin { ("binarios", format!("uint{bits}")) } else { ("hexadecimales", format!("uint{bits}")) };
                        self.d.error("PYP2168", expr.span, format!("un {tipo} se muestra con {natural} digitos {unit} como minimo (ancho fijo, sin recortar): el ancho {} es menor", spec.width));
                        ok = false;
                        continue;
                    }
                    has_field = true;
                    if let Some(v) = self.konst(expr) {
                        // un valor conocido al compilar se formatea aqui y no cuesta nada en el PIC
                        pending.extend(uart::format_uint(v as u16, bits, *spec));
                        continue;
                    }
                    if !pending.is_empty() {
                        let r = self.text_rom(std::mem::take(&mut pending));
                        out.push(FmtPart::Lit(r));
                    }
                    let tables = match spec.kind {
                        FKind::HexUpper | FKind::HexLower => {
                            let k = (spec.kind == FKind::HexLower) as usize;
                            if hex_tables[k].is_none() {
                                hex_tables[k] = Some(self.text_rom(if k == 1 { uart::HEX_LOWER.to_vec() } else { uart::HEX_UPPER.to_vec() }));
                            }
                            [hex_tables[k], None]
                        }
                        FKind::Dec if bits == 8 => [Some(self.text_rom(uart::POW10_8_LO.to_vec())), None],
                        FKind::Dec => [Some(self.text_rom(uart::POW10_16_LO.to_vec())), Some(self.text_rom(uart::POW10_16_HI.to_vec()))],
                        FKind::Bin => [None, None],
                    };
                    out.push(FmtPart::Field { ast: i, bits, spec: *spec, tables });
                }
            }
        }
        if !ok {
            return self.set(e, Type::Error, None);
        }
        let _ = has_field;
        if !pending.is_empty() {
            let r = self.text_rom(pending);
            out.push(FmtPart::Lit(r));
        }
        if out.is_empty() {
            self.d.error("PYP2165", a.span, "UART.write_text con un texto vacio no envia nada: quita la llamada");
            return self.set(e, Type::Error, None);
        }
        for p in &out {
            if let FmtPart::Lit(r) = p {
                if self.roms[*r].1.len() > 255 {
                    self.d.error("PYP2165", a.span, format!("cada trozo de texto constante de write_text tiene hasta 255 bytes y este tiene {}: divídelo en varias llamadas", self.roms[*r].1.len()));
                    return self.set(e, Type::Error, None);
                }
            }
        }
        if let Some(cur) = self.cur {
            self.functions[cur].uses_rom = true;
        }
        self.uart_uses8.get_or_insert(e.span);
        self.uart_needs.get_or_insert(e.span);
        self.uart_calls.insert(e.id, UartCall { op: UartOp::WriteFmt });
        self.uart_fmt.insert(e.id, out);
        self.set(e, Type::Void, None)
    }

    /// `UART.read_line(buf)` -> uint8 y `UART.write_buf(buf, n)`: el arreglo `uint8[N]` se nombra directamente (no es un valor).
    fn uart_buffer_op(&mut self, e: &Expr, method: &str, args: &[Expr]) -> Type {
        let (want, ret) = if method == "read_line" { (1, U8) } else { (2, Type::Void) };
        if args.len() != want {
            for a in args {
                self.expr_unless_array(a);
            }
            let usage = if method == "read_line" { "UART.read_line(buf)" } else { "UART.write_buf(buf, n)" };
            self.d.error("PYP2055", e.span, format!("'UART.{method}' se escribe {usage}"));
            return self.set(e, Type::Error, None);
        }
        let arr = match &args[0].kind {
            ExprKind::Name(n) if self.array_idx.contains_key(n) => Some(self.array_idx[n]),
            _ => {
                self.expr_unless_array(&args[0]);
                self.d.error("PYP2188", args[0].span, format!("UART.{method} recibe el nombre de un arreglo uint8[N] (buf: uint8[16]): el texto vive en un arreglo de RAM declarado explicitamente"));
                None
            }
        };
        if want == 2 {
            self.expect(&args[1], U8, Some("n, la cantidad de bytes, es un uint8"));
        }
        let Some(arr) = arr else { return self.set(e, Type::Error, None) };
        self.uart_uses8.get_or_insert(e.span);
        self.uart_needs.get_or_insert(e.span);
        let op = if method == "read_line" { UartOp::ReadLine(arr) } else { UartOp::WriteBuf(arr) };
        self.uart_calls.insert(e.id, UartCall { op });
        self.set(e, ret, None)
    }

    /// Como `expr` pero sin quejarse por el nombre de un arreglo (el error propio ya lo explica).
    fn expr_unless_array(&mut self, a: &Expr) {
        if !matches!(&a.kind, ExprKind::Name(n) if self.array_idx.contains_key(n)) && !matches!(a.kind, ExprKind::Str(_) | ExprKind::FStr(_)) {
            self.expr(a);
        }
    }

    /// La tabla anonima de un literal de texto: dos literales iguales comparten los mismos bytes en flash.
    fn text_rom(&mut self, bytes: Vec<u8>) -> usize {
        if let Some(i) = self.roms.iter().position(|(n, v)| n.starts_with("texto_") && !self.rom_idx.contains_key(n) && *v == bytes) {
            return i;
        }
        let mut k = self.roms.len();
        while self.rom_idx.contains_key(&format!("texto_{k}")) || self.roms.iter().any(|(n, _)| *n == format!("texto_{k}")) {
            k += 1;
        }
        self.roms.push((format!("texto_{k}"), bytes));
        self.roms.len() - 1
    }

    /// UART usada sin un `UART.start(baud=...)` alcanzable, una sola velocidad y el generador de baudios resuelto con el reloj.
    ///
    /// Igual que con el ADC, se comprueba que el `start()` **exista** y se pueda ejecutar desde `main` o `@init_device`, no el orden.
    fn check_uart_usage(&mut self) {
        let pend = std::mem::take(&mut self.uart_pending);
        if let Some(sp) = self.uart_needs.take() {
            let reach = self.entry_reachable();
            if !pend.iter().any(|p| p.func.is_none_or(|f| reach.contains(&f))) {
                let why = if pend.is_empty() { "nunca se llama a UART.start(baud=...)" } else { "las llamadas a UART.start() estan en funciones que ni main ni @init_device pueden llamar" };
                self.d.error("PYP2181", sp, format!("la UART se usa pero {why}: inicializala en main o en una funcion que main llame (se comprueba que exista, no el orden)"));
            }
        }
        if let Some(first) = pend.first() {
            if let Some(other) = pend.iter().find(|p| p.bits9 != first.bits9) {
                self.d.error("PYP2186", other.span, "UART.start pide un ancho de caracter distinto (bits=8 y bits=9): el EUSART tiene una sola configuracion");
            }
            if first.bits9 {
                if let Some(sp) = self.uart_uses8.take() {
                    self.d.error("PYP2187", sp, "con UART.start(bits=9) solo valen write9, send9, read9 y recv9: la operacion de 8 bits dejaria TX9D sin definir o descartaria el noveno bit");
                }
            } else if let Some(sp) = self.uart_uses9.take() {
                self.d.error("PYP2187", sp, "write9, send9, read9 y recv9 necesitan UART.start(baud=..., bits=9)");
            }
            if let Some(other) = pend.iter().find(|p| p.baud != first.baud) {
                self.d.error("PYP2186", other.span, format!("UART.start pide {} baudios y otra llamada {}: el EUSART tiene un solo generador; repite la misma velocidad", other.baud, first.baud));
            }
        }
        for p in pend {
            let Some(hz) = self.effective_clock(p.span, "UART.start") else { continue };
            let Some(brg) = uart::solve(hz, p.baud) else {
                self.d.error("PYP2182", p.span, format!("{} baudios no se pueden generar con un reloj de {hz} Hz: ni con SPBRG = 0 y BRGH = BRG16 = 1 (FOSC/4) se llega tan alto", p.baud));
                continue;
            };
            let err = brg.error_pct(hz);
            if err.abs() > uart::MAX_PCT {
                self.d.error(
                    "PYP2182",
                    p.span,
                    format!("a {hz} Hz el generador de baudios da {:.0} baudios para los {} pedidos (error {err:+.2} %): pasa de {} % y la comunicacion fallaria aunque el otro extremo fuera exacto; cambia el reloj o la velocidad", brg.achieved(hz), p.baud, uart::MAX_PCT),
                );
                continue;
            }
            if err.abs() > uart::WARN_PCT {
                self.d.warning(
                    "PYP3014",
                    p.span,
                    format!(
                        "error de baudios de {err:+.2} % ({:.0} baudios para los {} pedidos): pasa de {} % y el otro extremo suma el suyo; un reloj distinto lo mejora",
                        brg.achieved(hz),
                        p.baud,
                        uart::WARN_PCT
                    ),
                );
            }
            self.uart_calls.insert(p.id, UartCall { op: UartOp::Start(UartStart { baud: p.baud, bits9: p.bits9, brg: Some(brg) }) });
        }
    }

    /// Funciones alcanzables desde `main` y `@init_device` (o, sin `main`, desde las que nadie llama).
    fn entry_reachable(&self) -> BTreeSet<usize> {
        let called: BTreeSet<&String> = self.functions.iter().flat_map(|f| f.calls.iter()).collect();
        let roots: Vec<usize> = if let Some(&m) = self.fidx.get("main") {
            std::iter::once(m).chain((0..self.functions.len()).filter(|&i| self.functions[i].is_init)).collect()
        } else {
            (0..self.functions.len()).filter(|&i| !self.functions[i].is_interrupt && !called.contains(&self.functions[i].name)).collect()
        };
        let mut seen = BTreeSet::new();
        let mut stack = roots;
        while let Some(f) = stack.pop() {
            if seen.insert(f) {
                stack.extend(self.functions[f].calls.iter().filter_map(|c| self.fidx.get(c).copied()));
            }
        }
        seen
    }

    /// ADC usado sin una `ADC.start()` alcanzable, condiciones de las llamadas y reloj automatico.
    ///
    /// Limite explicito: se comprueba que **exista** un `start()` en una funcion que `main` o `@init_device` pueden llamar, no el
    /// orden en que se ejecutan; usar el ADC antes de iniciarlo lo detecta el simulador (GO con ADON apagado), no el compilador.
    fn check_adc_usage(&mut self) {
        let pend = std::mem::take(&mut self.adc_pending);
        if let Some(sp) = self.adc_needs.take() {
            let reach = self.entry_reachable();
            if !pend.iter().any(|p| p.func.is_none_or(|f| reach.contains(&f))) {
                let why = if pend.is_empty() { "nunca se llama a ADC.start()" } else { "las llamadas a ADC.start() estan en funciones que ni main ni @init_device pueden llamar" };
                self.d.error("PYP2171", sp, format!("el ADC se usa pero {why}: inicialo con ADC.start() en main o en una funcion que main llame (se comprueba que exista, no el orden)"));
            }
        }
        // el programa tiene una sola condicion electrica: todas las llamadas deben coincidir
        if let Some(first) = pend.first() {
            if let Some(other) = pend.iter().find(|p| p.cfg != first.cfg) {
                self.d.error("PYP2176", other.span, "las llamadas a ADC.start() piden condiciones distintas (acquisition_us / low_voltage): el programa tiene una sola; repite los mismos valores");
            }
            self.adc_config = first.cfg;
            if first.cfg.acq_ns < adc::TAMP_MAX_NS {
                self.d.warning(
                    "PYP3013",
                    first.span,
                    format!(
                        "acquisition_us = {} us es menos que el TAMP maximo del datasheet (5 us, AD133): el amplificador no llega a asentarse en el peor caso; el valor tipico es 11,5 us",
                        first.cfg.acq_ns as f64 / 1000.0
                    ),
                );
            }
        }
        for p in pend {
            let cfg = p.cfg;
            let clock = match p.clock {
                Some(c) => c,
                None => match self.effective_clock(p.span, "ADC.start") {
                    Some(hz) => adc::auto_clock(hz, &cfg),
                    None => continue,
                },
            };
            // un reloj elegido a mano fuera de AD130 es un aviso: el resultado dejaria de ser fiable
            if let (Some(hz), Some(_)) = (self.clock_hz, clock.divisor()) {
                if !adc::tad_in_spec(clock, hz, &cfg) {
                    let t = adc::tad_ns(clock, hz);
                    self.d.warning(
                        "PYP3012",
                        p.span,
                        format!(
                            "el reloj del ADC da TAD = {t:.0} ns y el datasheet (AD130) exige {}..{} ns{}: la conversion no es fiable; omite clock= para que se elija solo",
                            cfg.tad_min_ns(),
                            adc::TAD_MAX_NS,
                            if cfg.low_voltage { " con VDD < 3,0 V" } else { "" }
                        ),
                    );
                }
            }
            self.adc_calls.insert(p.id, AdcCall { op: AdcOp::Start(Some(clock)) });
        }
    }

    // ------------------------------------------------------ comparadores (pyxpic.periph.comparator) y CVREF (pyxpic.periph.vref)
    fn cmp_method(&mut self, e: &Expr, id: CmpId, method: &str, args: &[Expr], kwargs: &[Kwarg]) -> Type {
        let name = id.name();
        let void = |this: &mut Self| this.set(e, Type::Void, None);
        match method {
            "start" => {
                if !args.is_empty() {
                    self.d.error("PYP2215", args[0].span, format!("{name}.start se escribe con argumentos con nombre: {name}.start(negative=C12IN0, positive=REF_PIN, invert=False)"));
                    for a in args {
                        self.expr(a);
                    }
                }
                let (mut neg, mut pos, mut invert) = (None, None, false);
                for k in kwargs {
                    match (k.name.as_str(), &k.value.kind) {
                        ("negative", ExprKind::Name(n)) if self.comp_consts.contains(n) && comparator::NEG_INPUTS.iter().any(|(c, _)| c == n) => {
                            let ch = comparator::NEG_INPUTS.iter().position(|(c, _)| c == n).unwrap();
                            neg = Some(ch as u8);
                            self.set(&k.value, Type::Literal, None);
                            self.note_claims(&format!("ANS{}", comparator::NEG_INPUTS[ch].1), k.value.span);
                        }
                        ("negative", _) => {
                            self.expr(&k.value);
                            self.d.error("PYP2215", k.value.span, format!("{name}.start: negative es la entrada inversora, C12IN0, C12IN1, C12IN2 o C12IN3 (importadas de pyxpic.periph.comparator)"));
                        }
                        ("positive", ExprKind::Name(n)) if self.comp_consts.contains(n) && comparator::REFS.iter().any(|(c, _)| c == n) => {
                            pos = comparator::REFS.iter().find(|(c, _)| c == n).map(|(_, r)| *r);
                            self.set(&k.value, Type::Literal, None);
                            match pos {
                                Some(CmpRef::Pin) => self.note_claims(&format!("ANS{}", id.pos_pin_an()), k.value.span),
                                Some(CmpRef::Cvref) => {
                                    self.cvref_needed.get_or_insert(k.value.span);
                                }
                                _ => {}
                            }
                        }
                        ("positive", _) => {
                            self.expr(&k.value);
                            self.d.error(
                                "PYP2215",
                                k.value.span,
                                format!("{name}.start: positive es REF_PIN (el pin C{}IN+), REF_CVREF o REF_FIXED (0,6 V), importadas de pyxpic.periph.comparator", id.n()),
                            );
                        }
                        ("invert", _) => {
                            self.expr(&k.value);
                            match (self.konst(&k.value), self.ty(&k.value)) {
                                (Some(v), Type::Bool) => invert = v != 0,
                                _ => self.d.error("PYP2215", k.value.span, format!("{name}.start: invert es True o False (CxPOL: invierte el nivel de salida)")),
                            }
                        }
                        _ => {
                            self.expr(&k.value);
                            self.d.error("PYP2215", k.name_span, format!("{name}.start no tiene el argumento '{}' (admite: negative, positive, invert)", k.name));
                        }
                    }
                }
                if let (Some(neg), Some(pos)) = (neg, pos) {
                    self.comp_calls.insert(e.id, CompCall::Cmp(id, CompOp::Start { neg, pos, invert }));
                }
                // un argumento ausente (uno mal escrito ya dio su propio error)
                for want in ["negative", "positive"] {
                    if kwargs.iter().all(|k| k.name != want) {
                        self.d.error("PYP2215", e.span, format!("{name}.start necesita {want}=: {name}.start(negative=C12IN0, positive=REF_PIN)"));
                    }
                }
                void(self)
            }
            "stop" | "out" => {
                if let Some(k) = kwargs.first() {
                    self.d.error("PYP2215", k.name_span, format!("{name}.{method} no acepta argumentos con nombre"));
                }
                if !args.is_empty() {
                    self.d.error("PYP2055", e.span, format!("'{name}.{method}' no recibe argumentos"));
                    for a in args {
                        self.expr(a);
                    }
                }
                let op = if method == "stop" { CompOp::Stop } else { CompOp::Out };
                self.comp_calls.insert(e.id, CompCall::Cmp(id, op));
                if op == CompOp::Out {
                    self.set(e, Type::Bool, None)
                } else {
                    void(self)
                }
            }
            other => {
                self.d.error("PYP2216", e.span, format!("{name} no tiene el metodo '{other}' (disponibles: start, stop, out)"));
                for a in args {
                    self.expr(a);
                }
                self.set(e, Type::Error, None)
            }
        }
    }

    /// Un paso de CVREF: constante de 0 a 15.
    fn cvref_step(&mut self, a: &Expr, what: &str) -> Option<u8> {
        self.expr(a);
        match (self.konst(a), self.ty(a)) {
            (Some(v), t) if t != Type::Bool && (0..comparator::CVREF_STEPS as i128).contains(&v) => Some(v as u8),
            _ => {
                self.d.error("PYP2221", a.span, format!("{what} es una constante de 0 a 15 (VR<3:0>): se escribe en el registro al compilar"));
                None
            }
        }
    }

    fn cvref_method(&mut self, e: &Expr, method: &str, args: &[Expr], kwargs: &[Kwarg]) -> Type {
        let void = |this: &mut Self| this.set(e, Type::Void, None);
        match method {
            "start" => {
                if !args.is_empty() {
                    self.d.error("PYP2221", args[0].span, "CVREF.start se escribe con argumentos con nombre: CVREF.start(step=8, low_range=False)");
                    for a in args {
                        self.expr(a);
                    }
                }
                let (mut step, mut low) = (None, false);
                for k in kwargs {
                    match k.name.as_str() {
                        "step" => step = self.cvref_step(&k.value, "CVREF.start: step"),
                        "low_range" => {
                            self.expr(&k.value);
                            match (self.konst(&k.value), self.ty(&k.value)) {
                                (Some(v), Type::Bool) => low = v != 0,
                                _ => self.d.error("PYP2221", k.value.span, "CVREF.start: low_range es True (rango bajo, 0 a 15/24 de VDD) o False (rango alto, 1/4 a 23/32 de VDD)"),
                            }
                        }
                        _ => {
                            self.expr(&k.value);
                            self.d.error("PYP2221", k.name_span, format!("CVREF.start no tiene el argumento '{}' (admite: step, low_range)", k.name));
                        }
                    }
                }
                self.cvref_started = true;
                match step {
                    Some(step) => {
                        self.comp_calls.insert(e.id, CompCall::Cvref(CvrefOp::Start { step, low_range: low }));
                    }
                    None if kwargs.iter().all(|k| k.name != "step") => self.d.error("PYP2221", e.span, "CVREF.start necesita step=: CVREF.start(step=8)"),
                    None => {}
                }
                void(self)
            }
            "set" => {
                if let Some(k) = kwargs.first() {
                    self.d.error("PYP2221", k.name_span, "CVREF.set no acepta argumentos con nombre: CVREF.set(8)");
                }
                if args.len() != 1 {
                    self.d.error("PYP2055", e.span, "'CVREF.set' recibe un paso (0 a 15)");
                    for a in args {
                        self.expr(a);
                    }
                    return void(self);
                }
                if let Some(step) = self.cvref_step(&args[0], "CVREF.set recibe el paso, que") {
                    self.comp_calls.insert(e.id, CompCall::Cvref(CvrefOp::Set(step)));
                }
                void(self)
            }
            "stop" => {
                if !args.is_empty() || !kwargs.is_empty() {
                    self.d.error("PYP2055", e.span, "'CVREF.stop' no recibe argumentos");
                    for a in args {
                        self.expr(a);
                    }
                }
                self.comp_calls.insert(e.id, CompCall::Cvref(CvrefOp::Stop));
                void(self)
            }
            other => {
                self.d.error("PYP2222", e.span, format!("CVREF no tiene el metodo '{other}' (disponibles: start, set, stop)"));
                for a in args {
                    self.expr(a);
                }
                self.set(e, Type::Error, None)
            }
        }
    }

    /// Un comparador que pide `REF_CVREF` sin ninguna llamada a `CVREF.start()` en el programa (se comprueba que exista, no el orden).
    fn check_comparator_usage(&mut self) {
        if let Some(sp) = self.cvref_needed.take() {
            if !self.cvref_started {
                self.d.error(
                    "PYP2218",
                    sp,
                    "el comparador usa REF_CVREF pero nunca se llama a CVREF.start(): inicia la referencia con CVREF.start(step=...) (de 'from pyxpic.periph.vref import CVREF')",
                );
            }
        }
    }

    // ------------------------------------------------------ I2C maestro por hardware (pyxpic.periph.i2c)
    fn i2c_method(&mut self, e: &Expr, method: &str, args: &[Expr], kwargs: &[Kwarg]) -> Type {
        if method == "start" {
            if !args.is_empty() {
                self.d.error("PYP2226", args[0].span, "I2C.start se escribe con un argumento con nombre: I2C.start(freq=100000)");
            }
            let mut freq = None;
            for k in kwargs {
                self.expr(&k.value);
                if matches!(k.name.as_str(), "freq" | "frequency") {
                    self.unit_arg(&k.value, Dim::Frequency, 1, "Hz", "I2C.start: frequency");
                }
                if !matches!(k.name.as_str(), "freq" | "frequency") {
                    self.d.error("PYP2226", k.name_span, format!("I2C.start no tiene el argumento '{}' (admite: frequency)", k.name));
                    continue;
                }
                match self.konst(&k.value) {
                    Some(f) if (1_000..=1_000_000).contains(&f) && self.ty(&k.value) != Type::Bool => freq = Some(f as u32),
                    _ => self.d.error("PYP2226", k.value.span, "I2C.start: freq es una constante de 1000 a 1000000 (Hz): se calcula al compilar"),
                }
            }
            let Some(freq) = freq else {
                if kwargs.iter().all(|k| !matches!(k.name.as_str(), "freq" | "frequency")) {
                    self.d.error("PYP2226", e.span, "I2C.start necesita frequency=...: I2C.start(frequency=100kHz)");
                }
                return self.set(e, Type::Void, None);
            };
            self.note_claims_of("SSPEN", &["SCL", "SDA"], e.span);
            self.i2c_pending.push(I2cPending { id: e.id, span: e.span, freq, func: self.cur });
            return self.set(e, Type::Void, None);
        }
        if method == "read" {
            let ack = match kwargs {
                [k] if k.name == "ack" && args.is_empty() => Some(&k.value),
                _ => None,
            };
            let Some(ack) = ack else {
                for a in args.iter().chain(kwargs.iter().map(|k| &k.value)) {
                    self.expr(a);
                }
                self.d.error("PYP2226", e.span, "I2C.read se escribe I2C.read(ack=True) para pedir otro byte o I2C.read(ack=False) en el ultimo");
                return self.set(e, Type::Error, None);
            };
            self.expect(ack, Type::Bool, Some("ack es un bool: True pide otro byte, False marca el ultimo"));
            self.i2c_needs.get_or_insert(e.span);
            self.i2c_calls.insert(e.id, I2cCall { op: I2cOp::Read });
            return self.set(e, U8, None);
        }
        if let Some(k) = kwargs.first() {
            self.d.error("PYP2226", k.name_span, format!("I2C.{method} no acepta argumentos con nombre"));
        }
        let (op, arity, ret) = match method {
            "begin" => (I2cOp::Begin, 0, Type::Void),
            "restart" => (I2cOp::Restart, 0, Type::Void),
            "end" => (I2cOp::End, 0, Type::Void),
            "write" => (I2cOp::Write, 1, Type::Bool),
            other => {
                self.d.error("PYP2227", e.span, format!("I2C no tiene el metodo '{other}' (disponibles: start, begin, restart, end, write, read)"));
                for a in args {
                    self.expr(a);
                }
                return self.set(e, Type::Error, None);
            }
        };
        if args.len() != arity {
            let want = if arity == 1 { "el byte a enviar (la direccion con R/W en el bit 0, o un dato)" } else { "ningun argumento" };
            self.d.error("PYP2055", e.span, format!("'I2C.{method}' recibe {want}"));
            for a in args {
                self.expr(a);
            }
            return self.set(e, Type::Error, None);
        }
        for a in args {
            self.expect(a, U8, Some("I2C.write envia un uint8: convierte con uint8(...)"));
        }
        self.i2c_needs.get_or_insert(e.span);
        self.i2c_calls.insert(e.id, I2cCall { op });
        self.set(e, ret, None)
    }

    /// I2C usado sin un `I2C.start(freq=...)` alcanzable, una sola velocidad y SSPADD resuelto con el reloj.
    fn check_i2c_usage(&mut self) {
        let pend = std::mem::take(&mut self.i2c_pending);
        if let Some(sp) = self.i2c_needs.take() {
            let reach = self.entry_reachable();
            if !pend.iter().any(|p| p.func.is_none_or(|f| reach.contains(&f))) {
                let why = if pend.is_empty() { "nunca se llama a I2C.start(freq=...)" } else { "las llamadas a I2C.start() estan en funciones que ni main ni @init_device pueden llamar" };
                self.d.error("PYP2225", sp, format!("el I2C se usa pero {why}: inicializalo en main o en una funcion que main llame (se comprueba que exista, no el orden)"));
            }
        }
        if let Some(first) = pend.first() {
            if let Some(other) = pend.iter().find(|p| p.freq != first.freq) {
                self.d.error("PYP2228", other.span, format!("I2C.start pide {} Hz y otra llamada {} Hz: el MSSP tiene un solo generador; repite la misma velocidad", other.freq, first.freq));
            }
        }
        for p in pend {
            let Some(hz) = self.effective_clock(p.span, "I2C.start") else { continue };
            let Some(n) = i2c::solve(hz, p.freq) else {
                self.d.error(
                    "PYP2228",
                    p.span,
                    format!(
                        "{} Hz de SCL no se pueden generar con un reloj de {hz} Hz: SCL = FOSC / (4 * (SSPADD + 1)) con SSPADD de {} a {}, o sea de {:.0} a {:.0} Hz",
                        p.freq,
                        i2c::SSPADD_MIN,
                        i2c::SSPADD_MAX,
                        hz as f64 / (4.0 * (i2c::SSPADD_MAX as f64 + 1.0)),
                        hz as f64 / (4.0 * (i2c::SSPADD_MIN as f64 + 1.0))
                    ),
                );
                continue;
            };
            let st = I2cStart { freq: p.freq, sspadd: Some(n) };
            let got = st.achieved(hz);
            if got < p.freq as f64 * 0.8 {
                self.d.warning("PYP3019", p.span, format!("a {hz} Hz el MSSP da SCL = {got:.0} Hz para los {} Hz pedidos (SSPADD = {n}): un reloj distinto se acerca mas", p.freq));
            }
            self.i2c_calls.insert(p.id, I2cCall { op: I2cOp::Start(st) });
        }
    }

    /// Como `note_claims`, pero solo las funciones `only` del bit (el MSSP comparte el bit SSPEN entre SPI e I2C).
    fn note_claims_of(&mut self, bit: &str, only: &[&str], span: Span) {
        let Some(list) = self.target.pin_claims.as_ref().and_then(|m| m.get(bit)).cloned() else { return };
        for c in list.iter().filter(|c| only.contains(&c.function.as_str())) {
            for pin in &c.pins {
                self.claims.push(Claim { pin: pin.clone(), function: c.function.clone(), peripheral: c.peripheral.clone(), span, via_timer: false });
            }
        }
    }

    // ------------------------------------------------------ SPI maestro por hardware (pyxpic.periph.spi)
    fn spi_method(&mut self, e: &Expr, method: &str, args: &[Expr], kwargs: &[Kwarg]) -> Type {
        if method == "start" {
            if !args.is_empty() {
                self.d.error("PYP2234", args[0].span, "SPI.start se escribe con argumentos con nombre: SPI.start(freq=1000000, mode=0)");
            }
            let (mut freq, mut mode) = (None, Some(0u8));
            for k in kwargs {
                self.expr(&k.value);
                if matches!(k.name.as_str(), "freq" | "frequency") {
                    self.unit_arg(&k.value, Dim::Frequency, 1, "Hz", "SPI.start: frequency");
                }
                let value = self.konst(&k.value).filter(|_| self.ty(&k.value) != Type::Bool);
                match k.name.as_str() {
                    "freq" | "frequency" => match value {
                        Some(f) if (1..=20_000_000).contains(&f) => freq = Some(f as u32),
                        _ => self.d.error("PYP2234", k.value.span, "SPI.start: freq es una constante de 1 a 20000000 (Hz), la maxima que admite el dispositivo conectado"),
                    },
                    "mode" => match value {
                        Some(m) if (0..=3).contains(&m) => mode = Some(m as u8),
                        _ => {
                            self.d.error("PYP2234", k.value.span, "SPI.start: mode es una constante de 0 a 3 (el modo SPI estandar)");
                            mode = None;
                        }
                    },
                    other => self.d.error("PYP2234", k.name_span, format!("SPI.start no tiene el argumento '{other}' (admite: frequency, mode)")),
                }
            }
            let (Some(freq), Some(mode)) = (freq, mode) else {
                if kwargs.iter().all(|k| !matches!(k.name.as_str(), "freq" | "frequency")) {
                    self.d.error("PYP2234", e.span, "SPI.start necesita frequency=...: SPI.start(frequency=1MHz)");
                }
                return self.set(e, Type::Void, None);
            };
            self.note_claims_of("SSPEN", &["SCK", "SDI", "SDO"], e.span);
            self.spi_pending.push(SpiPending { id: e.id, span: e.span, freq, mode, func: self.cur });
            return self.set(e, Type::Void, None);
        }
        if let Some(k) = kwargs.first() {
            self.d.error("PYP2234", k.name_span, format!("SPI.{method} no acepta argumentos con nombre"));
        }
        if method != "transfer" {
            self.d.error("PYP2235", e.span, format!("SPI no tiene el metodo '{method}' (disponibles: start, transfer; SPI envia y recibe a la vez)"));
            for a in args {
                self.expr(a);
            }
            return self.set(e, Type::Error, None);
        }
        if args.len() != 1 {
            self.d.error("PYP2055", e.span, "'SPI.transfer' recibe el byte a enviar y devuelve el recibido");
            for a in args {
                self.expr(a);
            }
            return self.set(e, Type::Error, None);
        }
        self.expect(&args[0], U8, Some("SPI.transfer envia un uint8: convierte con uint8(...)"));
        self.spi_needs.get_or_insert(e.span);
        self.spi_calls.insert(e.id, SpiCall { op: SpiOp::Transfer });
        self.set(e, U8, None)
    }

    /// SPI usado sin un `SPI.start` alcanzable, una sola configuracion, el divisor resuelto con el reloj y el MSSP no compartido con I2C.
    fn check_spi_usage(&mut self) {
        let pend = std::mem::take(&mut self.spi_pending);
        let uses_i2c = !self.i2c_pending.is_empty() || self.i2c_needs.is_some();
        if let Some(p) = pend.first().filter(|_| uses_i2c) {
            self.d.error("PYP2239", p.span, "el programa usa SPI e I2C: los dos son el mismo modulo MSSP (y comparten RC3 y RC4)");
        }
        if let Some(sp) = self.spi_needs.take() {
            let reach = self.entry_reachable();
            if !pend.iter().any(|p| p.func.is_none_or(|f| reach.contains(&f))) {
                let why = if pend.is_empty() { "nunca se llama a SPI.start(freq=...)" } else { "las llamadas a SPI.start() estan en funciones que ni main ni @init_device pueden llamar" };
                self.d.error("PYP2237", sp, format!("el SPI se usa pero {why}: inicializalo en main o en una funcion que main llame (se comprueba que exista, no el orden)"));
            }
        }
        if let Some(first) = pend.first() {
            if let Some(other) = pend.iter().find(|p| (p.freq, p.mode) != (first.freq, first.mode)) {
                self.d.error("PYP2238", other.span, "dos SPI.start con distinta freq o mode: el programa usa una sola configuracion del MSSP");
            }
        }
        for p in pend {
            let Some(hz) = self.effective_clock(p.span, "SPI.start") else { continue };
            let Some(sspm) = spi::solve(hz, p.freq) else {
                self.d.error("PYP2238", p.span, format!("con un reloj de {hz} Hz el SPI mas lento es FOSC/64 = {} Hz, mas rapido que los {} Hz pedidos", hz / 64, p.freq));
                continue;
            };
            let st = SpiStart { freq: p.freq, mode: p.mode, sspm: Some(sspm) };
            let got = st.achieved(hz);
            // por encima de FOSC/4 pedir "lo mas rapido posible" no merece aviso
            if (got as f64) < p.freq as f64 * 0.8 && sspm != 0 {
                self.d.warning("PYP3023", p.span, format!("a {hz} Hz el MSSP da SCK = {got} Hz para un maximo de {} Hz (solo hay FOSC/4, /16 y /64)", p.freq));
            }
            self.spi_calls.insert(p.id, SpiCall { op: SpiOp::Start(st) });
        }
    }

    // ------------------------------------------------------ EEPROM de datos (pyxpic.periph.eeprom)
    fn eeprom_method(&mut self, e: &Expr, method: &str, args: &[Expr], kwargs: &[Kwarg]) -> Type {
        if let Some(k) = kwargs.first() {
            self.d.error("PYP2211", k.name_span, format!("EEPROM.{method} no acepta argumentos con nombre"));
        }
        let (op, arity, ret) = match method {
            "read" => (EepromOp::Read, 1, U8),
            "write" => (EepromOp::Write, 2, Type::Void),
            "start_write" => (EepromOp::StartWrite, 2, Type::Void),
            "busy" => (EepromOp::Busy, 0, Type::Bool),
            "failed" => (EepromOp::Failed, 0, Type::Bool),
            other => {
                self.d.error("PYP2212", e.span, format!("EEPROM no tiene el metodo '{other}' (disponibles: read, write, start_write, busy, failed)"));
                for a in args {
                    self.expr(a);
                }
                return self.set(e, Type::Error, None);
            }
        };
        if args.len() != arity {
            let want = match method {
                "read" => "una direccion (0 a 255)",
                "write" | "start_write" => "la direccion (0 a 255) y el valor (0 a 255)",
                _ => "ningun argumento",
            };
            self.d.error("PYP2055", e.span, format!("'EEPROM.{method}' recibe {want}"));
            for a in args {
                self.expr(a);
            }
            return self.set(e, Type::Error, None);
        }
        for (i, a) in args.iter().enumerate() {
            let what = if i == 0 { "la direccion" } else { "el valor" };
            self.expect(a, U8, Some(&format!("EEPROM.{method} recibe {what} como uint8 (0 a 255)")));
        }
        self.eeprom_calls.insert(e.id, EepromCall { op });
        self.set(e, ret, None)
    }

    // ------------------------------------------------------ PWM de los modulos CCP (pyxpic.periph.pwm)
    fn pwm_method(&mut self, e: &Expr, id: PwmId, method: &str, args: &[Expr], kwargs: &[Kwarg]) -> Type {
        let name = id.name();
        let void = |this: &mut Self| this.set(e, Type::Void, None);
        if method != "start" {
            if let Some(k) = kwargs.first() {
                self.d.error("PYP2201", k.name_span, format!("{name}.{method} no acepta argumentos con nombre"));
            }
        }
        match method {
            "start" => {
                if !args.is_empty() {
                    self.d.error(
                        "PYP2201",
                        args[0].span,
                        format!("{name}.start se escribe con argumentos con nombre: {name}.start(frequency=1000), {name}.start(frequency=1000, duty=256) o {name}.start(prescaler=4, period=249)"),
                    );
                }
                let (mut freq, mut pre, mut period, mut duty) = (None, None, None, 0u16);
                let mut duty_pct: Option<i128> = None;
                let int_arg = |this: &mut Self, k: &Kwarg, lo: i128, hi: i128, what: &str| -> Option<i128> {
                    this.expr(&k.value);
                    if k.name == "frequency" {
                        this.unit_arg(&k.value, Dim::Frequency, 1, "Hz", &format!("{name}.start: frequency"));
                    }
                    match this.konst(&k.value) {
                        Some(v) if (lo..=hi).contains(&v) && this.ty(&k.value) != Type::Bool => Some(v),
                        _ => {
                            this.d.error("PYP2202", k.value.span, format!("{name}.start: {} es una constante {what}", k.name));
                            None
                        }
                    }
                };
                for k in kwargs {
                    match k.name.as_str() {
                        "frequency" => freq = int_arg(self, k, 1, 4_000_000, "de 1 a 4 000 000 (Hz)"),
                        "prescaler" => {
                            self.expr(&k.value);
                            match self.konst(&k.value) {
                                Some(v) if pwm::PRESCALERS.iter().any(|&p| p as i128 == v) && self.ty(&k.value) != Type::Bool => pre = Some(v as u8),
                                _ => self.d.error("PYP2202", k.value.span, format!("{name}.start: prescaler es 1, 4 o 16 (el prescaler de Timer2)")),
                            }
                        }
                        "period" => period = int_arg(self, k, 0, 255, "de 0 a 255 (el valor de PR2)").map(|v| v as u8),
                        "duty" => {
                            self.expr(&k.value);
                            match self.dim(&k.value) {
                                // un porcentaje es del periodo: se resuelve con PR2 al conocer el reloj
                                Some(Dim::Percent) => {
                                    self.consume_quantities(&k.value);
                                    match self.konst(&k.value).filter(|v| (0..=100_000).contains(v)) {
                                        Some(v) => duty_pct = Some(v),
                                        None => self.d.error("PYP2241", k.value.span, format!("{name}.start: duty va de 0pct a 100pct")),
                                    }
                                }
                                Some(d) => {
                                    self.consume_quantities(&k.value);
                                    self.d.error("PYP2240", k.value.span, format!("{name}.start: duty es un porcentaje (50pct) o un numero de 0 a 1023, y esto es {d}"));
                                }
                                None => duty = int_arg(self, k, 0, 1023, "de 0 a 1023 (10 bits: CCPRxL y DCxB)").map_or(0, |v| v as u16),
                            }
                        }
                        other => {
                            self.expr(&k.value);
                            self.d.error("PYP2201", k.name_span, format!("{name}.start no tiene el argumento '{other}' (admite: frequency, prescaler, period, duty)"));
                        }
                    }
                }
                let spec = match (freq, pre, period) {
                    (Some(f), None, None) => Some(PwmSpec::Frequency(f as u32)),
                    (None, Some(p), Some(r)) => Some(PwmSpec::Raw(PwmTiming { prescaler: p, pr2: r })),
                    (None, None, None) if kwargs.iter().all(|k| k.name != "frequency" && k.name != "prescaler" && k.name != "period") => {
                        self.d.error("PYP2201", e.span, format!("{name}.start necesita el periodo: frequency=1000, o prescaler=4 y period=249 (prescaler x (PR2+1) ciclos)"));
                        None
                    }
                    (Some(_), _, _) if pre.is_some() || period.is_some() => {
                        self.d.error("PYP2201", e.span, format!("{name}.start: o frequency, o prescaler y period (una sola forma de pedir el periodo)"));
                        None
                    }
                    (None, Some(_), None) | (None, None, Some(_)) => {
                        self.d.error("PYP2201", e.span, format!("{name}.start: prescaler y period van juntos (el periodo es prescaler x (PR2+1) ciclos)"));
                        None
                    }
                    _ => None,
                };
                self.pwm_uses.push(PwmUse { pwm: id, span: e.span, duty: if duty_pct.is_some() { None } else { Some(duty as i128) } });
                self.pwm_pending.push(PwmStart { id: e.id, span: e.span, pwm: id, spec, duty, duty_pct, func: self.cur });
                self.claims.push(Claim { pin: id.pin().into(), function: id.function().into(), peripheral: name.into(), span: e.span, via_timer: false });
                void(self)
            }
            "duty" => {
                if args.len() != 1 {
                    self.d.error("PYP2055", e.span, format!("'{name}.duty' recibe el ciclo de trabajo (0 a 1023, en cuentas de TOSC x prescaler)"));
                    for a in args {
                        self.expr(a);
                    }
                    return self.set(e, Type::Error, None);
                }
                self.expect(&args[0], U16, Some(&format!("{name}.duty recibe un uint16 (0 a 1023)")));
                let c = self.konst(&args[0]);
                if let Some(v) = c {
                    if !(0..=1023).contains(&v) {
                        self.d.error("PYP2202", args[0].span, format!("{name}.duty: el ciclo de trabajo tiene 10 bits (0 a 1023), {v} no cabe"));
                    }
                }
                self.pwm_uses.push(PwmUse { pwm: id, span: e.span, duty: c });
                self.pwm_calls.insert(e.id, PwmCall { pwm: id, op: PwmOp::Duty });
                void(self)
            }
            "stop" => {
                if !args.is_empty() {
                    self.d.error("PYP2055", e.span, format!("'{name}.stop' no recibe argumentos"));
                    for a in args {
                        self.expr(a);
                    }
                }
                self.pwm_uses.push(PwmUse { pwm: id, span: e.span, duty: None });
                self.pwm_calls.insert(e.id, PwmCall { pwm: id, op: PwmOp::Stop });
                void(self)
            }
            other => {
                self.d.error("PYP2203", e.span, format!("{name} no tiene el metodo '{other}' (disponibles: start, duty, stop)"));
                for a in args {
                    self.expr(a);
                }
                self.set(e, Type::Error, None)
            }
        }
    }

    /// PWM: un `start()` alcanzable por modulo usado, un solo periodo para los dos modulos (comparten Timer2), Timer2 no es de otro
    /// uso y la frecuencia se resuelve con el reloj. Como con el ADC se comprueba que el `start()` **exista**, no el orden.
    fn check_pwm_usage(&mut self) {
        let pend = std::mem::take(&mut self.pwm_pending);
        let uses = std::mem::take(&mut self.pwm_uses);
        if uses.is_empty() {
            return;
        }
        let reach = self.entry_reachable();
        for id in [PwmId::Pwm1, PwmId::Pwm2] {
            let Some(u) = uses.iter().find(|u| u.pwm == id) else { continue };
            if !pend.iter().any(|p| p.pwm == id && p.func.is_none_or(|f| reach.contains(&f))) {
                let why = if pend.iter().any(|p| p.pwm == id) {
                    format!("las llamadas a {0}.start() estan en funciones que ni main ni @init_device pueden llamar", id.name())
                } else {
                    format!("nunca se llama a {}.start()", id.name())
                };
                self.d.error(
                    "PYP2204",
                    u.span,
                    format!("{} se usa pero {why}: inicialo con {}.start(...) en main o en una funcion que main llame (se comprueba que exista, no el orden)", id.name(), id.name()),
                );
            }
        }
        // el PWM es dueno de Timer2: T2CON y PR2 los fija el arranque
        if let Some(c) = self.timer_calls.values().find(|c| c.timer == TimerId::T2) {
            let _ = c;
            self.d.error("PYP2206", uses[0].span, "el PWM usa Timer2 (su periodo es PR2 y el prescaler de T2CON) y el programa tambien usa TMR2: un solo uso de Timer2");
        }
        let cpc = self.target.clocks_per_cycle.unwrap_or(4);
        let mut resolved: Vec<(Span, PwmTiming)> = Vec::new();
        for p in pend {
            let timing = match p.spec {
                None => continue,
                Some(PwmSpec::Raw(t)) => Some(t),
                Some(PwmSpec::Frequency(f)) => match self.effective_clock(p.span, &format!("{}.start", p.pwm.name())) {
                    Some(hz) => match pwm::resolve_frequency(hz, cpc, f) {
                        Ok(t) => Some(t),
                        Err(msg) => {
                            self.d.error("PYP2207", p.span, msg);
                            None
                        }
                    },
                    None => None,
                },
            };
            let Some(t) = timing else { continue };
            resolved.push((p.span, t));
            // el porcentaje es del periodo (4 x (PR2 + 1) cuentas), redondeado a la cuenta mas cercana
            let duty = match p.duty_pct {
                Some(m) => ((t.full_scale() as i128 * m + 50_000) / 100_000).min(1023) as u16,
                None => p.duty,
            };
            self.pwm_calls.insert(p.id, PwmCall { pwm: p.pwm, op: PwmOp::Start(Some(t), duty) });
        }
        if let Some((_, first)) = resolved.first().copied() {
            if let Some((sp, other)) = resolved.iter().find(|(_, t)| *t != first) {
                self.d.error(
                    "PYP2208",
                    *sp,
                    format!(
                        "los modulos PWM comparten Timer2: un solo periodo por programa (aqui prescaler {} y PR2 = {}, antes prescaler {} y PR2 = {})",
                        other.prescaler, other.pr2, first.prescaler, first.pr2
                    ),
                );
            }
            for u in &uses {
                if let Some(v) = u.duty.filter(|v| (0..=1023).contains(v) && *v as u32 > first.full_scale()) {
                    self.d.warning(
                        "PYP3015",
                        u.span,
                        format!(
                            "{}: el ciclo de trabajo {v} supera el periodo ({} cuentas con PR2 = {}): la salida queda siempre en alto (100 %); el maximo util es {}",
                            u.pwm.name(),
                            first.full_scale(),
                            first.pr2,
                            first.full_scale()
                        ),
                    );
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn method_call(&mut self, e: &Expr, obj: &str, obj_span: Span, attr: Option<&Ident>, method: &str, args: &[Expr], kwargs: &[Kwarg]) -> Type {
        if obj == "UART" && self.uart_name {
            return self.uart_method(e, method, args, kwargs);
        }
        if obj == "ADC" && self.adc_name {
            return self.adc_method(e, method, args, kwargs);
        }
        if obj == "EEPROM" && self.eeprom_name {
            return self.eeprom_method(e, method, args, kwargs);
        }
        if obj == "I2C" && self.i2c_name {
            return self.i2c_method(e, method, args, kwargs);
        }
        if obj == "SPI" && self.spi_name {
            return self.spi_method(e, method, args, kwargs);
        }
        if let Some(id) = CmpId::from_name(obj).filter(|_| self.comp_names.contains(obj)) {
            return self.cmp_method(e, id, method, args, kwargs);
        }
        if obj == "CVREF" && self.cvref_name {
            return self.cvref_method(e, method, args, kwargs);
        }
        if let Some(id) = PwmId::from_name(obj).filter(|_| self.pwm_names.contains(obj)) {
            return self.pwm_method(e, id, method, args, kwargs);
        }
        if self.reg_names.contains_key(obj) {
            return self.reg_method(e, obj, attr, method, args, kwargs);
        }
        if let Some(b) = attr {
            self.d.error("PYP2131", b.span, format!("'{obj}.{}' no existe: solo los registros de 'pyxpic.regs' tienen bits con nombre, p. ej. STATUS.RP0.set()", b.name));
            for a in args {
                self.expr(a);
            }
            return self.set(e, Type::Error, None);
        }
        if let Some(&tid) = self.timer_names.get(obj) {
            return self.timer_method(e, tid, method, args, kwargs);
        }
        if let Some(k) = kwargs.first() {
            if self.pin_names.contains_key(obj) {
                self.d.error("PYP2082", k.name_span, format!("los metodos de un pin no aceptan argumentos con nombre ('{}')", k.name));
            }
            for k in kwargs {
                self.expr(&k.value);
            }
        }
        let Some(&pi) = self.pin_names.get(obj) else {
            let hint = if self.pin_info(obj).is_some() { format!("; importalo con 'from pyxpic.periph.gpio import {obj}'") } else { String::new() };
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
                        t => self.d.error("PYP2041", a.span, format!("value() recibe un uint8 o un bool, no {t}")),
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
                    self.d.error("PYP2030", te.span, format!("el tipo '{t}' no esta soportado por el destino '{}' (maximo {} bits)", self.target.name, self.target.max_int_bits));
                    Type::Error
                }
                None => {
                    self.d.error("PYP2001", te.span, format!("tipo '{n}' no valido; usa uint8, int8, uint16, int16, uint32, int32 o bool"));
                    Type::Error
                }
            },
            TypeExprKind::Fixed { binary, base, scale } => {
                let kind = if *binary { "QFixed" } else { "Fixed" };
                let Some(b) = Type::from_name(base).filter(|t| t.is_int()) else {
                    self.d.error("PYP2252", te.span, format!("{kind}[T, n]: T es un entero (uint8, int8, uint16, int16, uint32, int32), no '{base}'"));
                    return Type::Error;
                };
                if !self.target.supports(b) {
                    self.d.error("PYP2030", te.span, format!("el tipo '{b}' no esta soportado por el destino '{}'", self.target.name));
                    return Type::Error;
                }
                let (bits, signed) = (b.bits() as i128, b.is_signed());
                // QFixed: n bits fraccionarios; Fixed: el mayor n con 10^n <= el maximo del entero (que quepa al menos 1.0)
                let max_scale = if *binary { bits - signed as i128 } else { (0..=9).filter(|n| 10i128.pow(*n as u32) <= b.max()).max().unwrap_or(0) };
                if !(0..=max_scale).contains(scale) {
                    self.d.error("PYP2252", te.span, format!("{kind}[{b}, {scale}]: la escala de {b} va de 0 a {max_scale}"));
                    return Type::Error;
                }
                self.note_emulated(b, te.span);
                Type::Fixed { bits: bits as u8, signed, scale: *scale as u8, binary: *binary }
            }
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
                format!("'{t}' excede el ancho nativo de {} bits de '{}': se emulara por software (mayor coste en flash y ciclos)", self.target.native_bits, self.target.name),
            );
        }
    }

    fn set(&mut self, e: &Expr, ty: Type, konst: Option<i128>) -> Type {
        self.info[e.id as usize] = ExprInfo { ty, konst };
        ty
    }
    /// Un parametro con unidad (`UART.start(baud=9600baud)`, `pause(500ms)`): convierte el literal a la unidad del parametro
    /// (`per_unit` unidades base por unidad del parametro). Un numero sin unidad se deja como esta (la forma de siempre). Una magnitud
    /// distinta de `want` es `PYP2240`; un valor que no es entero en la unidad del parametro, `PYP2241`.
    fn unit_arg(&mut self, e: &Expr, want: Dim, per_unit: i128, unit: &str, what: &str) {
        self.consume_quantities(e);
        let Some(d) = self.dim(e) else { return };
        self.dims[e.id as usize] = None;
        if d != want {
            self.d.error("PYP2240", e.span, format!("{what} espera {want} ({unit}) y esto es {d}"));
            self.info[e.id as usize].konst = None;
            return;
        }
        let Some(v) = self.konst(e) else { return };
        if v % per_unit != 0 {
            self.d.error("PYP2241", e.span, format!("{what}: ese valor no es un numero entero de {unit}, la unidad de este parametro; usa otra funcion o parametro con una unidad menor"));
            self.info[e.id as usize].konst = None;
            return;
        }
        self.info[e.id as usize].konst = Some(v / per_unit);
    }

    // ------------------------------------------------------------ punto fijo (docs/punto-fijo.md)

    /// El valor racional exacto de una constante (`Literal`, entero constante o `Decimal`) como `raw` de `f`: `Fixed` exige que sea exacto;
    /// `QFixed` redondea al valor mas cercano y avisa. Deja la expresion con el tipo `f` y el `raw` como constante.
    fn fixed_coerce(&mut self, e: &Expr, f: Type) -> bool {
        let Some((n, d)) = self.rational_of(e) else {
            self.d.error("PYP2254", e.span, format!("un valor {f} se escribe como constante (0.5, 25.37) o se convierte con {f}(x): '{}' no es una constante", self.ty(e)));
            return false;
        };
        let Some(num) = n.checked_mul(f.factor()) else {
            self.d.error("PYP2253", e.span, format!("la constante no cabe en {f}"));
            return false;
        };
        let (raw, exact) = if num % d == 0 {
            (num / d, true)
        } else if matches!(f, Type::Fixed { binary: true, .. }) {
            (if num >= 0 { (num + d / 2) / d } else { -((-num + d / 2) / d) }, false)
        } else {
            let Type::Fixed { scale, .. } = f else { unreachable!() };
            self.d.error("PYP2252", e.span, format!("{} no es exacto con {scale} decimales: {f} guarda multiplos de 1/10^{scale}; usa mas decimales o un valor que lo sea", show_ratio(n, d)));
            return false;
        };
        if !f.contains(raw) {
            self.d.error("PYP2253", e.span, format!("{} no cabe en {f} (va de {} a {})", show_ratio(n, d), show_ratio(f.min(), f.factor()), show_ratio(f.max(), f.factor())));
            return false;
        }
        if !exact {
            self.d.warning("PYP3024", e.span, format!("{} no es exacto en {f}: se guarda {} (el multiplo de 1/{} mas cercano)", show_ratio(n, d), show_ratio(raw, f.factor()), f.factor()));
        }
        self.set(e, f, Some(raw));
        true
    }

    /// Los dos lados de `+ - ==...` con punto fijo: el mismo tipo, o un lado constante que se convierte al tipo del otro.
    fn fixed_unify(&mut self, l: &Expr, r: &Expr, lt: Type, rt: Type, op: &str) -> Option<Type> {
        match (lt.is_fixed(), rt.is_fixed()) {
            (true, true) if lt == rt => Some(lt),
            (true, true) => {
                self.d.error("PYP2254", l.span.to(r.span), format!("tipos de punto fijo distintos para '{op}': {lt} y {rt}; convierte uno: {lt}(...)"));
                None
            }
            (true, false) if matches!(rt, Type::Literal | Type::Decimal) => self.fixed_coerce(r, lt).then_some(lt),
            (false, true) if matches!(lt, Type::Literal | Type::Decimal) => self.fixed_coerce(l, rt).then_some(rt),
            _ => {
                let f = if lt.is_fixed() { lt } else { rt };
                self.d.error("PYP2254", l.span.to(r.span), format!("no se mezcla {lt} con {rt} en '{op}': convierte el entero con {f}(x)"));
                None
            }
        }
    }

    fn fixed_addsub(&mut self, e: &Expr, op: BinOp, l: &Expr, r: &Expr, lt: Type, rt: Type) -> Type {
        let Some(t) = self.fixed_unify(l, r, lt, rt, op.symbol()) else { return self.set(e, Type::Error, None) };
        if let (Some(a), Some(b)) = (self.konst(l), self.konst(r)) {
            return match fold(op, a, b) {
                Some(v) if t.contains(v) => self.set(e, t, Some(v)),
                _ => {
                    self.d.error("PYP2253", e.span, format!("la constante no cabe en {t}"));
                    self.set(e, Type::Error, None)
                }
            };
        }
        self.set(e, t, None)
    }

    /// `*` y `/` con punto fijo: dos del mismo tipo (producto con intermedio de 32 bits y reescala), o uno por una constante (un entero
    /// escala sin reescalar; un decimal o una division se hacen con una fraccion exacta).
    fn fixed_muldiv(&mut self, e: &Expr, op: BinOp, l: &Expr, r: &Expr, lt: Type, rt: Type) -> Type {
        let err = |this: &mut Self| this.set(e, Type::Error, None);
        match (lt.is_fixed(), rt.is_fixed()) {
            (true, true) => {
                if op != BinOp::Mul {
                    self.d.error("PYP2253", e.span, "no se divide un punto fijo entre otro punto fijo: divide por una constante (x / 2, x / 2.5)");
                    return err(self);
                }
                if lt != rt {
                    self.d.error("PYP2254", e.span, format!("tipos de punto fijo distintos para '*': {lt} y {rt}; convierte uno: {lt}(...)"));
                    return err(self);
                }
                let Type::Fixed { bits, signed, scale, binary } = lt else { unreachable!() };
                if let (Some(a), Some(b)) = (self.konst(l), self.konst(r)) {
                    let p = a * b;
                    let v = if binary { p >> scale } else { p / lt.factor() };
                    if !lt.contains(v) {
                        self.d.error("PYP2253", e.span, format!("el producto constante no cabe en {lt}"));
                        return err(self);
                    }
                    return self.set(e, lt, Some(v));
                }
                if bits > 8 {
                    self.d.error("PYP2255", e.span, format!("el producto de dos {lt} necesita un intermedio de {} bits: {WIDE_MSG}; con una base de 8 bits (Fixed[uint8, n]) si cabe", 2 * bits));
                    return err(self);
                }
                self.fixed_ops.insert(e.id, FixedOp::Mul { binary, scale, signed });
                self.set(e, lt, None)
            }
            _ => {
                let fixed_left = lt.is_fixed();
                let (fx, oe, ot) = if fixed_left { (lt, r, rt) } else { (rt, l, lt) };
                if !fixed_left && op != BinOp::Mul {
                    self.d.error("PYP2253", e.span, "no se divide una constante entre un punto fijo");
                    return err(self);
                }
                let signed = fx.is_signed();
                let fixed_e = if fixed_left { l } else { r };
                let child = if fixed_left { 0 } else { 1 };
                let (num, den, neutral) = match ot {
                    // un entero multiplica el valor sin reescalar: 2 * x
                    Type::Literal if op == BinOp::Mul => (0, 1, true),
                    Type::Literal => {
                        let k = self.konst(oe).unwrap_or(0);
                        if k == 0 {
                            self.d.error("PYP2050", oe.span, "division por cero");
                            return err(self);
                        }
                        (1, k, false)
                    }
                    Type::Decimal => {
                        let (p, q) = self.rats[oe.id as usize].unwrap();
                        if op == BinOp::Mul {
                            (p, q, false)
                        } else {
                            if p == 0 {
                                self.d.error("PYP2050", oe.span, "division por cero");
                                return err(self);
                            }
                            (q, p, false)
                        }
                    }
                    other => {
                        self.d.error("PYP2254", e.span, format!("un {fx} se multiplica por otro {fx} o por una constante, no por {other}: convierte el entero con {fx}(x)"));
                        return err(self);
                    }
                };
                if neutral {
                    let k = self.konst(oe).unwrap_or(0);
                    if !fx.to_ir().contains(k) {
                        self.d.error("PYP2253", oe.span, format!("el multiplicador {k} no cabe en la base de {fx}"));
                        return err(self);
                    }
                    self.set(oe, fx.to_ir(), Some(k));
                    return match self.konst(fixed_e) {
                        Some(a) if fx.contains(a * k) => self.set(e, fx, Some(a * k)),
                        Some(_) => {
                            self.d.error("PYP2253", e.span, format!("el producto constante no cabe en {fx}"));
                            err(self)
                        }
                        None => self.set(e, fx, None),
                    };
                }
                let Some((num, den)) = crate::units::ratio(num, den) else { return err(self) };
                if num.abs() > 32_767 || den > 32_767 {
                    self.d.error("PYP2256", oe.span, format!("la fraccion {num}/{den} es demasiado grande: el numerador y el denominador llegan a 32767"));
                    return err(self);
                }
                if let Some(a) = self.konst(fixed_e) {
                    let v = a * num / den;
                    if !fx.contains(v) {
                        self.d.error("PYP2253", e.span, format!("el resultado constante no cabe en {fx}"));
                        return err(self);
                    }
                    return self.set(e, fx, Some(v));
                }
                if !fits_wide(fx, num, signed) {
                    self.d.error("PYP2255", e.span, format!("{fx} por {num}/{den} necesita un intermedio de mas de 16 bits: {WIDE_MSG}; divide (el numerador 1) o usa una base de 8 bits"));
                    return err(self);
                }
                self.fixed_ops.insert(e.id, FixedOp::Scale { num, den, child, signed });
                self.set(e, fx, None)
            }
        }
    }

    /// `Fixed[int16, 2](x)`: un entero (x * factor), una constante decimal o otro punto fijo (reescala exacta con intermedio de 32 bits).
    fn fixed_cast(&mut self, e: &Expr, te: &TypeExpr, arg: &Expr) -> Type {
        let target = self.resolve_type(te, false);
        let at = self.expr(arg);
        if target == Type::Error || at == Type::Error {
            return self.set(e, Type::Error, None);
        }
        let signed_any = |a: Type| a.is_signed() || target.is_signed();
        match at {
            Type::Literal | Type::Decimal => {
                if !self.fixed_coerce(arg, target) {
                    return self.set(e, Type::Error, None);
                }
                let k = self.konst(arg);
                self.set(e, target, k)
            }
            Type::Int { .. } | Type::Fixed { .. } => {
                // raw_dst = raw_src * factor_dst / factor_src
                let (num, den) = crate::units::ratio(target.factor(), at.factor()).unwrap_or((1, 1));
                if let Some(c) = self.konst(arg) {
                    let v = c * num / den;
                    if !target.contains(v) {
                        self.d.error("PYP2253", e.span, format!("la constante no cabe en {target}"));
                        return self.set(e, Type::Error, None);
                    }
                    return self.set(e, target, Some(v));
                }
                if !fits_wide(at, num, signed_any(at)) {
                    self.d.error("PYP2255", e.span, format!("convertir {at} a {target} necesita un intermedio de mas de 16 bits: {WIDE_MSG}"));
                    return self.set(e, Type::Error, None);
                }
                self.fixed_ops.insert(e.id, FixedOp::Scale { num, den, child: 0, signed: signed_any(at) });
                self.set(e, target, None)
            }
            other => {
                self.d.error("PYP2053", e.span, format!("no se puede convertir {other} a {target}"));
                self.set(e, Type::Error, None)
            }
        }
    }

    /// Una constante racional exacta: siempre un `decimal` (`1.0 / 4` es 0.25, no la division entera 0). Un decimal con valor entero
    /// pasa a ser un entero solo donde se pide un entero (`expect`): `RESULT: Const[uint8] = 100 * 0.5`.
    fn set_rational(&mut self, e: &Expr, (n, d): (i128, i128)) -> Type {
        match crate::units::ratio(n, d) {
            Some(r) => {
                self.rats[e.id as usize] = Some(r);
                self.set(e, Type::Decimal, None)
            }
            None => {
                self.d.error("PYP2250", e.span, "decimal invalido o demasiado grande");
                self.set(e, Type::Error, None)
            }
        }
    }

    /// El valor racional de una expresion constante (decimal o entera).
    fn rational_of(&self, e: &Expr) -> Option<(i128, i128)> {
        match self.ty(e) {
            Type::Decimal => self.rats[e.id as usize],
            Type::Literal | Type::Int { .. } => self.konst(e).map(|k| (k, 1)),
            _ => None,
        }
    }

    /// `+ - * /` con un decimal: aritmetica racional exacta al compilar. Un resultado entero es un literal entero (`100 * 0.5` es 50);
    /// uno no entero sigue siendo un decimal. Con un valor de ejecucion es un error: sin punto fijo no hay forma de operar un decimal en el PIC.
    fn decimal_binary(&mut self, e: &Expr, op: BinOp, l: &Expr, r: &Expr) -> Type {
        let (Some((a, b)), Some((c, d))) = (self.rational_of(l), self.rational_of(r)) else {
            self.d.error("PYP2251", e.span, "un decimal no entero solo se opera con constantes al compilar: con un valor en ejecucion hace falta un tipo de punto fijo (Fixed o QFixed)");
            return self.set(e, Type::Error, None);
        };
        let res = match op {
            BinOp::Add => a.checked_mul(d).zip(c.checked_mul(b)).and_then(|(x, y)| x.checked_add(y)).zip(b.checked_mul(d)),
            BinOp::Sub => a.checked_mul(d).zip(c.checked_mul(b)).and_then(|(x, y)| x.checked_sub(y)).zip(b.checked_mul(d)),
            BinOp::Mul => a.checked_mul(c).zip(b.checked_mul(d)),
            BinOp::TrueDiv if c == 0 => {
                self.d.error("PYP2050", r.span, "division por cero");
                return self.set(e, Type::Error, None);
            }
            BinOp::TrueDiv => a.checked_mul(d).zip(b.checked_mul(c)),
            BinOp::Div => {
                self.d.error("PYP2251", e.span, "'//' es la division entera: con un decimal usa '/' (division exacta, 1.0 / 3)");
                return self.set(e, Type::Error, None);
            }
            _ => {
                self.d.error("PYP2251", e.span, format!("el operador '{}' no se aplica a un decimal: solo + - * /", op.symbol()));
                return self.set(e, Type::Error, None);
            }
        };
        match res {
            Some((n, den)) => self.set_rational(e, (n, den)),
            None => {
                self.d.error("PYP2250", e.span, "el resultado decimal es demasiado grande");
                self.set(e, Type::Error, None)
            }
        }
    }

    /// `FACTOR: Const = 0.5`: una constante sin tipo; el valor decide (decimal o entero). Solo a nivel de modulo; no ocupa RAM.
    fn declare_const_auto(&mut self, v: &VarDecl, is_global: bool) {
        let name = &v.name.name;
        if !is_global {
            self.d.error("PYP2011", v.name.span, "un Const sin tipo se declara a nivel de modulo: dentro de una funcion usa Const[tipo]");
            return;
        }
        if self.globals.contains_key(name) || self.decimal_consts.contains_key(name) || self.fidx.contains_key(name) {
            self.d.error("PYP2009", v.name.span, format!("'{name}' ya esta declarado"));
            return;
        }
        let Some(val) = &v.value else {
            self.d.error("PYP2007", v.span, format!("'{name}' debe inicializarse en su declaracion"));
            return;
        };
        match (self.expr(val), self.konst(val)) {
            (Type::Error, _) => {}
            (Type::Decimal, _) => {
                let r = self.rats[val.id as usize].unwrap();
                self.decimal_consts.insert(name.clone(), r);
            }
            (Type::Literal, Some(k)) => {
                self.globals.insert(name.clone(), Symbol { name: name.clone(), ty: Type::Literal, kind: SymKind::Global, konst: Some(k), is_final: true });
            }
            _ => self.d.error("PYP2011", val.span, "un valor Const debe ser una expresion constante"),
        }
    }

    /// La magnitud de una expresion, si la tiene.
    fn dim(&self, e: &Expr) -> Option<Dim> {
        self.dims[e.id as usize]
    }

    /// Marca como usados los literales con unidad de `e` (el contexto que los acepta, como `FREQUENCY:`, lo llama).
    fn consume_quantities(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Quantity(..) => self.quantity_pending.retain(|(id, _, _)| *id != e.id),
            ExprKind::Unary(_, i) => self.consume_quantities(i),
            ExprKind::Binary(_, l, r) => {
                self.consume_quantities(l);
                self.consume_quantities(r);
            }
            _ => {}
        }
    }

    /// Los literales con unidad que ningun parametro uso: `500ms` donde se espera un numero no tiene un significado definido.
    fn flush_quantities(&mut self) {
        for (_, span, dim) in std::mem::take(&mut self.quantity_pending) {
            self.d.error("PYP2230", span, format!("un valor de {dim} ({}) no se puede usar aqui: ningun parametro de este contexto lo acepta todavia", dim.base_unit()));
        }
    }

    /// Magnitud de una operacion constante: `+` y `-` piden la misma magnitud; `*` y `/` con un numero la conservan; dividir dos valores
    /// de la misma magnitud da un numero; las demas combinaciones (`MHz * MHz`, `1 / MHz`) no tienen una unidad definida.
    fn combine_dims(&mut self, e: &Expr, op: BinOp, l: &Expr, r: &Expr) {
        let (a, b) = (self.dim(l), self.dim(r));
        if a.is_none() && b.is_none() {
            return;
        }
        let bad = |this: &mut Self, why: String| this.d.error("PYP2231", e.span, why);
        let out = match (op, a, b) {
            (BinOp::Add | BinOp::Sub, Some(x), Some(y)) if x == y => Some(x),
            (BinOp::Add | BinOp::Sub, Some(x), Some(y)) => {
                bad(self, format!("no se pueden {} {x} y {y}: son magnitudes distintas", if op == BinOp::Add { "sumar" } else { "restar" }));
                None
            }
            (BinOp::Add | BinOp::Sub, Some(x), None) | (BinOp::Add | BinOp::Sub, None, Some(x)) => {
                bad(self, format!("no se puede {} un numero sin unidad a un valor de {x}: escribe la unidad (por ejemplo 1kHz)", if op == BinOp::Add { "sumar" } else { "restar" }));
                None
            }
            (BinOp::Mul, Some(x), None) | (BinOp::Mul, None, Some(x)) => Some(x),
            (BinOp::Div, Some(x), None) => Some(x),
            (BinOp::Div, Some(x), Some(y)) if x == y => None,
            (BinOp::Mul | BinOp::Div, Some(x), Some(y)) => {
                bad(self, format!("{x} {} {y} no tiene una unidad definida (solo se multiplica una magnitud por un numero)", if op == BinOp::Mul { "por" } else { "entre" }));
                None
            }
            (BinOp::Div, None, Some(y)) => {
                bad(self, format!("un numero entre un valor de {y} no tiene una unidad definida"));
                None
            }
            (_, Some(x), _) | (_, _, Some(x)) => Some(x),
            _ => None,
        };
        self.dims[e.id as usize] = out;
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
        if self.pin_names.contains_key(&f.name.name)
            || self.time_fns.contains_key(&f.name.name)
            || self.bits_fns.contains(&f.name.name)
            || self.timer_names.contains_key(&f.name.name)
            || self.reg_names.contains_key(&f.name.name)
            || self.timer_consts.contains(&f.name.name)
        {
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
                self.d.error("PYP2004", f.name.span, format!("la funcion '{}' necesita anotacion de retorno (usa '-> None' si no devuelve valor)", f.name.name));
                Type::Error
            }
        };
        let mut is_interrupt = false;
        let mut is_init = false;
        let mut is_asm = false;
        for dec in &f.decorators {
            if dec.name == "interrupt" {
                if self.functions.iter().any(|x| x.is_interrupt) {
                    self.d.error("PYP2005", dec.span, "solo se permite una funcion @interrupt");
                }
                is_interrupt = true;
                if !params.is_empty() || !matches!(ret, Type::Void | Type::Error) {
                    self.d.error("PYP2006", f.name.span, "una funcion @interrupt no recibe parametros ni devuelve valor");
                }
            } else if dec.name == "asm_function" {
                is_asm = true;
                let small = |t: &Type| matches!(t, Type::Error | Type::Bool) || (t.is_int() && t.bits() <= 16);
                if f.asm.is_none() {
                    self.d.error("PYP2110", f.name.span, "una funcion @asm_function lleva el ensamblador como unica cadena en su cuerpo");
                }
                if !params.iter().all(|(_, t)| small(t)) || !(ret == Type::Void || small(&ret)) {
                    self.d.error("PYP2112", f.name.span, "una funcion @asm_function usa parametros y retorno de hasta 16 bits (uint8, int8, uint16, int16, bool)");
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
        if is_asm && (is_interrupt || is_init) {
            self.d.error("PYP2113", f.name.span, "una funcion @asm_function no puede ser a la vez @interrupt o @init_device");
        }
        if is_interrupt && is_init {
            self.d.error("PYP2097", f.name.span, "una funcion no puede ser a la vez @interrupt y @init_device");
        }
        self.fidx.insert(f.name.name.clone(), self.functions.len());
        self.functions.push(FuncInfo { name: f.name.name.clone(), params, ret, is_interrupt, is_init, is_asm, calls: BTreeSet::new(), uses_rom: false, runtime_depth: 0, span: f.name.span });
    }

    fn declare_variable(&mut self, v: &VarDecl, is_global: bool) {
        let name = &v.name.name;
        if let TypeExprKind::Array(elem, n) = &v.ann.kind {
            return self.declare_array(v, elem, *n, is_global);
        }
        match &v.ann.kind {
            TypeExprKind::Clock(kind) => return self.declare_clock(v, kind, is_global),
            TypeExprKind::Frequency(e) => return self.declare_frequency(v, e, is_global),
            TypeExprKind::OscMode(m) => return self.declare_osc_mode(v, m, is_global),
            TypeExprKind::Setting(w, e) => return self.declare_clock_setting(v, w.as_deref(), e.as_deref(), is_global),
            _ => {}
        }
        if matches!(v.ann.kind, TypeExprKind::ConstAuto) {
            return self.declare_const_auto(v, is_global);
        }
        match &v.ann.kind {
            TypeExprKind::Const(inner) if matches!(inner.kind, TypeExprKind::Rom(_)) => {
                let TypeExprKind::Rom(elem) = &inner.kind else { unreachable!() };
                return self.declare_rom(v, elem, is_global, true);
            }
            TypeExprKind::Rom(elem) => return self.declare_rom(v, elem, is_global, false),
            _ => {}
        }
        let (inner, is_final) = match &v.ann.kind {
            TypeExprKind::Const(inner) => (&**inner, true),
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
        } else if exists
            || self.fidx.contains_key(name)
            || self.pin_names.contains_key(name)
            || self.time_fns.contains_key(name)
            || self.bits_fns.contains(name)
            || self.timer_names.contains_key(name)
            || self.reg_names.contains_key(name)
            || self.timer_consts.contains(name)
            || self.array_idx.contains_key(name)
            || self.rom_idx.contains_key(name)
        {
            self.d.error("PYP2009", v.name.span, format!("'{name}' ya esta declarado"));
        }
        let mut konst = None;
        if let Some(val) = &v.value {
            if ty != Type::Error {
                self.expect(val, ty, None);
                konst = self.konst(val);
                if konst.is_none() && (is_global || is_final) {
                    let (code, msg) =
                        if is_global { ("PYP2010", "el inicializador de una variable global debe ser una constante") } else { ("PYP2011", "un valor Const debe ser una expresion constante") };
                    self.d.error(code, val.span, msg);
                }
            } else {
                self.expr(val);
            }
        }
        if is_global && name == "CLOCK_HZ" {
            self.check_clock(v, ty, is_final, konst);
        }
        if !is_global {
            self.local_decls.push((name.clone(), v.name.span));
        }
        let sym = Symbol { name: name.clone(), ty, kind: if is_global { SymKind::Global } else { SymKind::Local }, konst: if is_final { konst } else { None }, is_final };
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
        self.local_decls.clear();
        self.read_names.clear();
        self.global_decls.clear();
        self.loop_depth = 0;
        self.fn_labels.clear();
        collect_labels(&f.body, &mut self.fn_labels, self.d);
        let params = self.functions[idx].params.clone();
        for (name, ty) in params {
            if self.globals.contains_key(&name) {
                self.d.error("PYP2008", f.name.span, format!("el parametro '{name}' oculta una variable global"));
            }
            self.locals.insert(name.clone(), Symbol { name, ty, kind: SymKind::Param, konst: None, is_final: false });
        }
        if f.asm.is_none() {
            self.block(&f.body);
            // variables y parametros que nunca se leen (un nombre que empieza por `_` es intencional y no avisa)
            for (name, span) in std::mem::take(&mut self.local_decls) {
                if !name.starts_with('_') && !self.read_names.contains(&name) {
                    self.d.warning("PYP3021", span, format!("la variable local '{name}' se declara pero nunca se lee: quitala o usala (un nombre que empieza por _ no avisa)"));
                }
            }
            for p in &f.params {
                if !p.name.name.starts_with('_') && !self.read_names.contains(&p.name.name) {
                    self.d.warning("PYP3022", p.name.span, format!("el parametro '{}' de '{}' nunca se usa: quitalo o llamalo _{}", p.name.name, f.name.name, p.name.name));
                }
            }
        }
        let ret = self.functions[idx].ret;
        if f.asm.is_none() && !matches!(ret, Type::Void | Type::Error) && !terminates(&f.body) {
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
    /// `NOMBRE: Rom[u8] = [..]`: datos constantes en la memoria de programa (`docs/sdk-tablas.md`).
    /// `CLOCK: Internal` / `CLOCK: Crystal`...: la fuente del reloj; la frecuencia va en `FREQUENCY: 8MHz` (docs/sdk-reloj.md). Aqui solo se
    /// anota la declaracion: `resolve_clock_decls` junta las tres (`CLOCK`, `FREQUENCY`, `OSC_MODE`) cuando se conocen.
    fn declare_clock(&mut self, v: &VarDecl, kind: &str, is_global: bool) {
        if !is_global || v.name.name != "CLOCK" {
            self.d.error("PYP2190", v.name.span, "el reloj se declara a nivel de modulo y se llama CLOCK: 'CLOCK: Internal' o 'CLOCK: Crystal' (con 'FREQUENCY: 8MHz' en otra linea)");
            return;
        }
        if let Some(val) = &v.value {
            self.expr(val);
            self.d.error("PYP2190", val.span, "CLOCK: Internal no se asigna con '=': la frecuencia va en 'FREQUENCY: 8MHz'");
        }
        if self.clock_decl.is_some() {
            self.d.error("PYP2192", v.name.span, "CLOCK ya se declaro: una sola vez");
            return;
        }
        let Some(k) = ClockKind::parse(kind) else { return };
        self.clock_decl = Some((k, v.span));
    }

    /// `FREQUENCY: 8MHz`: la frecuencia de la fuente de reloj; cualquier expresion constante de frecuencia (`11.0592MHz`, `8 * MHz`, `16MHz / 2`).
    fn declare_frequency(&mut self, v: &VarDecl, e: &Expr, is_global: bool) {
        if !is_global {
            self.d.error("PYP2190", v.name.span, "FREQUENCY se declara a nivel de modulo, junto a CLOCK");
            return;
        }
        if let Some(val) = &v.value {
            self.expr(val);
            self.d.error("PYP2190", val.span, "FREQUENCY: 8MHz lleva el valor despues de los dos puntos, sin '='");
        }
        if self.frequency_decl.is_some() {
            self.d.error("PYP2192", v.name.span, "FREQUENCY ya se declaro: una sola vez");
            return;
        }
        if self.expr(e) == Type::Error {
            return;
        }
        self.consume_quantities(e);
        match self.dim(e) {
            Some(Dim::Frequency) => {}
            Some(other) => {
                self.d.error("PYP2232", e.span, format!("FREQUENCY es una frecuencia (Hz, kHz, MHz) y esto es {other}"));
                return;
            }
            None => {
                self.d.error("PYP2232", e.span, "FREQUENCY lleva la unidad: 'FREQUENCY: 8MHz' (un numero sin unidad no dice si son Hz o MHz)");
                return;
            }
        }
        let Some(hz) = self.konst(e).filter(|h| (1..=u32::MAX as i128).contains(h)) else {
            self.d.error("PYP2193", e.span, "la frecuencia es una constante mayor que 0 y menor que 4,29 GHz: se resuelve al compilar");
            return;
        };
        self.frequency_decl = Some((hz as u32, e.span));
    }

    /// `OSC_MODE: HS`: el modo del oscilador externo (`Auto`, o uno de los del dispositivo: `LP`, `XT`, `HS`, `EC`...).
    fn declare_osc_mode(&mut self, v: &VarDecl, mode: &str, is_global: bool) {
        if !is_global {
            self.d.error("PYP2190", v.name.span, "OSC_MODE se declara a nivel de modulo, junto a CLOCK");
            return;
        }
        if self.osc_mode_decl.is_some() {
            self.d.error("PYP2192", v.name.span, "OSC_MODE ya se declaro: una sola vez");
            return;
        }
        self.osc_mode_decl = Some((mode.to_string(), v.ann.span));
    }

    /// `PLL`, `CLOCK_DIVIDER`, `CLOCK_OUTPUT`, `FAIL_SAFE_CLOCK` y `CLOCK_SWITCHING`: ajustes complementarios del reloj.
    fn declare_clock_setting(&mut self, v: &VarDecl, word: Option<&str>, e: Option<&Expr>, is_global: bool) {
        let name = v.name.name.as_str();
        if !is_global {
            self.d.error("PYP2190", v.name.span, format!("{name} se declara a nivel de modulo, junto a CLOCK"));
            return;
        }
        if let Some(val) = &v.value {
            self.expr(val);
            self.d.error("PYP2190", val.span, format!("{name}: el valor va despues de los dos puntos, sin '='"));
        }
        if self.clock_setting_spans.insert(name.to_string(), v.span).is_some() {
            self.d.error("PYP2192", v.name.span, format!("{name} ya se declaro: una sola vez"));
            return;
        }
        let onoff = |w: Option<&str>| match w {
            Some("On") => Some(true),
            Some("Off") => Some(false),
            _ => None,
        };
        let number = |this: &mut Self, e: Option<&Expr>| -> Option<u32> {
            let e = e?;
            if this.expr(e) == Type::Error {
                return None;
            }
            match this.konst(e).filter(|n| (1..=u32::MAX as i128).contains(n)).filter(|_| this.dim(e).is_none()) {
                Some(n) => Some(n as u32),
                None => {
                    this.d.error("PYP2193", e.span, format!("{name} es un numero entero constante mayor que 0, sin unidad"));
                    None
                }
            }
        };
        match name {
            "PLL" => match word {
                Some("Off") => self.clock_settings.pll = Some(clock::Toggle::Off),
                Some("On") => self.clock_settings.pll = Some(clock::Toggle::On),
                Some(_) => self.d.error("PYP2190", v.ann.span, "PLL es Off, On o el multiplicador: 'PLL: 4'"),
                None => self.clock_settings.pll = number(self, e).map(clock::Toggle::Mult),
            },
            "CLOCK_DIVIDER" => match word {
                Some(_) => self.d.error("PYP2190", v.ann.span, "CLOCK_DIVIDER es el divisor: 'CLOCK_DIVIDER: 2'"),
                None => self.clock_settings.divider = number(self, e),
            },
            _ => {
                let Some(b) = onoff(word) else {
                    self.d.error("PYP2190", v.ann.span, format!("{name} es On u Off: '{name}: On'"));
                    return;
                };
                match name {
                    "CLOCK_OUTPUT" => self.clock_settings.output = Some(b),
                    "FAIL_SAFE_CLOCK" => self.clock_settings.fail_safe = Some(b),
                    _ => self.clock_settings.switching = Some(b),
                }
            }
        }
    }

    /// Junta `CLOCK`, `FREQUENCY` y `OSC_MODE`, los valida contra el perfil del dispositivo y aplica el reloj.
    fn resolve_clock_decls(&mut self) {
        if std::mem::replace(&mut self.clock_resolved, true) {
            return;
        }
        let (clock, freq, mode) = (self.clock_decl, self.frequency_decl, self.osc_mode_decl.clone());
        if clock.is_none() && freq.is_none() && mode.is_none() && self.clock_setting_spans.is_empty() {
            return;
        }
        if self.clock_hz.is_some() {
            let at = clock.map(|c| c.1).or(freq.map(|f| f.1)).or(mode.as_ref().map(|m| m.1)).unwrap_or(Span::new(0, 0));
            self.d.error("PYP2192", at, "el reloj ya se declaro con CLOCK_HZ: usa solo una de las dos formas (CLOCK + FREQUENCY, o CLOCK_HZ)");
            return;
        }
        let Some((kind, kspan)) = clock else {
            let at = freq.map(|f| f.1).or(mode.as_ref().map(|m| m.1)).or(self.clock_setting_spans.values().next().copied()).unwrap_or(Span::new(0, 0));
            self.d.error("PYP2190", at, "FREQUENCY, OSC_MODE, PLL y los demas ajustes acompanan a CLOCK: declara primero 'CLOCK: Internal' o 'CLOCK: Crystal'");
            return;
        };
        let profile = self.target.oscillator.clone().unwrap_or_else(clock::OscillatorProfile::pic16f887);
        let (hz, at) = match freq {
            Some((h, s)) => (h, s),
            None if kind == ClockKind::Internal && profile.reset_hz > 0 => (profile.reset_hz, kspan),
            None => {
                self.d.error("PYP2193", kspan, format!("CLOCK: {} necesita su frecuencia: 'FREQUENCY: 16MHz'", kind.name()));
                return;
            }
        };
        let src = match clock::resolve_with(&profile, kind, hz, mode.as_ref().map(|m| m.0.as_str()), &self.clock_settings) {
            Ok(s) => s,
            Err(msg) => {
                self.d.error("PYP2194", at, msg);
                return;
            }
        };
        if let Some(max) = self.target.max_clock_hz {
            if src.system_hz > max {
                self.d.error("PYP2094", at, format!("{} Hz (FOSC resultante) excede el reloj maximo de '{}' ({max} Hz)", src.system_hz, self.target.name));
            }
        }
        self.apply_clock_source(src, kspan);
    }

    /// Fija la frecuencia y la fuente del reloj y reserva los pines del oscilador externo.
    fn apply_clock_source(&mut self, src: ClockSource, span: Span) {
        for (pin, function) in &src.pins {
            self.claims.push(Claim { pin: pin.clone(), function: function.clone(), peripheral: format!("oscilador externo ({})", src.kind.name()), span, via_timer: false });
        }
        if !src.pins.is_empty() {
            let pins: Vec<String> = src.pins.iter().map(|(p, f)| format!("{p} ({f})")).collect();
            self.d.warning(
                "PYP3016",
                span,
                format!("CLOCK: {} ({}) ocupa {}: dejan de estar disponibles para cualquier otro uso del programa (importarlos como E/S es el error PYP2150)", src.kind.name(), clock::fmt_hz(src.hz), pins.join(" y ")),
            );
        }
        self.clock_hz = Some(src.system_hz);
        self.clock_source = Some(src);
    }

    /// `SOURCE_FREQ`, `FOSC` y `FCY`: constantes calculadas con el reloj efectivo, disponibles para el usuario y el SDK. Sin PLL ni
    /// divisores (fase siguiente), `FOSC` es la frecuencia de la fuente; `FCY` es `FOSC` entre los relojes por ciclo de instruccion.
    fn define_clock_constants(&mut self) {
        let Some(hz) = self.clock_hz.or(self.target.default_clock_hz) else { return };
        let per = self.target.clocks_per_cycle.unwrap_or(4).max(1);
        let source = self.clock_source.as_ref().map(|c| c.hz).unwrap_or(hz);
        for (name, v) in [("SOURCE_FREQ", source), ("FOSC", hz), ("FCY", hz / per)] {
            self.globals.entry(name.to_string()).or_insert(Symbol { name: name.to_string(), ty: U32, kind: SymKind::Global, konst: Some(v as i128), is_final: true });
        }
    }

    /// `buf: uint8[16]`: un arreglo de capacidad fija en RAM (docs/sdk-arreglos.md). Solo globales y solo `uint8`; empieza en cero
    /// (el arranque lo pone a cero, como al resto de las globales) y no se inicializa con valores.
    fn declare_array(&mut self, v: &VarDecl, elem: &TypeExpr, n: i128, is_global: bool) {
        let name = &v.name.name;
        if !is_global {
            self.d.error("PYP2170", v.span, format!("'{name}': un arreglo se declara a nivel de modulo, no dentro de una funcion (la RAM de los arreglos es fija)"));
        }
        match &elem.kind {
            TypeExprKind::Name(t) if Type::from_name(t) == Some(U8) => {}
            _ => self.d.error("PYP2170", elem.span, "los arreglos de RAM solo admiten uint8 por ahora: buf: uint8[16]"),
        }
        if !(1..=255).contains(&n) {
            self.d.error("PYP2171", v.ann.span, format!("la capacidad de un arreglo va de 1 a 255 (el indice es un uint8), '{name}' pide {n}"));
        }
        let mut init = Vec::new();
        if let Some(val) = &v.value {
            match &val.kind {
                ExprKind::List(items) => {
                    for it in items {
                        let t = self.expr(it);
                        match (t, self.konst(it)) {
                            (Type::Error, _) => {}
                            (_, Some(k)) if (0..=255).contains(&k) => init.push(k as u8),
                            (_, Some(k)) => self.d.error("PYP2160", it.span, format!("el valor {k} no cabe en un uint8 (0..=255)")),
                            (_, None) => self.d.error("PYP2160", it.span, "los valores iniciales de un arreglo son constantes: se escriben en el arranque, antes de main"),
                        }
                    }
                    if items.len() as i128 != n {
                        self.d.error(
                            "PYP2160",
                            val.span,
                            format!("'{name}' tiene {n} elementos y la lista trae {}: un inicializador da todos los valores (para empezar en cero, quita el '= ...')", items.len()),
                        );
                        init.clear();
                    }
                }
                _ => {
                    self.expr(val);
                    self.d.error("PYP2160", val.span, format!("'{name}' se inicializa con una lista de constantes: buf: uint8[3] = [1, 2, 3]"));
                }
            }
        }
        if self.globals.contains_key(name)
            || self.rom_idx.contains_key(name)
            || self.array_idx.contains_key(name)
            || self.fidx.contains_key(name)
            || self.pin_names.contains_key(name)
            || self.reg_names.contains_key(name)
            || self.timer_names.contains_key(name)
            || self.bits_fns.contains(name)
            || self.time_fns.contains_key(name)
        {
            self.d.error("PYP2009", v.name.span, format!("'{name}' ya esta declarado"));
            return;
        }
        self.array_idx.insert(name.clone(), self.arrays.len());
        self.arrays.push((name.clone(), n.clamp(1, 255) as usize));
        self.array_inits.push(init);
    }

    /// El indice de `buf[i]` / `buf[i] = v`: un `uint8` (o una constante dentro de la capacidad).
    fn array_index_ok(&mut self, obj: &str, ai: usize, index: &Expr) -> bool {
        let len = self.arrays[ai].1;
        let t = self.expr(index);
        match (t, self.konst(index)) {
            (Type::Error, _) => false,
            (_, Some(k)) if (0..len as i128).contains(&k) => {
                self.coerce_literal(index, U8);
                true
            }
            (_, Some(k)) => {
                self.d.error("PYP2161", index.span, format!("el indice {k} esta fuera del arreglo '{obj}' (tiene {len} elementos: 0..={})", len - 1));
                false
            }
            (Type::Int { bits: 8, signed: false }, None) => true,
            (other, None) => {
                self.d.error("PYP2163", index.span, format!("el indice de un arreglo es un uint8, no {other}: convierte con uint8(...)"));
                false
            }
        }
    }

    fn index_assign(&mut self, target: &Ident, index: &Expr, op: Option<BinOp>, value: &Expr, span: Span) {
        let name = &target.name;
        let Some(&ai) = self.array_idx.get(name) else {
            self.expr(index);
            self.expr(value);
            let msg = if self.rom_idx.contains_key(name) {
                format!("'{name}' es una tabla Rom de solo lectura: no se puede escribir en ella")
            } else if self.lookup(name).is_some() {
                format!("'{name}' no es un arreglo: solo se indexan los declarados como '{name}: uint8[N]'")
            } else {
                format!("'{name}' no esta declarado como arreglo")
            };
            self.d.error("PYP2162", target.span, msg);
            return;
        };
        let ok = self.array_index_ok(name, ai, index);
        match op {
            Some(BinOp::Shl | BinOp::Shr) => self.shift_amount(value, U8),
            _ => self.expect(value, U8, Some("los elementos de un arreglo son uint8")),
        }
        let _ = (ok, span);
    }

    fn declare_rom(&mut self, v: &VarDecl, inner: &TypeExpr, is_global: bool, is_final: bool) {
        let name = &v.name.name;
        // Rom ya es de solo lectura: `Const` no hace falta (se acepta `Const[Rom[u8]]`)
        let _ = is_final;
        if !is_global {
            self.d.error("PYP2160", v.span, format!("'{name}': una tabla Rom se declara a nivel de modulo, no dentro de una funcion"));
        }
        match &inner.kind {
            TypeExprKind::Name(n) if Type::from_name(n) == Some(U8) => {}
            _ => self.d.error("PYP2160", inner.span, "Rom solo admite uint8 por ahora: cada entrada es un RETLW"),
        }
        if self.globals.contains_key(name)
            || self.rom_idx.contains_key(name)
            || self.array_idx.contains_key(name)
            || self.fidx.contains_key(name)
            || self.pin_names.contains_key(name)
            || self.reg_names.contains_key(name)
            || self.timer_names.contains_key(name)
            || self.bits_fns.contains(name)
        {
            self.d.error("PYP2009", v.name.span, format!("'{name}' ya esta declarado"));
        }
        let Some(val) = &v.value else {
            self.d.error("PYP2007", v.span, format!("'{name}' debe inicializarse en su declaracion"));
            return;
        };
        let ExprKind::List(items) = &val.kind else {
            self.expr(val);
            self.d.error("PYP2160", val.span, format!("'{name}' se inicializa con una lista de constantes: [1, 2, 3]"));
            return;
        };
        if items.is_empty() || items.len() > 256 {
            self.d.error("PYP2160", val.span, format!("una tabla tiene de 1 a 256 entradas (el indice es un uint8), '{name}' tiene {}", items.len()));
        }
        let mut values = Vec::new();
        for it in items {
            let t = self.expr(it);
            match (t, self.konst(it)) {
                (Type::Error, _) => {}
                (_, Some(k)) if (0..=255).contains(&k) => values.push(k as u8),
                (_, Some(k)) => self.d.error("PYP2160", it.span, format!("la entrada {k} no cabe en un uint8 (0..=255)")),
                (_, None) => self.d.error("PYP2160", it.span, "las entradas de una tabla son constantes: se escriben en la memoria de programa al compilar"),
            }
        }
        self.rom_idx.insert(name.clone(), self.roms.len());
        self.roms.push((name.clone(), values));
    }

    /// `TABLA[i]`: lectura de costo fijo; un indice constante se resuelve al compilar.
    fn index_expr(&mut self, e: &Expr, obj: &str, obj_span: Span, index: &Expr) -> Type {
        if let Some(&ai) = self.array_idx.get(obj) {
            return if self.array_index_ok(obj, ai, index) {
                self.array_gets.insert(e.id, ai);
                self.set(e, U8, None)
            } else {
                self.set(e, Type::Error, None)
            };
        }
        let Some(&ti) = self.rom_idx.get(obj) else {
            self.expr(index);
            let msg = if self.lookup(obj).is_some() {
                format!("'{obj}' no es una tabla ni un arreglo: solo se indexan las 'Rom[uint8]' y los 'uint8[N]'")
            } else {
                format!("'{obj}' no esta declarada como tabla")
            };
            self.d.error("PYP2162", obj_span, msg);
            return self.set(e, Type::Error, None);
        };
        let len = self.roms[ti].1.len();
        let t = self.expr(index);
        match (t, self.konst(index)) {
            (Type::Error, _) => self.set(e, Type::Error, None),
            (_, Some(k)) if (0..len as i128).contains(&k) => {
                self.coerce_literal(index, U8);
                let v = self.roms[ti].1[k as usize] as i128;
                self.set(e, U8, Some(v))
            }
            (_, Some(k)) => {
                self.d.error("PYP2161", index.span, format!("el indice {k} esta fuera de la tabla '{obj}' (tiene {len} entradas: 0..={})", len - 1));
                self.set(e, Type::Error, None)
            }
            (Type::Int { bits: 8, signed: false }, None) => {
                self.rom_gets.insert(e.id, ti);
                if let Some(cur) = self.cur {
                    self.functions[cur].uses_rom = true;
                }
                self.set(e, U8, None)
            }
            (other, None) => {
                self.d.error("PYP2163", index.span, format!("el indice de una tabla es un uint8, no {other}: convierte con uint8(...)"));
                self.set(e, Type::Error, None)
            }
        }
    }

    fn call_depth(&self, f: &str, visiting: &mut HashSet<String>, memo: &mut HashMap<String, u32>) -> u32 {
        if let Some(&d) = memo.get(f) {
            return d;
        }
        let Some(&i) = self.fidx.get(f) else { return 0 };
        if !visiting.insert(f.to_string()) {
            return 0;
        }
        let calls: Vec<String> = self.functions[i].calls.iter().cloned().collect();
        let own = (self.functions[i].uses_rom as u32).max(self.functions[i].runtime_depth);
        let d = calls.iter().map(|c| 1 + self.call_depth(c, visiting, memo)).max().unwrap_or(0).max(own);
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
            if worst.as_ref().is_none_or(|(w, _)| u > *w) {
                worst = Some((u, r.clone()));
            }
        }
        let (main_use, main_root) = worst.unwrap_or((0, String::new()));
        // la entrada a la interrupcion empuja el PC (1 nivel) y la ISR puede llamar a otras
        let isr = self.functions.iter().find(|f| f.is_interrupt).map(|f| f.name.clone());
        let isr_use = isr.as_ref().map_or(0, |n| 1 + self.call_depth(n, &mut visiting, &mut memo));
        self.stack_use = Some((main_use, isr_use));
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
        // lo que sigue a una sentencia que no vuelve (return, break, continue, un if cuyas dos ramas no vuelven, un `while True` sin break) no se
        // ejecuta nunca: se avisa una vez por bloque en la primera sentencia inalcanzable (un `pass` suelto no cuenta) y se sigue comprobando
        let (mut why, mut warned) = (None, false);
        for s in stmts {
            // una etiqueta es un destino de GOTO: lo que sigue vuelve a ser alcanzable
            if matches!(s.kind, StmtKind::Label(_)) {
                (why, warned) = (None, false);
            }
            if let (Some(w), false) = (why, warned) {
                if !matches!(s.kind, StmtKind::Pass) {
                    self.d.warning("PYP3020", s.span, format!("codigo inalcanzable: {w}"));
                    warned = true;
                }
            }
            self.stmt(s);
            if why.is_none() {
                why = diverges(s);
            }
        }
    }

    fn lookup(&self, name: &str) -> Option<Symbol> {
        self.locals.get(name).or_else(|| self.globals.get(name)).cloned()
    }

    /// Cada sentencia reinicia el estado del acarreo: solo lo deja definido la que acaba de ejecutar
    /// `rol_c`, `ror_c`, `clear_carry` o `set_carry` como expresion completa (ver `docs/sdk-bits.md`).
    fn stmt(&mut self, s: &Stmt) {
        let same_fn = self.carry_fn == self.cur;
        self.carry_fn = self.cur;
        self.carry_prev = std::mem::replace(&mut self.carry_ok, false) && same_fn;
        self.stmt_top = match &s.kind {
            StmtKind::Assign { value, .. } => Some(value.id),
            StmtKind::Decl(v) => v.value.as_ref().map(|e| e.id),
            StmtKind::Expr(e) => Some(e.id),
            _ => None,
        };
        let top = self.stmt_top;
        self.stmt_inner(s);
        if top.and_then(|id| self.bits_calls.get(&id)).is_some_and(|c| c.defines_carry()) {
            self.carry_ok = true;
        }
    }

    fn stmt_inner(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Pass | StmtKind::Label(_) => {}
            StmtKind::Goto(target) => {
                if !self.fn_labels.contains(&target.name) {
                    self.d.error("PYP2230", target.span, format!("no hay una etiqueta '{}:' en esta funcion (un GOTO solo salta dentro de la funcion donde esta)", target.name));
                }
            }
            StmtKind::Decl(v) => self.declare_variable(v, false),
            StmtKind::Global(names) => {
                for n in names {
                    match self.globals.get(&n.name) {
                        None => self.d.error("PYP2020", n.span, format!("'{}' no es una variable global", n.name)),
                        Some(g) if g.is_final => self.d.error("PYP2021", n.span, format!("'{}' es constante (Const) y no puede modificarse", n.name)),
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
            StmtKind::IndexAssign { target, index, op, value } => self.index_assign(target, index, *op, value, s.span),
            StmtKind::AugAssign { target, op, value } => {
                // `x += 1` lee x: no cuenta como "nunca se lee" (un contador que solo se incrementa es un uso)
                self.read_names.insert(target.name.clone());
                let sym = self.assign_target(target);
                match sym {
                    Some(sym) if sym.ty.is_int() => {
                        if matches!(op, BinOp::Shl | BinOp::Shr) {
                            self.shift_amount(value, sym.ty);
                        } else {
                            self.expect(value, sym.ty, None);
                        }
                    }
                    Some(sym) if sym.ty.is_fixed() => {
                        if matches!(op, BinOp::Add | BinOp::Sub) {
                            self.expect(value, sym.ty, None);
                        } else {
                            self.d.error("PYP2253", s.span, format!("'{}=' no se aplica al punto fijo: escribe x = x {} y (solo += y -= son directos)", op.symbol(), op.symbol()));
                            self.expr(value);
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
        if self.array_idx.contains_key(&t.name) {
            self.d.error("PYP2172", t.span, format!("'{0}' es un arreglo: se escribe un elemento, '{0}[indice] = valor'", t.name));
            return None;
        }
        let Some(sym) = self.lookup(&t.name) else {
            self.d.error("PYP2022", t.span, format!("'{0}' no esta declarada; declarala con tipo: '{0}: uint8 = ...'", t.name));
            return None;
        };
        if sym.is_final {
            self.d.error("PYP2021", t.span, format!("'{}' es constante (Const) y no puede modificarse", t.name));
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
        let mut t = self.expr(e);
        if target.is_fixed() && t != Type::Error {
            match t {
                _ if t == target => {}
                Type::Literal | Type::Decimal => {
                    self.fixed_coerce(e, target);
                }
                Type::Fixed { .. } => self.d.error("PYP2254", e.span, msg.map(String::from).unwrap_or_else(|| format!("se esperaba {target}, se encontro {t}; convierte con {target}(...)"))),
                _ => self.d.error("PYP2254", e.span, msg.map(String::from).unwrap_or_else(|| format!("se esperaba {target}, se encontro {t}; convierte el entero con {target}(x)"))),
            }
            return;
        }
        // un decimal de valor entero (100 * 0.5 = 50) vale como entero donde se pide uno
        if t == Type::Decimal && target.is_int() {
            if let Some((n, 1)) = self.rats[e.id as usize] {
                t = self.set(e, Type::Literal, Some(n));
            }
        }
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
            let hint = if t.is_int() && target.is_int() {
                format!("; usa {target}(...) para convertir explicitamente")
            } else if t == Type::Decimal {
                "; un decimal no entero solo existe al compilar: multiplicalo hasta un entero (100 * 0.5) o usa un tipo de punto fijo".to_string()
            } else {
                String::new()
            };
            let m = msg.map(String::from).unwrap_or_else(|| format!("se esperaba {target}, se encontro {t}{hint}"));
            self.d.error("PYP2041", e.span, m);
        }
    }

    fn expr(&mut self, e: &Expr) -> Type {
        match &e.kind {
            ExprKind::Int(v) => self.set(e, Type::Literal, Some(*v)),
            ExprKind::Decimal(n, d) => self.set_rational(e, (*n, *d)),
            ExprKind::FixedCast(te, arg) => self.fixed_cast(e, te, arg),
            ExprKind::Quantity(v, dim) => {
                self.dims[e.id as usize] = Some(*dim);
                if !dim.is_plain_integer() {
                    self.quantity_pending.push((e.id, e.span, *dim));
                }
                self.set(e, Type::Literal, Some(*v))
            }
            ExprKind::Bool(b) => self.set(e, Type::Bool, Some(*b as i128)),
            ExprKind::Name(n) => self.name(e, n),
            ExprKind::Index { obj, obj_span, index } => self.index_expr(e, obj, *obj_span, index),
            ExprKind::Str(_) | ExprKind::FStr(_) => {
                self.d.error("PYP2164", e.span, "un texto entre comillas o una f-string solo se admite como argumento de UART.write_text (para el codigo de un caracter usa ord('A'))");
                self.set(e, Type::Error, None)
            }
            ExprKind::List(items) => {
                for it in items {
                    self.expr(it);
                }
                self.d.error("PYP1025", e.span, "una lista solo se admite como valor de una tabla: NOMBRE: Rom[uint8] = [1, 2, 3]");
                self.set(e, Type::Error, None)
            }
            ExprKind::Bool_(_, vals) => {
                for v in vals {
                    self.expect(v, Type::Bool, Some("los operandos de 'and'/'or' deben ser bool"));
                }
                self.set(e, Type::Bool, None)
            }
            ExprKind::Unary(op, inner) => {
                let t = self.unary(e, *op, inner);
                self.dims[e.id as usize] = self.dims[inner.id as usize];
                t
            }
            ExprKind::Binary(op, l, r) => {
                let t = self.binary(e, *op, l, r);
                self.combine_dims(e, *op, l, r);
                t
            }
            ExprKind::Compare(op, l, r) => self.compare(e, *op, l, r),
            ExprKind::Call { func, args, .. } => self.call(e, func, args),
            ExprKind::MethodCall { obj, obj_span, attr, method, args, kwargs } => self.method_call(e, obj, *obj_span, attr.as_ref(), method, args, kwargs),
        }
    }

    fn name(&mut self, e: &Expr, n: &str) -> Type {
        if let Some(&r) = self.decimal_consts.get(n) {
            self.read_names.insert(n.to_string());
            return self.set_rational(e, r);
        }
        if matches!(n, "SOURCE_FREQ" | "FOSC" | "FCY") && !self.globals.contains_key(n) {
            // el reloj se resuelve con lo declarado hasta aqui (CLOCK y FREQUENCY van al principio del programa)
            self.resolve_clock_decls();
            self.define_clock_constants();
        }
        if let Some(sym) = self.lookup(n) {
            self.read_names.insert(n.to_string());
            if sym.kind == SymKind::Global && matches!(n, "MHz" | "kHz" | "Hz") {
                self.dims[e.id as usize] = Some(Dim::Frequency);
            }
            return self.set(e, sym.ty, sym.konst);
        }
        if self.timer_consts.contains(n) {
            self.d.error("PYP2128", e.span, format!("'{n}' es una constante de configuracion de temporizadores: solo se usa como argumento de TMRx.start(...)"));
        } else if self.uart_name && n == "UART" {
            self.d.error("PYP2185", e.span, "'UART' es el puerto serie: usa sus metodos (UART.start(baud=9600), UART.write(x), UART.read()...)");
        } else if self.adc_name && n == "ADC" {
            self.d.error("PYP2175", e.span, "'ADC' es el convertidor: usa sus metodos (ADC.start(), ADC.read(AN0), ADC.result()...)");
        } else if self.adc_consts.contains(n) {
            self.d.error("PYP2175", e.span, format!("'{n}' es una constante del ADC: solo se usa como argumento de ADC.select/read o ADC.start(clock=...)"));
        } else if self.spi_name && n == "SPI" {
            self.d.error("PYP2236", e.span, "'SPI' es el bus del MSSP: usa sus metodos (SPI.start(freq=1000000), SPI.transfer(b))");
        } else if self.i2c_name && n == "I2C" {
            self.d.error("PYP2229", e.span, "'I2C' es el bus maestro del MSSP: usa sus metodos (I2C.start(freq=100000), I2C.begin(), I2C.write(b), I2C.end()...)");
        } else if self.eeprom_name && n == "EEPROM" {
            self.d.error("PYP2213", e.span, "'EEPROM' es la memoria de datos: usa sus metodos (EEPROM.read(a), EEPROM.write(a, v), EEPROM.busy()...)");
        } else if self.comp_names.contains(n) {
            self.d.error("PYP2217", e.span, format!("'{n}' es un comparador: usa sus metodos ({n}.start(negative=C12IN0, positive=REF_PIN), {n}.out(), {n}.stop())"));
        } else if self.comp_consts.contains(n) {
            self.d.error("PYP2217", e.span, format!("'{n}' es una constante de los comparadores: solo se usa como argumento de CMPx.start(negative=..., positive=...)"));
        } else if self.cvref_name && n == "CVREF" {
            self.d.error("PYP2223", e.span, "'CVREF' es la referencia de tension: usa sus metodos (CVREF.start(step=8), CVREF.set(n), CVREF.stop())");
        } else if self.pwm_names.contains(n) {
            self.d.error("PYP2205", e.span, format!("'{n}' es un modulo PWM: usa sus metodos ({n}.start(frequency=1000), {n}.duty(v), {n}.stop())"));
        } else if self.array_idx.contains_key(n) {
            self.d.error("PYP2172", e.span, format!("'{n}' es un arreglo: se lee con {n}[indice], se escribe con {n}[indice] = valor y su tamano es len({n})"));
        } else if self.rom_idx.contains_key(n) {
            self.d.error("PYP2164", e.span, format!("'{n}' es una tabla: se lee con {n}[indice] y su tamano con len({n})"));
        } else if self.reg_names.contains_key(n) {
            self.d.error("PYP2136", e.span, format!("'{n}' es un registro: usa sus metodos ({n}.read(), {n}.write(v), {n}.BIT.set()...)"));
        } else if self.timer_names.contains_key(n) {
            self.d.error("PYP2129", e.span, format!("'{n}' es un temporizador: usa sus metodos ({n}.start(...), {n}.value(), {n}.expired()...)"));
        } else if self.pin_names.contains_key(n) {
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
        if t.is_fixed() {
            if op == UnOp::Neg && t.is_signed() {
                let k = self.konst(inner).map(|c| -c).filter(|c| t.contains(*c));
                return self.set(e, t, k);
            }
            if op == UnOp::Pos {
                let k = self.konst(inner);
                return self.set(e, t, k);
            }
            self.d.error("PYP2044", e.span, format!("operador unario no aplicable a {t}"));
            return self.set(e, Type::Error, None);
        }
        if t == Type::Decimal && matches!(op, UnOp::Neg | UnOp::Pos) {
            let (n, d) = self.rats[inner.id as usize].unwrap();
            return self.set_rational(e, if op == UnOp::Neg { (-n, d) } else { (n, d) });
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
                    self.d.error("PYP2044", e.span, "'~' requiere un operando con tipo explicito (p. ej. ~uint8(5))");
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
        if lt.is_fixed() || rt.is_fixed() {
            return self.fixed_unify(l, r, lt, rt, op);
        }
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
                    self.d.error("PYP2049", l.span, "el operando izquierdo de un desplazamiento necesita tipo explicito (p. ej. uint8(1) << n)");
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
        if lt.is_fixed() || rt.is_fixed() {
            return match op {
                BinOp::Mul | BinOp::Div | BinOp::TrueDiv => self.fixed_muldiv(e, op, l, r, lt, rt),
                BinOp::Add | BinOp::Sub => self.fixed_addsub(e, op, l, r, lt, rt),
                _ => {
                    self.d.error("PYP2253", e.span, format!("el operador '{sym}' no se aplica al punto fijo (admite + - * / y las comparaciones)"));
                    self.set(e, Type::Error, None)
                }
            };
        }
        if op == BinOp::TrueDiv && lt != Type::Decimal && rt != Type::Decimal {
            self.d.error("PYP1022", e.span, "'/' solo divide decimales al compilar (1.0 / 3); para enteros usa '//' (division entera)");
            return self.set(e, Type::Error, None);
        }
        if lt == Type::Decimal || rt == Type::Decimal {
            return self.decimal_binary(e, op, l, r);
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
        self.note_runtime(e, op, t, a, b);
        self.set(e, t, None)
    }

    /// `*`, `//` y `%` que no se resuelven con un desplazamiento o una mascara usan una rutina nativa: pila y aviso.
    fn note_runtime(&mut self, e: &Expr, op: BinOp, t: Type, a: Option<i128>, b: Option<i128>) {
        let Some(kind) = crate::runtime::routine_for(op, t, a, b) else { return };
        if let Some(cur) = self.cur {
            let f = &mut self.functions[cur];
            f.runtime_depth = f.runtime_depth.max(kind.stack_levels());
        }
        let name = kind.name(t.bits());
        if self.rt_warned.insert(name.clone()) {
            self.d.warning(
                "PYP3011",
                e.span,
                format!(
                    "'{}' sobre {t} se compila a la rutina nativa {name} (una copia por contexto si la usan main y la interrupcion; {} nivel(es) de pila; costo en ciclos en docs/sdk-runtime.md)",
                    op.symbol(),
                    kind.stack_levels()
                ),
            );
        }
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

    /// `pyxpic.bits`: `swap`, `rol_c`, `ror_c`, `clear_carry`, `set_carry`, `carry`, `bit_set`, `bit_clear`, `bit_test`.
    fn bits_call(&mut self, e: &Expr, func: &str, args: &[Expr]) -> Type {
        let want = match func {
            "swap" | "rol_c" | "ror_c" => 1,
            "bit_set" | "bit_clear" | "bit_test" => 2,
            _ => 0,
        };
        if args.len() != want {
            self.d.error("PYP2141", e.span, format!("'{func}' recibe {want} argumento(s), recibio {}", args.len()));
            for a in args {
                self.expr(a);
            }
            return self.set(e, Type::Error, None);
        }
        // un argumento que debe ser una variable (no una expresion): el resultado se lee o se modifica en su sitio
        let var = |this: &mut Self, a: &Expr, modifies: bool| -> Option<Type> {
            let ExprKind::Name(n) = &a.kind else {
                this.expr(a);
                this.d.error("PYP2143", a.span, format!("el argumento de '{func}' debe ser una variable, no una expresion: se lee o se modifica en su sitio"));
                return None;
            };
            let Some(sym) = this.lookup(n) else {
                this.expr(a);
                return None;
            };
            if sym.is_final || sym.konst.is_some() {
                this.d.error("PYP2143", a.span, format!("'{n}' es una constante: '{func}' necesita una variable"));
                return None;
            }
            if modifies && sym.kind == SymKind::Global && !this.global_decls.contains(n) {
                this.d.error("PYP2023", a.span, format!("para modificar la global '{n}' declara 'global {n}' en la funcion"));
            }
            this.set(a, sym.ty, None);
            Some(sym.ty)
        };
        let top = self.stmt_top == Some(e.id);
        let op = match func {
            "swap" => {
                self.expect(&args[0], U8, Some("swap recibe un uint8"));
                self.bits_calls.insert(e.id, BitsOp::Swap);
                return self.set(e, U8, None);
            }
            "rol_c" | "ror_c" => {
                let ty = var(self, &args[0], false);
                if ty.is_some_and(|t| t != U8) {
                    self.d.error("PYP2141", args[0].span, format!("{func} rota un uint8 ({}): para mas ancho, encadena una rotacion por byte", ty.unwrap()));
                }
                if !top {
                    self.d.error("PYP2142", e.span, format!("{func} debe ser la expresion completa de la sentencia ('y = {func}(x)'): el acarreo se pierde con cualquier otra operacion"));
                }
                if !self.carry_prev {
                    self.d.error(
                        "PYP2142",
                        e.span,
                        format!("{func} usa el acarreo de entrada y la sentencia anterior no lo deja definido: ponlo con clear_carry()/set_carry() o encadena otra rotacion justo antes"),
                    );
                }
                if func == "rol_c" {
                    BitsOp::RolC
                } else {
                    BitsOp::RorC
                }
            }
            "clear_carry" | "set_carry" => {
                if !top {
                    self.d.error("PYP2142", e.span, format!("{func}() se escribe como sentencia propia"));
                }
                if func == "clear_carry" {
                    BitsOp::ClearCarry
                } else {
                    BitsOp::SetCarry
                }
            }
            "carry" => {
                if !top {
                    self.d.error("PYP2142", e.span, "carry() debe ser la expresion completa de la sentencia ('c = carry()')");
                }
                if !self.carry_prev {
                    self.d.error("PYP2142", e.span, "carry() lee la bandera C justo despues de rol_c, ror_c, clear_carry o set_carry: la sentencia anterior no la deja definida");
                }
                self.bits_calls.insert(e.id, BitsOp::Carry);
                return self.set(e, Type::Bool, None);
            }
            _ => {
                let modifies = func != "bit_test";
                let ty = var(self, &args[0], modifies);
                let bit = self.expr(&args[1]);
                let k = self.konst(&args[1]);
                let bits = ty.filter(|t| t.is_int()).map(|t| t.bits());
                if let (Some(t), None) = (ty, bits) {
                    self.d.error("PYP2141", args[0].span, format!("{func} trabaja sobre un entero, no sobre {t}"));
                } else if bits.is_some_and(|b| b > 16) {
                    self.d.error("PYP2141", args[0].span, format!("{func} trabaja sobre enteros de hasta 16 bits"));
                }
                let op_of = |n: u8| match func {
                    "bit_set" => BitsOp::BitSet(n),
                    "bit_clear" => BitsOp::BitClear(n),
                    _ => BitsOp::BitTest(n),
                };
                match (bit, k) {
                    (Type::Error, _) => {}
                    (_, Some(n)) if bits.is_some_and(|b| (0..b as i128).contains(&n)) => {
                        self.set(&args[1], U8, k);
                        self.bits_calls.insert(e.id, op_of(n as u8));
                    }
                    (_, Some(n)) if bits.is_some() => self.d.error("PYP2144", args[1].span, format!("el bit {n} no existe en un entero de {} bits (0..={})", bits.unwrap(), bits.unwrap() - 1)),
                    (_, None) => self.d.error("PYP2144", args[1].span, format!("el numero de bit de {func} debe ser una constante: se codifica en la instruccion")),
                    _ => {}
                }
                return if func == "bit_test" { self.set(e, Type::Bool, None) } else { self.set(e, Type::Void, None) };
            }
        };
        self.bits_calls.insert(e.id, op);
        match op {
            BitsOp::RolC | BitsOp::RorC => self.set(e, U8, None),
            _ => self.set(e, Type::Void, None),
        }
    }

    /// `parse_dec(buf, n, error)` y `parse_hex(buf, n, error)` -> uint16 (docs/sdk-texto.md): texto de un arreglo `uint8[N]` a numero.
    /// `error` es una variable `bool` que se nombra: se escribe `True` si el texto no es un numero valido (y el valor es 0) y `False` si lo es.
    fn parse_call(&mut self, e: &Expr, func: &str, args: &[Expr]) -> Type {
        if args.len() != 3 {
            for a in args {
                self.expr_unless_array(a);
            }
            self.d.error("PYP2055", e.span, format!("'{func}' se escribe {func}(buf, n, error): el arreglo, cuantos bytes leer y la variable bool donde se anota el error"));
            return self.set(e, Type::Error, None);
        }
        let mut ok = true;
        let arr = match &args[0].kind {
            ExprKind::Name(n) if self.array_idx.contains_key(n) => Some(self.array_idx[n]),
            _ => {
                self.expr_unless_array(&args[0]);
                self.d.error("PYP2189", args[0].span, format!("{func} lee el texto de un arreglo uint8[N] (buf: uint8[16]) y se le pasa su nombre"));
                None
            }
        };
        self.expect(&args[1], U8, Some("n, cuantos bytes leer, es un uint8"));
        match &args[2].kind {
            ExprKind::Name(name) => {
                let id = Ident { name: name.clone(), span: args[2].span };
                match self.assign_target(&id) {
                    Some(sym) if sym.ty == Type::Bool => {}
                    Some(sym) => {
                        if sym.ty != Type::Error {
                            self.d.error("PYP2189", args[2].span, format!("'{name}' es {}: el error se anota en una variable bool (error: bool = False)", sym.ty));
                        }
                        ok = false;
                    }
                    None => ok = false,
                }
            }
            _ => {
                self.expr(&args[2]);
                self.d.error("PYP2189", args[2].span, format!("el tercer argumento de {func} es el nombre de una variable bool donde se anota el error (error: bool = False)"));
                ok = false;
            }
        }
        let (Some(arr), true) = (arr, ok) else { return self.set(e, Type::Error, None) };
        let op = if func == "parse_dec" { TextOp::ParseDec } else { TextOp::ParseHex };
        self.text_calls.insert(e.id, TextCall { op, arr });
        self.set(e, U16, None)
    }

    /// `ord('A')` -> 65, como en Python (docs/sdk-texto.md): el codigo ASCII de un caracter escrito en el programa.
    /// Es una constante: se resuelve al compilar y no genera codigo.
    fn ord_call(&mut self, e: &Expr, args: &[Expr]) -> Type {
        if let [Expr { kind: ExprKind::Str(raw), .. }] = args {
            if let Ok([byte]) = uart::decode_text(raw).as_deref() {
                let value = *byte as i128;
                return self.set(e, Type::Literal, Some(value));
            }
        } else {
            for a in args {
                self.expr(a);
            }
        }
        self.d.error("PYP2232", e.span, "ord() recibe un solo caracter ASCII entre comillas: ord('A') es 65 (se resuelve al compilar; un byte recibido ya es su codigo y se compara directo)");
        self.set(e, Type::Error, None)
    }

    fn call(&mut self, e: &Expr, func: &str, args: &[Expr]) -> Type {
        if func == "len" && !self.fidx.contains_key("len") {
            return match args {
                [a] if matches!(&a.kind, ExprKind::Name(n) if self.rom_idx.contains_key(n)) => {
                    let ExprKind::Name(n) = &a.kind else { unreachable!() };
                    let len = self.roms[self.rom_idx[n]].1.len() as i128;
                    self.set(e, Type::Literal, Some(len))
                }
                [a] if matches!(&a.kind, ExprKind::Name(n) if self.array_idx.contains_key(n)) => {
                    let ExprKind::Name(n) = &a.kind else { unreachable!() };
                    let len = self.arrays[self.array_idx[n]].1 as i128;
                    self.set(e, Type::Literal, Some(len))
                }
                _ => {
                    for a in args {
                        self.expr(a);
                    }
                    self.d.error("PYP2162", e.span, "len() solo mide una tabla o un arreglo: len(TABLA)");
                    self.set(e, Type::Error, None)
                }
            };
        }
        if (func == "parse_dec" || func == "parse_hex") && !self.fidx.contains_key(func) {
            return self.parse_call(e, func, args);
        }
        if func == "ord" && !self.fidx.contains_key("ord") {
            return self.ord_call(e, args);
        }
        if self.bits_fns.contains(func) {
            return self.bits_call(e, func, args);
        }
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
            let hint = if time_unit(func).is_some() { format!("; importala con 'from pyxpic.system.time import {func}'") } else { String::new() };
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
            t if t.is_fixed() => {
                // la parte entera (hacia cero con Fixed, hacia abajo con QFixed): uint8(temperatura)
                if let Some(c) = self.konst(a) {
                    let v = if matches!(t, Type::Fixed { binary: true, .. }) && !t.is_signed() { c / t.factor() } else { c / t.factor() };
                    return self.set(e, target, Some(target.wrap(v)));
                }
                let signed = t.is_signed() || target.is_signed();
                if !fits_wide(t, 1, signed) {
                    self.d.error("PYP2255", e.span, format!("convertir {t} a {target} no cabe en el intermedio de 16 bits: {WIDE_MSG}"));
                    return self.set(e, Type::Error, None);
                }
                self.fixed_ops.insert(e.id, FixedOp::Scale { num: 1, den: t.factor(), child: 0, signed });
                self.set(e, target, None)
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
/// `pyxpic.periph.*`: hardware integrado del chip (validado por el Device Pack).
const PERIPH_MODULES: &[&str] = &["gpio", "adc", "timer", "pwm", "ccp", "eccp", "uart", "spi", "i2c", "comparator", "vref", "eeprom", "latch"];
/// `pyxpic.system.*`: CPU, reloj y energia.
const SYSTEM_MODULES: &[&str] = &["time", "irq", "wdt", "clock", "power", "flash"];
/// `pyxpic.soft.*`: implementaciones por software sobre cualquier GPIO (cuestan ciclos de CPU).
const SOFT_MODULES: &[&str] = &["spi", "i2c", "uart", "onewire"];
const IMPLEMENTED_MODULES: &[&str] = &[
    "pyxpic.periph.uart",
    "pyxpic.periph.adc",
    "pyxpic.periph.pwm",
    "pyxpic.periph.eeprom",
    "pyxpic.periph.i2c",
    "pyxpic.periph.spi",
    "pyxpic.periph.comparator",
    "pyxpic.periph.vref",
    "pyxpic.periph.gpio",
    "pyxpic.periph.timer",
    "pyxpic.regs",
    "pyxpic.bits",
    "pyxpic.system.time",
];

/// Problema con la ruta de un modulo `pyxpic.*` (renombrado, categoria sin modulo, inexistente o aun no implementado).
/// `None` si es un modulo implementado o no es del arbol de categorias.
fn module_problem(module: &str) -> Option<(&'static str, String)> {
    for (old, new) in [("pyxpic.gpio", "pyxpic.periph.gpio"), ("pyxpic.time", "pyxpic.system.time")] {
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
        let pkg = format!("pyxpic.{cat}");
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

/// Reune las etiquetas de una funcion (en cualquier bloque anidado); una repetida es PYP2231.
fn collect_labels(stmts: &[Stmt], out: &mut HashSet<String>, d: &mut Diagnostics) {
    for s in stmts {
        match &s.kind {
            StmtKind::Label(l) => {
                if !out.insert(l.name.clone()) {
                    d.error("PYP2231", l.span, format!("la etiqueta '{}:' ya existe en esta funcion", l.name));
                }
            }
            StmtKind::If { body, orelse, .. } => {
                collect_labels(body, out, d);
                collect_labels(orelse, out, d);
            }
            StmtKind::While { body, .. } | StmtKind::For { body, .. } => collect_labels(body, out, d),
            _ => {}
        }
    }
}

/// Analisis simple de "todos los caminos terminan en return".
fn terminates(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match &s.kind {
        StmtKind::Return(_) | StmtKind::Goto(_) => true,
        StmtKind::If { body, orelse, .. } => !orelse.is_empty() && terminates(body) && terminates(orelse),
        StmtKind::While { test, body } => matches!(test.kind, ExprKind::Bool(true)) && !has_break(body),
        _ => false,
    })
}

/// Por que lo que sigue a esta sentencia no se ejecuta, si es que no se ejecuta.
fn diverges(s: &Stmt) -> Option<&'static str> {
    match &s.kind {
        StmtKind::Return(_) => Some("sigue a un return"),
        StmtKind::Break => Some("sigue a un break"),
        StmtKind::Continue => Some("sigue a un continue"),
        StmtKind::Goto(_) => Some("sigue a un goto"),
        StmtKind::If { body, orelse, .. } if !orelse.is_empty() && body.iter().any(|b| diverges(b).is_some()) && orelse.iter().any(|b| diverges(b).is_some()) => {
            Some("las dos ramas del if terminan (return, break o continue)")
        }
        StmtKind::While { test, body } if matches!(test.kind, ExprKind::Bool(true)) && !has_break(body) => Some("sigue a un 'while True' sin break: no termina nunca"),
        _ => None,
    }
}

fn has_break(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match &s.kind {
        StmtKind::Break => true,
        StmtKind::If { body, orelse, .. } => has_break(body) || has_break(orelse),
        _ => false, // los break de bucles anidados no cuentan
    })
}
