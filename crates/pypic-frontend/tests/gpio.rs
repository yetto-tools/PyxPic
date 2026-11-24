//! Acceso a pines: `from pypic.periph.gpio import RD0` y metodos output/input/on/off/toggle/value.

use pypic_frontend::{analyze, TargetProfile};

fn codes(src: &str) -> Vec<&'static str> {
    analyze(src, &TargetProfile::generic()).diagnostics.codes()
}

fn ok(src: &str) {
    let r = analyze(src, &TargetProfile::generic());
    assert!(r.program.is_some(), "esperaba ok:\n{}", r.diagnostics.render(src, "t.py"));
}

fn err(src: &str, code: &str) {
    let c = codes(src);
    assert!(c.contains(&code), "esperaba {code}, hubo {c:?}\n{src}");
}

const HEAD: &str = "from pypic.periph.gpio import RD0, RB0\n";

#[test]
fn valid_gpio_program() {
    ok("from pypic.periph.gpio import RD0, RB0\n\
        def main() -> None:\n    RD0.output()\n    RB0.input()\n    while True:\n        RD0.toggle()\n        if RB0.value() == 1:\n            RD0.off()\n        else:\n            RD0.on()\n");
}

#[test]
fn value_returns_u8_and_accepts_u8_bool_or_literal() {
    ok(&format!("{HEAD}x: u8 = 0\ndef main() -> None:\n    global x\n    RB0.input()\n    RD0.output()\n    x = RB0.value()\n    RD0.value(x)\n    RD0.value(1)\n    RD0.value(True)\n"));
    err(&format!("{HEAD}def main() -> None:\n    RB0.input()\n    b: bool = RB0.value()\n"), "PYP2041"); // u8 no es bool
    err(&format!("{HEAD}def main() -> None:\n    RD0.output()\n    RD0.value(u16(1))\n"), "PYP2041");
    err(&format!("{HEAD}def main() -> None:\n    RD0.output()\n    x: u8 = RD0.on()\n"), "PYP2041"); // None no es u8
}

#[test]
fn pins_must_be_imported_and_exist() {
    err("def main() -> None:\n    RD0.output()\n", "PYP2078");
    err("from pypic.periph.gpio import LED\ndef main() -> None:\n    pass\n", "PYP2074");
    err("from pypic.periph.gpio import RD0\nfrom pypic.foo import x\n", "PYP1002");
    err("from pypic.periph.gpio import RD0\ndef main() -> None:\n    RB0.output()\n", "PYP2078"); // RB0 no importado
}

#[test]
fn a_pin_must_be_configured_before_use_anywhere_in_the_program() {
    err(&format!("{HEAD}def main() -> None:\n    RD0.on()\n"), "PYP2079");
    err(&format!("{HEAD}def main() -> None:\n    RD0.toggle()\n"), "PYP2079");
    err(&format!("{HEAD}def main() -> None:\n    x: u8 = RD0.value()\n"), "PYP2081");
    // solo como entrada: no se puede escribir
    err(&format!("{HEAD}def main() -> None:\n    RD0.input()\n    RD0.on()\n"), "PYP2079");
    // la configuracion puede estar en otra funcion (analisis de todo el programa)
    ok(&format!("{HEAD}def setup() -> None:\n    RD0.output()\ndef main() -> None:\n    setup()\n    RD0.on()\n"));
    // salida tambien se puede leer
    ok(&format!("{HEAD}def main() -> None:\n    RD0.output()\n    x: u8 = RD0.value()\n"));
}

#[test]
fn method_errors() {
    err(&format!("{HEAD}def main() -> None:\n    RD0.blink()\n"), "PYP2080");
    err(&format!("{HEAD}def main() -> None:\n    RD0.output()\n    RD0.on(1)\n"), "PYP2055");
    err(&format!("{HEAD}def main() -> None:\n    RD0.output()\n    RD0.value(1, 2)\n"), "PYP2055");
}

#[test]
fn pins_are_not_values_and_names_cannot_clash() {
    err(&format!("{HEAD}def main() -> None:\n    x: u8 = RD0\n"), "PYP2077");
    err(&format!("{HEAD}RD0: u8 = 0\ndef main() -> None:\n    pass\n"), "PYP2009");
    err(&format!("{HEAD}def RD0() -> None:\n    pass\ndef main() -> None:\n    pass\n"), "PYP2009");
    err(&format!("{HEAD}def main() -> None:\n    RD0: u8 = 1\n"), "PYP2009");
}

#[test]
fn attributes_strings_and_old_syntax_are_rejected() {
    err(&format!("{HEAD}def main() -> None:\n    x: u8 = RD0.mode\n"), "PYP1026");
    err(&format!("{HEAD}def main() -> None:\n    RD0.output()\n    RD0.value(\"a\")\n"), "PYP1021");
    // el estilo con objeto (Pin(...)) no existe: no hay constructor
    err("from pypic.periph.gpio import Pin\n", "PYP2074");
    err("led = Pin(\"RD0\", 1)\n", "PYP1001");
}

#[test]
fn pin_calls_work_inside_isr_and_helpers() {
    ok(&format!("{HEAD}@interrupt\ndef isr() -> None:\n    RD0.toggle()\ndef main() -> None:\n    RD0.output()\n"));
}

#[test]
fn diagnostics_point_at_the_use() {
    let src = format!("{HEAD}def main() -> None:\n    RD0.on()\n");
    let out = analyze(&src, &TargetProfile::generic()).diagnostics.render(&src, "t.py");
    assert!(out.contains("t.py:3:5: error[PYP2079]") && out.contains("RD0.output()"), "{out}");
}
