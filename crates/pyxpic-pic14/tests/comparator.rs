//! Comparadores y CVREF (`pyxpic.periph.comparator` y `pyxpic.periph.vref`, #45): el nivel de `CMPx.out()` contra el interprete de referencia
//! para todas las entradas y referencias, los registros que escribe `start`, las esperas de asentamiento, la polaridad, CVREF (ecuacion 8-1),
//! la bandera de interrupcion y las reglas del lenguaje.

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_frontend::analyze;
use pyxpic_ir::{lower, verify, Machine};
use pyxpic_pic14::codegen::Compiled;
use pyxpic_pic14::sim::Sim;
use pyxpic_pic14::{compile, Options};

fn pack() -> DevicePack {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap()
}

fn head() -> String {
    "from pyxpic.device import PIC16F887\nfrom pyxpic.periph.comparator import CMP1, CMP2, C12IN0, C12IN1, C12IN2, C12IN3, REF_PIN, REF_CVREF, REF_FIXED\nfrom pyxpic.periph.vref import CVREF\nCLOCK: Internal
FREQUENCY: 4MHz\n".to_string()
}

fn diagnostics(src: &str) -> (bool, String) {
    let r = analyze(src, &pack().target_profile());
    (r.program.is_some(), r.diagnostics.render(src, "t.py"))
}

fn build(src: &str) -> (pyxpic_ir::Module, Compiled) {
    let r = analyze(src, &pack().target_profile());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    let c = compile(&m, &Options::default()).unwrap_or_else(|e| panic!("codegen: {e}\n{src}"));
    (m, c)
}

fn prog(globals: &str, body: &str) -> String {
    format!("{}{globals}\n\n\ndef main() -> None:\n{body}    while True:\n        pass\n", head())
}

fn global(c: &Compiled, s: &Sim, name: &str) -> u8 {
    let g = c.globals.iter().find(|g| g.name == name).unwrap_or_else(|| panic!("no hay la global {name}"));
    s.read(g.addr as usize)
}

const CM1CON0: usize = 0x107;
const CM2CON0: usize = 0x108;
const CM2CON1: usize = 0x109;
const VRCON: usize = 0x97;
const SRCON: usize = 0x185;
const PIR2: usize = 0x0D;

/// Corre `src` en el interprete y en el simulador con las mismas entradas analogicas y devuelve el valor de la global `o` en los dos.
fn both(src: &str, inputs: &[(usize, u16)], o: &str) -> (u8, u8) {
    let (m, c) = build(src);
    let mut mach = Machine::new(&m);
    for &(ch, v) in inputs {
        mach.adc_inputs[ch] = v as i128;
    }
    let _ = mach.run_main();
    let mut s = Sim::new(&c.image);
    for &(ch, v) in inputs {
        s.adc_inputs[ch] = v;
    }
    s.run_until(2_000, |_| false);
    let g = m.globals.iter().position(|g| g.name == o).unwrap();
    (mach.globals[g] as u8, global(&c, &s, o))
}

// ---- el nivel contra el interprete de referencia ------------------------------------------------------------------------------

#[test]
fn pin_reference_matches_the_reference_interpreter_for_every_negative_input() {
    // AN0, AN1, AN9 y AN10 son C12IN0-..C12IN3-; C1IN+ = AN3 y C2IN+ = AN2
    for (name, an) in [("C12IN0", 0usize), ("C12IN1", 1), ("C12IN2", 9), ("C12IN3", 10)] {
        for (pos, neg, want) in [(800u16, 300u16, 1u8), (300, 800, 0), (500, 500, 0)] {
            let src = prog("o: uint8 = 9\n", &format!("    global o\n    CMP1.start(negative={name}, positive=REF_PIN)\n    if CMP1.out():\n        o = 1\n    else:\n        o = 0\n"));
            let (i, s) = both(&src, &[(3, pos), (an, neg)], "o");
            assert_eq!((i, s), (want, want), "C1 con {name}: + = {pos}, - = {neg}");
            let src = prog("o: uint8 = 9\n", &format!("    global o\n    CMP2.start(negative={name}, positive=REF_PIN)\n    if CMP2.out():\n        o = 1\n    else:\n        o = 0\n"));
            let (i, s) = both(&src, &[(2, pos), (an, neg)], "o");
            assert_eq!((i, s), (want, want), "C2 con {name}: + = {pos}, - = {neg}");
        }
    }
}

#[test]
fn invert_flips_the_output() {
    for (pos, neg) in [(800u16, 300u16), (300, 800)] {
        let plain = prog("o: uint8 = 9\n", "    global o\n    CMP1.start(negative=C12IN0, positive=REF_PIN)\n    if CMP1.out():\n        o = 1\n    else:\n        o = 0\n");
        let inv = prog("o: uint8 = 9\n", "    global o\n    CMP1.start(negative=C12IN0, positive=REF_PIN, invert=True)\n    if CMP1.out():\n        o = 1\n    else:\n        o = 0\n");
        let (a, b) = (both(&plain, &[(3, pos), (0, neg)], "o"), both(&inv, &[(3, pos), (0, neg)], "o"));
        assert_eq!(a.0, a.1);
        assert_eq!(b.0, b.1);
        assert_eq!(a.0, 1 - b.0, "+ = {pos}, - = {neg}");
    }
}

#[test]
fn cvref_reference_follows_equation_8_1_in_both_ranges() {
    // alto (VRR = 0): VDD/4 + VR/32 x VDD; bajo (VRR = 1): VR/24 x VDD; con la entrada - a la mitad de VDD (511,5 cuentas)
    for low in [false, true] {
        for step in 0..16u8 {
            let counts = pyxpic_frontend::comparator::cvref_counts(step, low);
            let want = (counts > 511.0) as u8;
            let lr = if low { "True" } else { "False" };
            let src = prog(
                "o: uint8 = 9\n",
                &format!(
                    "    global o\n    CVREF.start(step={step}, low_range={lr})\n    CMP1.start(negative=C12IN0, positive=REF_CVREF)\n    if CMP1.out():\n        o = 1\n    else:\n        o = 0\n"
                ),
            );
            let (i, s) = both(&src, &[(0, 511)], "o");
            assert_eq!((i, s), (want, want), "paso {step}, rango bajo = {low}: CVREF = {counts:.1} cuentas contra 511");
        }
    }
}

#[test]
fn cvref_set_changes_the_step_and_keeps_the_range() {
    let src = prog(
        "a: uint8 = 9\nb: uint8 = 9\n",
        "    global a, b\n    CVREF.start(step=0, low_range=True)\n    CMP1.start(negative=C12IN0, positive=REF_CVREF)\n    if CMP1.out():\n        a = 1\n    else:\n        a = 0\n    CVREF.set(15)\n    if CMP1.out():\n        b = 1\n    else:\n        b = 0\n",
    );
    // rango bajo: paso 0 = 0 V (menor que 400) y paso 15 = 15/24 de VDD = 639 cuentas (mayor que 400)
    let (m, c) = build(&src);
    let mut mach = Machine::new(&m);
    mach.adc_inputs[0] = 400;
    let _ = mach.run_main();
    let mut s = Sim::new(&c.image);
    s.adc_inputs[0] = 400;
    s.run_until(3_000, |_| false);
    assert_eq!((global(&c, &s, "a"), global(&c, &s, "b")), (0, 1));
    for (n, want) in [("a", 0), ("b", 1)] {
        let g = m.globals.iter().position(|g| g.name == n).unwrap();
        assert_eq!(mach.globals[g] as u8, want, "interprete: {n}");
    }
    assert_eq!(s.read(VRCON), 0xA0 | 15, "VREN, VRR y VR = 15; VRSS y VROE en 0");
}

#[test]
fn fixed_reference_is_0_6_volts() {
    // con VDD = 5 V la referencia fija es 122,76 cuentas: una entrada - de 100 queda por debajo y una de 150 por encima
    for (neg, want) in [(100u16, 1u8), (150, 0)] {
        let src = prog("o: uint8 = 9\n", "    global o\n    CMP1.start(negative=C12IN0, positive=REF_FIXED)\n    if CMP1.out():\n        o = 1\n    else:\n        o = 0\n");
        assert_eq!(both(&src, &[(0, neg)], "o"), (want, want), "- = {neg}");
    }
}

#[test]
fn a_stopped_comparator_reads_its_polarity() {
    // CxON = 0 manda un 0 al XOR: CxOUT = CxPOL (8.2.1 y figura 8-2)
    for (inv, want) in [("False", 0u8), ("True", 1)] {
        let src = prog(
            "o: uint8 = 9\n",
            &format!("    global o\n    CMP1.start(negative=C12IN0, positive=REF_PIN, invert={inv})\n    CMP1.stop()\n    if CMP1.out():\n        o = 1\n    else:\n        o = 0\n"),
        );
        assert_eq!(both(&src, &[(3, 900), (0, 100)], "o"), (want, want), "invert = {inv}");
    }
}

// ---- registros, esperas y bandera -----------------------------------------------------------------------------------------------

#[test]
fn start_writes_the_documented_registers() {
    /// (llamada, registro CMxCON0, valor esperado, otro registro: direccion, mascara y valor)
    type Case = (&'static str, usize, u8, Option<(usize, u8, u8)>);
    let cases: &[Case] = &[
        // (llamada, registro CMxCON0, valor esperado, (otro registro, mascara, valor))
        ("CMP1.start(negative=C12IN2, positive=REF_PIN)", CM1CON0, 0x80 | 2, None),
        ("CMP1.start(negative=C12IN3, positive=REF_PIN, invert=True)", CM1CON0, 0x80 | 0x10 | 3, None),
        ("CMP1.start(negative=C12IN1, positive=REF_CVREF)", CM1CON0, 0x80 | 0x04 | 1, Some((CM2CON1, 0x20, 0x20))),
        ("CMP1.start(negative=C12IN0, positive=REF_FIXED)", CM1CON0, 0x80 | 0x04, Some((SRCON, 0x01, 0x01))),
        ("CMP2.start(negative=C12IN0, positive=REF_CVREF, invert=True)", CM2CON0, 0x80 | 0x10 | 0x04, Some((CM2CON1, 0x10, 0x10))),
    ];
    for &(call, reg, want, other) in cases {
        let src = prog("", &format!("    CVREF.start(step=3)\n    {call}\n"));
        let (_, c) = build(&src);
        let mut s = Sim::new(&c.image);
        s.run_until(2_000, |_| false);
        // CxOUT (bit 6) depende de las entradas (todas en 0): se enmascara
        assert_eq!(s.read(reg) & !0x40, want, "{call}");
        if let Some((r, mask, v)) = other {
            assert_eq!(s.read(r) & mask, v, "{call}: registro {r:#x}");
        }
    }
}

#[test]
fn start_enables_the_analog_inputs_and_clears_the_flag() {
    let src = prog("", "    CMP1.start(negative=C12IN2, positive=REF_PIN)\n");
    let (_, c) = build(&src);
    let mut s = Sim::new(&c.image);
    s.write(0x188, 0); // ANSEL en digital
    s.write(0x189, 0); // ANSELH en digital
    s.write(0x85, 0); // TRISA salida
    s.write(0x86, 0); // TRISB salida
                      // la entrada + (RA3/AN3) en 900 y la - (RB3/AN9) en 100: la salida sube a 1 al encender y CxIF se levanta; start lo baja al final
    s.adc_inputs[3] = 900;
    s.adc_inputs[9] = 100;
    s.run_until(2_000, |_| false);
    assert_eq!(s.read(0x188) & 0x08, 0x08, "ANS3 (RA3 = C1IN+) analogico");
    assert_eq!(s.read(0x189) & 0x02, 0x02, "ANS9 (RB3 = C12IN2-) analogico");
    assert_eq!(s.read(0x85) & 0x08, 0x08, "RA3 como entrada");
    assert_eq!(s.read(0x86) & 0x08, 0x08, "RB3 como entrada");
    assert_eq!(s.read(CM1CON0) & 0x40, 0x40, "C1OUT = 1");
    assert_eq!(s.read(CM2CON1) & 0x80, 0x80, "MC1OUT copia C1OUT");
    assert_eq!(s.read(PIR2) & 0x20, 0, "C1IF queda en 0 tras start");
}

#[test]
fn a_later_change_of_the_output_raises_the_interrupt_flag() {
    let src = prog("", "    CMP1.start(negative=C12IN0, positive=REF_PIN)\n");
    let (_, c) = build(&src);
    let mut s = Sim::new(&c.image);
    s.adc_inputs[3] = 100;
    s.adc_inputs[0] = 500;
    s.run_until(2_000, |_| false);
    assert_eq!((s.read(CM1CON0) & 0x40, s.read(PIR2) & 0x20), (0, 0));
    s.adc_inputs[3] = 900; // + sube por encima de -
    s.run_until(s.cycles + 10, |_| false);
    assert_eq!((s.read(CM1CON0) & 0x40, s.read(PIR2) & 0x20), (0x40, 0x20), "cambio de nivel: C1IF = 1");
}

#[test]
fn settling_waits_follow_the_datasheet() {
    // CM05 (10 us), CV04 (10 us) y VR02 (100 us) a 4 MHz = 10, 10 y 100 ciclos de instruccion mas las instrucciones de los registros
    let measure = |body: &str| -> u64 {
        let (_, c) = build(&prog("", body));
        let mut s = Sim::new(&c.image);
        let mut last_pc = u16::MAX;
        let mut n = 0u64;
        // ciclos hasta que el PC deja de avanzar (el `while True: pass` final salta a si mismo)
        loop {
            let pc = s.pc;
            s.step();
            if pc == last_pc || n > 20_000 {
                break;
            }
            last_pc = pc;
            n += 1;
        }
        s.cycles
    };
    let base = measure("    pass\n");
    let cmp_pin = measure("    CMP1.start(negative=C12IN0, positive=REF_PIN)\n");
    let cmp_fixed = measure("    CMP1.start(negative=C12IN0, positive=REF_FIXED)\n");
    let cv = measure("    CVREF.start(step=4)\n");
    // lo que no es espera: pines (analog_input), registros y la lectura final; la diferencia entre la referencia fija y el pin es la espera de 100 us
    assert!(cmp_pin >= base + 10, "el cambio de modo espera 10 us (10 ciclos): {cmp_pin} contra {base}");
    assert!(cv >= base + 10 && cv < base + 40, "CVREF.start espera 10 us y escribe un registro: {cv} contra {base}");
    assert!(cmp_fixed >= cmp_pin + 85, "la referencia fija espera 100 us en vez de 10: {cmp_fixed} contra {cmp_pin}");
}

// ---- reglas del lenguaje ------------------------------------------------------------------------------------------------------

fn expect_error(body: &str, code: &str) {
    let src = prog("", body);
    let (ok, d) = diagnostics(&src);
    assert!(!ok && d.contains(code), "se esperaba {code} con:\n{body}\n{d}");
}

#[test]
fn language_rules() {
    expect_error("    CMP1.start(negative=C12IN0)\n", "PYP2215"); // falta positive
    expect_error("    CMP1.start(positive=REF_PIN)\n", "PYP2215"); // falta negative
    expect_error("    CMP1.start(C12IN0, REF_PIN)\n", "PYP2215"); // solo con nombre
    expect_error("    CMP1.start(negative=REF_PIN, positive=REF_PIN)\n", "PYP2215");
    expect_error("    CMP1.start(negative=C12IN0, positive=C12IN1)\n", "PYP2215");
    expect_error("    CMP1.start(negative=C12IN0, positive=REF_PIN, invert=1)\n", "PYP2215");
    expect_error("    CMP1.start(negative=C12IN0, positive=REF_PIN, speed=3)\n", "PYP2215");
    expect_error("    CMP1.nope()\n", "PYP2216");
    expect_error("    x: bool = CMP1\n", "PYP2217");
    expect_error("    x: bool = C12IN0\n", "PYP2217");
    expect_error("    CMP1.start(negative=C12IN0, positive=REF_CVREF)\n", "PYP2218"); // sin CVREF.start
    expect_error("    CVREF.start(step=16)\n", "PYP2221");
    expect_error("    CVREF.start()\n", "PYP2221");
    expect_error("    CVREF.start(step=1, low_range=3)\n", "PYP2221");
    expect_error("    CVREF.set(16)\n", "PYP2221");
    expect_error("    CVREF.nope()\n", "PYP2222");
    expect_error("    x: bool = CVREF\n", "PYP2223");
}

#[test]
fn an_unknown_name_is_rejected_at_the_import() {
    let src = "from pyxpic.device import PIC16F887\nfrom pyxpic.periph.comparator import CMP3\nCLOCK: Internal
FREQUENCY: 4MHz\n\n\ndef main() -> None:\n    while True:\n        pass\n";
    let (ok, d) = diagnostics(src);
    assert!(!ok && d.contains("PYP2214"), "{d}");
    let src = "from pyxpic.device import PIC16F887\nfrom pyxpic.periph.vref import VREF\nCLOCK: Internal
FREQUENCY: 4MHz\n\n\ndef main() -> None:\n    while True:\n        pass\n";
    let (ok, d) = diagnostics(src);
    assert!(!ok && d.contains("PYP2220"), "{d}");
}

#[test]
fn a_comparator_input_cannot_also_be_a_digital_pin() {
    // RA0 es C12IN0-: usarlo a la vez como pin de E/S choca (PYP2150)
    let src = "from pyxpic.device import PIC16F887\nfrom pyxpic.periph.gpio import RA0\nfrom pyxpic.periph.comparator import CMP1, C12IN0, REF_FIXED\nCLOCK: Internal
FREQUENCY: 4MHz\n\n\ndef main() -> None:\n    RA0.output()\n    CMP1.start(negative=C12IN0, positive=REF_FIXED)\n    while True:\n        pass\n";
    let (ok, d) = diagnostics(src);
    assert!(!ok && d.contains("PYP2150"), "{d}");
}

#[test]
fn calling_out_without_using_the_result_emits_nothing() {
    let (m, _) = build(&prog("", "    CMP1.start(negative=C12IN0, positive=REF_FIXED)\n    CMP1.out()\n"));
    let n: usize = m.functions.iter().flat_map(|f| f.blocks.iter()).flat_map(|b| b.insts.iter()).filter(|i| matches!(i, pyxpic_ir::ir::Inst::Comp { call, .. } if call.returns_value())).count();
    assert_eq!(n, 0);
}
