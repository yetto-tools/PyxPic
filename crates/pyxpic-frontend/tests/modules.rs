//! Arbol de modulos del SDK: periph (hardware), system (CPU, reloj, energia) y soft (software).

use pyxpic_frontend::{analyze, TargetProfile};

fn diag(src: &str) -> (Vec<&'static str>, String) {
    let r = analyze(src, &TargetProfile::generic());
    (r.diagnostics.codes(), r.diagnostics.render(src, "t.py"))
}

fn codes(src: &str) -> Vec<&'static str> {
    diag(src).0
}

#[test]
fn implemented_modules_import_fine() {
    assert!(analyze("from pyxpic.periph.gpio import RD0\n", &TargetProfile::generic()).program.is_some());
    let t = "from pyxpic.system.time import pause\nCLOCK_HZ: Const[u32] = 4_000_000\ndef main() -> None:\n    pause(1)\n";
    assert!(analyze(t, &TargetProfile::generic()).program.is_some());
}

#[test]
fn the_old_flat_paths_no_longer_exist_and_say_the_new_one() {
    for (old, new) in [("pyxpic.gpio", "pyxpic.periph.gpio"), ("pyxpic.time", "pyxpic.system.time")] {
        let src = format!("from {old} import RD0\n");
        let (c, out) = diag(&src);
        assert_eq!(c, vec!["PYP2106"], "{old}");
        assert!(out.contains(new) && out.contains("categoria"), "{out}");
    }
}

#[test]
fn reserved_modules_are_recognised_but_not_implemented_yet() {
    for m in ["pyxpic.periph.ccp", "pyxpic.system.wdt", "pyxpic.system.irq", "pyxpic.system.clock", "pyxpic.soft.spi", "pyxpic.soft.i2c", "pyxpic.soft.onewire"] {
        let (c, out) = diag(&format!("from {m} import X\n"));
        assert_eq!(c, vec!["PYP2105"], "{m}");
        assert!(out.contains("todavia no esta implementado") && out.contains("catalogo-pic16f887"), "{out}");
    }
}

#[test]
fn hardware_and_software_versions_are_different_modules_with_different_descriptions() {
    let (_, hw) = diag("from pyxpic.periph.ccp import CCP1\n");
    let (_, sw) = diag("from pyxpic.soft.spi import SPI\n");
    assert!(hw.contains("periferico por hardware") && !hw.contains("por software"), "{hw}");
    assert!(sw.contains("implementacion por software") && !sw.contains("periferico por hardware"), "{sw}");
}

#[test]
fn a_module_that_does_not_exist_in_its_category_lists_the_ones_that_do() {
    // GPIO no existe "por software" y el hardware de 1-Wire tampoco: cada categoria tiene sus modulos
    let (c, out) = diag("from pyxpic.soft.gpio import RD0\n");
    assert_eq!(c, vec!["PYP1002"]);
    assert!(out.contains("pyxpic.soft") && out.contains("onewire") && !out.contains("adc"), "{out}");
    let (c, out) = diag("from pyxpic.periph.onewire import X\n");
    assert_eq!(c, vec!["PYP1002"]);
    assert!(out.contains("pyxpic.periph") && out.contains("gpio") && out.contains("timer"), "{out}");
    assert_eq!(codes("from pyxpic.system.gpio import RD0\n"), vec!["PYP1002"]);
}

#[test]
fn a_category_is_not_importable_as_a_module() {
    for pkg in ["pyxpic.periph", "pyxpic.system", "pyxpic.soft"] {
        let (c, out) = diag(&format!("from {pkg} import gpio\n"));
        assert_eq!(c, vec!["PYP1002"], "{pkg}");
        assert!(out.contains("es una categoria"), "{out}");
    }
}

#[test]
fn unrelated_pyxpic_paths_keep_the_generic_error_listing_the_tree() {
    let (c, out) = diag("from pyxpic.foo import x\n");
    assert_eq!(c, vec!["PYP1002"]);
    assert!(out.contains("pyxpic.periph") && out.contains("pyxpic.system") && out.contains("pyxpic.soft"), "{out}");
}
