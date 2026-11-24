//! Declaracion inicial del dispositivo, CLOCK_HZ y la funcion @init_device.

use pypic_frontend::{analyze, declared_device, TargetProfile};

fn generic() -> TargetProfile {
    TargetProfile::generic()
}

/// Perfil como el que construye un Device Pack: nombre del dispositivo y reloj maximo.
fn pic16f887() -> TargetProfile {
    TargetProfile::new("PIC16F887", 8, 32).with_device("PIC16F887", 20_000_000)
}

fn codes(src: &str, t: &TargetProfile) -> Vec<&'static str> {
    analyze(src, t).diagnostics.codes()
}

const MAIN: &str = "def main() -> None:\n    pass\n";

// ---- declaracion del dispositivo -----------------------------------------------------------------

#[test]
fn declared_device_is_found_without_analysing_the_program() {
    assert_eq!(declared_device("from pypic.device import PIC16F887\n"), Some("PIC16F887".to_string()));
    assert_eq!(declared_device("\"\"\"doc\"\"\"\nfrom pypic.device import PIC12F625\ndef main() -> None:\n    pass\n"), Some("PIC12F625".to_string()));
    assert_eq!(declared_device("def main() -> None:\n    pass\n"), None);
    assert_eq!(declared_device("from pypic.periph.gpio import RD0\n"), None);
    // tolera errores en el resto del programa
    assert_eq!(declared_device("from pypic.device import PIC16F887\nx: = \n"), Some("PIC16F887".to_string()));
}

#[test]
fn a_valid_declaration_is_accepted_and_recorded() {
    let src = format!("from pypic.device import PIC16F887\n{MAIN}");
    let r = analyze(&src, &pic16f887());
    assert_eq!(r.program.unwrap().device.as_deref(), Some("PIC16F887"));
    assert!(analyze(&src, &generic()).program.is_some()); // sin pack tambien (solo se valida el formato del nombre)
    // el nombre se compara sin distinguir mayusculas
    assert!(analyze(&src.replace("PIC16F887", "pic16f887"), &pic16f887()).program.is_some());
}

#[test]
fn declaring_a_different_device_than_the_one_compiled_for_is_an_error() {
    let src = format!("from pypic.device import PIC12F625\n{MAIN}");
    assert_eq!(codes(&src, &pic16f887()), vec!["PYP2090"]);
    let out = analyze(&src, &pic16f887()).diagnostics.render(&src, "t.py");
    assert!(out.contains("PIC12F625") && out.contains("PIC16F887"), "{out}");
}

#[test]
fn the_declaration_must_be_the_first_line_and_unique() {
    let late = format!("from pypic import u8\nfrom pypic.device import PIC16F887\n{MAIN}");
    assert_eq!(codes(&late, &pic16f887()), vec!["PYP2092"]);
    let twice = format!("from pypic.device import PIC16F887\nfrom pypic.device import PIC16F887\n{MAIN}");
    assert_eq!(codes(&twice, &pic16f887()), vec!["PYP2091"]);
    let two_names = format!("from pypic.device import PIC16F887, PIC12F625\n{MAIN}");
    assert_eq!(codes(&two_names, &pic16f887()), vec!["PYP2092"]);
}

#[test]
fn something_that_is_not_a_pic_name_is_rejected_without_a_pack() {
    assert_eq!(codes(&format!("from pypic.device import Arduino\n{MAIN}"), &generic()), vec!["PYP2096"]);
    assert!(analyze(&format!("from pypic.device import dsPIC33FJ128GP802\n{MAIN}"), &generic()).program.is_some());
}

// ---- CLOCK_HZ ---------------------------------------------------------------------------------------

#[test]
fn clock_hz_is_a_final_u32_checked_against_the_device_maximum() {
    let ok = format!("from pypic.device import PIC16F887\nCLOCK_HZ: Final[u32] = 20_000_000\n{MAIN}");
    let p = analyze(&ok, &pic16f887()).program.unwrap();
    assert_eq!(p.clock_hz, Some(20_000_000));
    let too_fast = ok.replace("20_000_000", "20_000_001");
    assert_eq!(codes(&too_fast, &pic16f887()), vec!["PYP2094"]);
    // sin pack no hay maximo que comprobar
    assert_eq!(analyze(&too_fast, &generic()).program.unwrap().clock_hz, Some(20_000_001));
}

#[test]
fn clock_hz_has_a_fixed_shape() {
    for bad in [
        "CLOCK_HZ: u32 = 4_000_000\n",            // no es Final
        "CLOCK_HZ: Final[u16] = 4000\n",           // no es u32
        "CLOCK_HZ: Final[u32] = 0\n",              // 0 Hz
    ] {
        assert_eq!(codes(&format!("{bad}{MAIN}"), &generic()), vec!["PYP2093"], "{bad}");
    }
}

#[test]
fn final_constants_do_not_trigger_the_emulated_type_warning() {
    // u32 en un destino de 8 bits: una constante no cuesta nada en el MCU, una variable si
    let konst = format!("K: Final[u32] = 4_000_000\n{MAIN}");
    assert!(analyze(&konst, &pic16f887()).diagnostics.items.is_empty());
    let var = format!("v: u32 = 5\n{MAIN}");
    assert_eq!(codes(&var, &pic16f887()), vec!["PYP3001"]);
}

// ---- @init_device ---------------------------------------------------------------------------------------

#[test]
fn init_device_is_marked_by_a_decorator_and_any_name_works() {
    let src = "from pypic.periph.gpio import RD0\n@init_device\ndef configurar() -> None:\n    RD0.output()\ndef main() -> None:\n    RD0.on()\n";
    let p = analyze(src, &generic()).program.unwrap();
    assert!(p.functions.iter().any(|f| f.name == "configurar" && f.is_init));
    assert!(p.functions.iter().any(|f| f.name == "main" && !f.is_init));
}

#[test]
fn init_device_rules() {
    assert_eq!(codes("@init_device\ndef f(a: u8) -> None:\n    pass\n", &generic()), vec!["PYP2097"]); // sin parametros
    assert_eq!(codes("@init_device\ndef f() -> u8:\n    return 1\n", &generic()), vec!["PYP2097"]); // sin retorno
    assert_eq!(codes("@init_device\n@interrupt\ndef f() -> None:\n    pass\n", &generic()), vec!["PYP2097"]); // no a la vez que @interrupt
    let two = "@init_device\ndef a() -> None:\n    pass\n@init_device\ndef b() -> None:\n    pass\n";
    assert_eq!(codes(two, &generic()), vec!["PYP2099"]);
    let call = "@init_device\ndef a() -> None:\n    pass\ndef main() -> None:\n    a()\n";
    assert_eq!(codes(call, &generic()), vec!["PYP2098"]); // la ejecuta el arranque, no el codigo
    assert!(codes("@bogus\ndef f() -> None:\n    pass\n", &generic()).contains(&"PYP1004"));
}

#[test]
fn init_device_counts_toward_stack_depth_and_isr_sharing() {
    // init_device (1 + 2 llamadas) es la rama mas profunda: 3 niveles con una pila de 3 -> cabe; con 2, no
    let src = "def leaf() -> None:\n    pass\ndef mid() -> None:\n    leaf()\n@init_device\ndef setup() -> None:\n    mid()\ndef main() -> None:\n    pass\n";
    let t3 = TargetProfile::new("t", 8, 32).with_stack_levels(3);
    let t2 = TargetProfile::new("t", 8, 32).with_stack_levels(2);
    assert!(analyze(src, &t3).program.is_some());
    assert_eq!(codes(src, &t2), vec!["PYP2061"]);
    // una funcion alcanzable desde init_device y desde la ISR se comparte entre contextos
    let shared = "def helper() -> None:\n    pass\n@init_device\ndef setup() -> None:\n    helper()\n@interrupt\ndef isr() -> None:\n    helper()\ndef main() -> None:\n    pass\n";
    assert_eq!(codes(shared, &generic()), vec!["PYP2060"]);
}
