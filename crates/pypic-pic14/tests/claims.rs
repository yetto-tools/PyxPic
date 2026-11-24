//! Pines que un periferico encendido toma (#54): `peripherals.json` dice que bit reclama que funcion.

use std::path::PathBuf;

use pypic_device::DevicePack;
use pypic_frontend::{analyze, TargetProfile};

fn target() -> TargetProfile {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap().target_profile()
}

const HEAD: &str = "from pypic.device import PIC16F887\nCLOCK_HZ: Final[u32] = 4_000_000\n";

/// (compila, diagnosticos con su codigo y mensaje)
fn diag(body: &str) -> (bool, Vec<String>) {
    let src = format!("{HEAD}{body}");
    let r = analyze(&src, &target());
    let text = r.diagnostics.render(&src, "t.py");
    let lines = text.lines().filter(|l| l.contains("error[") || l.contains("warning[")).map(String::from).collect();
    (r.program.is_some(), lines)
}

fn has(lines: &[String], code: &str, needle: &str) -> bool {
    lines.iter().any(|l| l.contains(code) && l.contains(needle))
}

#[test]
fn a_pin_taken_by_the_eusart_cannot_also_be_digital_io() {
    // SPEN enciende el EUSART y RC6/RC7 pasan a ser TX y RX
    let (ok, d) = diag("from pypic.regs import RCSTA\nfrom pypic.periph.gpio import RC6\ndef main() -> None:\n    RCSTA.SPEN.set()\n    RC6.output()\n    RC6.on()\n");
    assert!(!ok && has(&d, "PYP2150", "RC6") && has(&d, "PYP2150", "TX") && has(&d, "PYP2150", "EUSART"), "{d:?}");
}

#[test]
fn writing_a_constant_that_sets_the_enable_bit_counts_too() {
    let (ok, d) = diag("from pypic.regs import RCSTA\nfrom pypic.periph.gpio import RC7\ndef main() -> None:\n    RCSTA.write(0x90)\n    RC7.input()\n");
    assert!(!ok && has(&d, "PYP2150", "RC7") && has(&d, "PYP2150", "RX"), "{d:?}");
    // un valor sin el bit SPEN no reclama nada
    let (ok, d) = diag("from pypic.regs import RCSTA\nfrom pypic.periph.gpio import RC7\ndef main() -> None:\n    RCSTA.write(0x10)\n    RC7.input()\n");
    assert!(ok, "{d:?}");
}

#[test]
fn the_mssp_takes_scl_and_sda_but_leaves_ss_alone() {
    let (ok, d) = diag("from pypic.regs import SSPCON\nfrom pypic.periph.gpio import RC3\ndef main() -> None:\n    SSPCON.SSPEN.set()\n    RC3.output()\n");
    assert!(!ok && has(&d, "PYP2150", "RC3") && has(&d, "PYP2150", "MSSP"), "{d:?}");
    // RA5/SS puede ser E/S en maestro
    let (ok, d) = diag("from pypic.regs import SSPCON\nfrom pypic.periph.gpio import RA5\ndef main() -> None:\n    SSPCON.SSPEN.set()\n    RA5.output()\n");
    assert!(ok, "{d:?}");
}

#[test]
fn an_unrelated_pin_and_a_peripheral_that_is_never_enabled_are_fine() {
    let (ok, d) = diag("from pypic.regs import RCSTA\nfrom pypic.periph.gpio import RD0\ndef main() -> None:\n    RCSTA.SPEN.set()\n    RD0.output()\n    RD0.on()\n");
    assert!(ok, "{d:?}");
    // RC6 importado pero el EUSART nunca se enciende: nada que reclamar
    let (ok, d) = diag("from pypic.periph.gpio import RC6\ndef main() -> None:\n    RC6.output()\n");
    assert!(ok, "{d:?}");
}

#[test]
fn the_external_clock_bit_of_timer0_takes_ra4() {
    let (ok, d) = diag("from pypic.regs import OPTION_REG\nfrom pypic.periph.gpio import RA4\ndef main() -> None:\n    OPTION_REG.T0CS.set()\n    RA4.input()\n");
    assert!(!ok && has(&d, "PYP2150", "RA4") && has(&d, "PYP2150", "T0CKI") && has(&d, "PYP2150", "TIMER0"), "{d:?}");
}

#[test]
fn two_peripherals_on_the_same_pin_warn() {
    // RA4 es T0CKI (Timer0) y C1OUT (comparador 1): el programa pide las dos
    let (ok, d) = diag("from pypic.regs import CM1CON0\nfrom pypic.periph.timer import TMR0, T0CKI\ndef main() -> None:\n    TMR0.start(prescaler=1, source=T0CKI)\n    CM1CON0.C1OE.set()\n");
    assert!(ok, "es un aviso, no un error: {d:?}");
    assert!(has(&d, "PYP3010", "RA4") && has(&d, "PYP3010", "TIMER0") && has(&d, "PYP3010", "COMPARATORS"), "{d:?}");
    // y por registros las dos
    let (ok, d) = diag("from pypic.regs import CM1CON0, OPTION_REG\ndef main() -> None:\n    OPTION_REG.T0CS.set()\n    CM1CON0.C1OE.set()\n");
    assert!(ok && has(&d, "PYP3010", "RA4"), "{d:?}");
    // el mismo periferico dos veces no es conflicto
    let (ok, d) = diag("from pypic.regs import OPTION_REG\ndef main() -> None:\n    OPTION_REG.T0CS.set()\n    OPTION_REG.T0CS.set()\n");
    assert!(ok && d.is_empty(), "{d:?}");
}

#[test]
fn timer_pins_keep_their_existing_diagnostic_without_a_duplicate() {
    let (ok, d) = diag("from pypic.periph.gpio import RA4\nfrom pypic.periph.timer import TMR0, T0CKI\ndef main() -> None:\n    TMR0.start(prescaler=1, source=T0CKI)\n    RA4.input()\n");
    assert!(!ok && has(&d, "PYP2125", "RA4") && !d.iter().any(|l| l.contains("PYP2150")), "{d:?}");
}

#[test]
fn the_comparator_output_takes_the_pin_of_its_function() {
    let (ok, d) = diag("from pypic.regs import CM2CON0\nfrom pypic.periph.gpio import RA5\ndef main() -> None:\n    CM2CON0.C2OE.set()\n    RA5.output()\n");
    assert!(!ok && has(&d, "PYP2150", "C2OUT"), "{d:?}");
}
