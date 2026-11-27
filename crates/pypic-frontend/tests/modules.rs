//! Arbol de modulos del SDK: periph (hardware), system (CPU, reloj, energia) y soft (software).

use pypic_frontend::{analyze, TargetProfile};

fn diag(src: &str) -> (Vec<&'static str>, String) {
    let r = analyze(src, &TargetProfile::generic());
    (r.diagnostics.codes(), r.diagnostics.render(src, "t.py"))
}

fn codes(src: &str) -> Vec<&'static str> {
    diag(src).0
}

#[test]
fn implemented_modules_import_fine() {
    assert!(analyze("from pypic.periph.gpio import RD0\n", &TargetProfile::generic()).program.is_some());
    let t = "from pypic.system.time import pause\nCLOCK_HZ: Const[u32] = 4_000_000\ndef main() -> None:\n    pause(1)\n";
    assert!(analyze(t, &TargetProfile::generic()).program.is_some());
}

#[test]
fn the_old_flat_paths_no_longer_exist_and_say_the_new_one() {
    for (old, new) in [("pypic.gpio", "pypic.periph.gpio"), ("pypic.time", "pypic.system.time")] {
        let src = format!("from {old} import RD0\n");
        let (c, out) = diag(&src);
        assert_eq!(c, vec!["PYP2106"], "{old}");
        assert!(out.contains(new) && out.contains("categoria"), "{out}");
    }
}

#[test]
fn reserved_modules_are_recognised_but_not_implemented_yet() {
    for m in ["pypic.periph.ccp", "pypic.system.wdt", "pypic.system.irq", "pypic.system.clock", "pypic.soft.spi", "pypic.soft.i2c", "pypic.soft.onewire"] {
        let (c, out) = diag(&format!("from {m} import X\n"));
        assert_eq!(c, vec!["PYP2105"], "{m}");
        assert!(out.contains("todavia no esta implementado") && out.contains("catalogo-pic16f887"), "{out}");
    }
}

#[test]
fn hardware_and_software_versions_are_different_modules_with_different_descriptions() {
    let (_, hw) = diag("from pypic.periph.ccp import CCP1\n");
    let (_, sw) = diag("from pypic.soft.spi import SPI\n");
    assert!(hw.contains("periferico por hardware") && !hw.contains("por software"), "{hw}");
    assert!(sw.contains("implementacion por software") && !sw.contains("periferico por hardware"), "{sw}");
}

#[test]
fn a_module_that_does_not_exist_in_its_category_lists_the_ones_that_do() {
    // GPIO no existe "por software" y el hardware de 1-Wire tampoco: cada categoria tiene sus modulos
    let (c, out) = diag("from pypic.soft.gpio import RD0\n");
    assert_eq!(c, vec!["PYP1002"]);
    assert!(out.contains("pypic.soft") && out.contains("onewire") && !out.contains("adc"), "{out}");
    let (c, out) = diag("from pypic.periph.onewire import X\n");
    assert_eq!(c, vec!["PYP1002"]);
    assert!(out.contains("pypic.periph") && out.contains("gpio") && out.contains("timer"), "{out}");
    assert_eq!(codes("from pypic.system.gpio import RD0\n"), vec!["PYP1002"]);
}

#[test]
fn a_category_is_not_importable_as_a_module() {
    for pkg in ["pypic.periph", "pypic.system", "pypic.soft"] {
        let (c, out) = diag(&format!("from {pkg} import gpio\n"));
        assert_eq!(c, vec!["PYP1002"], "{pkg}");
        assert!(out.contains("es una categoria"), "{out}");
    }
}

#[test]
fn unrelated_pypic_paths_keep_the_generic_error_listing_the_tree() {
    let (c, out) = diag("from pypic.foo import x\n");
    assert_eq!(c, vec!["PYP1002"]);
    assert!(out.contains("pypic.periph") && out.contains("pypic.system") && out.contains("pypic.soft"), "{out}");
}
