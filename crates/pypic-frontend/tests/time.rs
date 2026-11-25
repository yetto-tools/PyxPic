//! `from pypic.system.time import pause, wait, delay_us, delay_cycles`: reglas del frontend.

use pypic_frontend::{analyze, TargetProfile};

fn generic() -> TargetProfile {
    TargetProfile::generic()
}

/// Perfil como el de un Device Pack: reloj por defecto 4 MHz, 4 relojes por ciclo, maximo 20 MHz.
fn pic() -> TargetProfile {
    TargetProfile::new("PIC16F887", 8, 32).with_device("PIC16F887", 20_000_000).with_clocks_per_cycle(4).with_default_clock(4_000_000)
}

fn codes(src: &str, t: &TargetProfile) -> Vec<&'static str> {
    analyze(src, t).diagnostics.codes()
}

const HEAD: &str = "from pypic.device import PIC16F887\nfrom pypic.system.time import pause, wait, delay_us, delay_cycles\nCLOCK_HZ: Const[u32] = 4_000_000\n";

fn body(stmt: &str) -> String {
    format!("{HEAD}def main() -> None:\n    {stmt}\n")
}

#[test]
fn valid_calls() {
    for s in ["pause(500)", "wait(2)", "delay_us(100)", "delay_cycles(12345)", "delay_cycles(0)", "pause(0)"] {
        assert!(analyze(&body(s), &pic()).program.is_some(), "{s}");
    }
    let var = format!("{HEAD}n: u16 = 5\ndef main() -> None:\n    pause(n)\n    wait(n)\n");
    assert!(analyze(&var, &pic()).program.is_some());
    let konst = format!("{HEAD}K: Const[u16] = 100\ndef main() -> None:\n    delay_cycles(K)\n    delay_us(K)\n");
    assert!(analyze(&konst, &pic()).program.is_some());
}

// ---- una sola forma por operacion ----------------------------------------------------------------------

#[test]
fn there_is_one_name_per_operation() {
    assert_eq!(codes("from pypic.system.time import delay_ms\n", &generic()), vec!["PYP2104"]);
    assert_eq!(codes("from pypic.system.time import delay\n", &generic()), vec!["PYP2104"]);
    let out = analyze("from pypic.system.time import delay_ms\n", &generic()).diagnostics.render("from pypic.system.time import delay_ms\n", "t.py");
    assert!(out.contains("pause") && out.contains("wait") && out.contains("delay_us"), "{out}");
}

#[test]
fn nap_and_sleep_are_reserved_and_distinct_not_aliases() {
    assert_eq!(codes("from pypic.system.time import nap\n", &generic()), vec!["PYP2103"]);
    assert_eq!(codes("from pypic.system.time import sleep\n", &generic()), vec!["PYP2103"]);
    let src = "from pypic.system.time import nap, sleep\n";
    assert_eq!(codes(src, &generic()), vec!["PYP2103", "PYP2103"]); // dos errores, uno por nombre
    assert_eq!(codes("from pypic.system.time import bogus\n", &generic()), vec!["PYP1002"]);
}

#[test]
fn must_be_imported_with_a_hint() {
    let src = "from pypic.device import PIC16F887\nCLOCK_HZ: Const[u32] = 4_000_000\ndef main() -> None:\n    pause(5)\n";
    assert_eq!(codes(src, &pic()), vec!["PYP2043"]);
    let out = analyze(src, &pic()).diagnostics.render(src, "t.py");
    assert!(out.contains("from pypic.system.time import pause"), "{out}");
}

// ---- argumentos --------------------------------------------------------------------------------------------

#[test]
fn microseconds_and_cycles_need_a_constant() {
    let var = format!("{HEAD}n: u16 = 5\ndef main() -> None:\n    delay_us(n)\n");
    assert!(codes(&var, &pic()).contains(&"PYP2100")); // (u16 en 8 bits da tambien el aviso PYP3001)
    let var = format!("{HEAD}n: u16 = 5\ndef main() -> None:\n    delay_cycles(n)\n");
    assert!(codes(&var, &pic()).contains(&"PYP2100"));
}

#[test]
fn ms_and_seconds_take_a_u16_or_a_constant() {
    let u8var = format!("{HEAD}x: u8 = 5\ndef main() -> None:\n    pause(x)\n");
    assert_eq!(codes(&u8var, &pic()), vec!["PYP2041"]); // sin ensanchamiento implicito
    assert_eq!(codes(&body("pause(70000)"), &pic()), vec!["PYP2040"]); // no cabe en u16
    assert_eq!(codes(&body("delay_us(-1)"), &pic()), vec!["PYP2040"]);
    assert_eq!(codes(&body("pause()"), &pic()), vec!["PYP2055"]);
    assert_eq!(codes(&body("pause(1, 2)"), &pic()), vec!["PYP2055"]);
    assert_eq!(codes(&body("_x: u8 = pause(5)"), &pic()), vec!["PYP2041"]); // devuelve None
}

#[test]
fn names_cannot_clash_with_imported_waits() {
    let src = "from pypic.system.time import pause\ndef pause() -> None:\n    pass\n";
    assert!(codes(src, &generic()).contains(&"PYP2009"));
}

// ---- reloj ---------------------------------------------------------------------------------------------------

#[test]
fn without_clock_hz_the_device_default_is_used_with_a_warning() {
    let src = "from pypic.device import PIC16F887\nfrom pypic.system.time import pause\ndef main() -> None:\n    pause(1)\n";
    let r = analyze(src, &pic());
    assert_eq!(r.diagnostics.codes(), vec!["PYP3004"]);
    let p = r.program.unwrap();
    assert_eq!((p.clock_hz, p.clock_defaulted), (Some(4_000_000), true));
    let out = analyze(src, &pic()).diagnostics.render(src, "t.py");
    assert!(out.contains("warning[PYP3004]") && out.contains("4000000") && out.contains("CLOCK: Internal[4 * MHz]"), "{out}");
}

#[test]
fn without_clock_hz_and_without_a_device_default_it_is_an_error() {
    let src = "from pypic.system.time import pause\ndef main() -> None:\n    pause(1)\n";
    assert_eq!(codes(src, &generic()), vec!["PYP2101"]);
}

#[test]
fn a_declared_clock_has_no_default_warning_and_a_different_one_warns_about_the_oscillator() {
    let same = body("pause(1)");
    assert!(analyze(&same, &pic()).diagnostics.items.is_empty());
    let p = analyze(&same, &pic()).program.unwrap();
    assert_eq!((p.clock_hz, p.clock_defaulted), (Some(4_000_000), false));
    // 8 MHz declarado, pero la configuracion por defecto produce 4 MHz: el compilador aun no programa el oscilador
    let eight = same.replace("4_000_000", "8_000_000");
    let r = analyze(&eight, &pic());
    assert_eq!(r.diagnostics.codes(), vec!["PYP3005"]);
    assert!(r.program.is_some());
    // sin esperas, el reloj no importa para esta advertencia
    let none = "from pypic.device import PIC16F887\nCLOCK_HZ: Const[u32] = 8_000_000\ndef main() -> None:\n    pass\n";
    assert!(analyze(none, &pic()).diagnostics.items.is_empty());
}

// ---- interrupcion y precision --------------------------------------------------------------------------------

#[test]
fn waits_are_not_allowed_in_the_interrupt() {
    let direct = format!("{HEAD}@interrupt\ndef isr() -> None:\n    pause(1)\ndef main() -> None:\n    pass\n");
    assert_eq!(codes(&direct, &pic()), vec!["PYP2102"]);
    let indirect = format!("{HEAD}def helper() -> None:\n    delay_us(5)\n@interrupt\ndef isr() -> None:\n    helper()\ndef main() -> None:\n    pass\n");
    assert_eq!(codes(&indirect, &pic()), vec!["PYP2102"]);
    // en main (y en lo que solo main llama) si
    let ok = format!("{HEAD}def helper() -> None:\n    pause(1)\n@interrupt\ndef isr() -> None:\n    pass\ndef main() -> None:\n    helper()\n");
    assert!(analyze(&ok, &pic()).program.is_some());
}

#[test]
fn inexact_times_warn_with_the_rounding_error() {
    // 32.768 kHz: un ciclo = 122.07 us; 1 us no cabe en un ciclo
    let src = body("delay_us(1)").replace("4_000_000", "32_768");
    let out = analyze(&src, &pic()).diagnostics.render(&src, "t.py");
    assert!(out.contains("warning[PYP3003]") && out.contains("se redondea a 0 ciclos") && out.contains("-1000.0 ns"), "{out}");
    assert!(codes(&src, &pic()).contains(&"PYP3003"));
    // exactos: sin PYP3003
    for s in ["pause(500)", "delay_us(1)", "delay_us(37)", "wait(3)"] {
        assert!(!codes(&body(s), &pic()).contains(&"PYP3003"), "{s}");
    }
    // cantidad variable a un reloj en el que 1 ms no es un numero entero de ciclos: un aviso por unidad
    let var = format!("{HEAD}n: u16 = 5\ndef main() -> None:\n    pause(n)\n    pause(n)\n").replace("4_000_000", "32_768");
    assert_eq!(codes(&var, &pic()).iter().filter(|c| **c == "PYP3002").count(), 1);
}
