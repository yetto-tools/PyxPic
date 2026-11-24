//! `pypic.bits`: prueba diferencial entre el interprete de la IR y el simulador del nucleo, y reglas del acarreo.

use pypic_frontend::{analyze, TargetProfile};
use pypic_ir::{lower, verify, Machine};
use pypic_pic14::codegen::Compiled;
use pypic_pic14::sim::Sim;
use pypic_pic14::{compile, Options};

const HEAD: &str = "from pypic.bits import swap, rol_c, ror_c, clear_carry, set_carry, carry, bit_set, bit_clear, bit_test\n";

fn build(src: &str) -> (pypic_ir::Module, Compiled) {
    let r = analyze(src, &TargetProfile::generic());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    let c = compile(&m, &Options::default()).unwrap_or_else(|e| panic!("codegen: {e}\n{src}"));
    (m, c)
}

/// Ejecuta en la IR y en el simulador y compara todas las globales.
fn differential(body: &str) -> (std::collections::BTreeMap<String, i128>, Compiled) {
    let src = format!("{HEAD}{body}");
    let (m, c) = build(&src);
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    let mut s = Sim::new(&c.image);
    s.run_until(5_000_000, |s| s.halted);
    assert!(s.halted, "el programa no termino\n{src}");
    let mut out = std::collections::BTreeMap::new();
    for g in c.globals.iter().filter(|g| !g.is_const) {
        let n = g.ty.bits().max(8) as usize / 8;
        let v: i128 = (0..n).map(|i| (s.read(g.addr as usize + i) as i128) << (8 * i)).sum();
        let v = g.ty.wrap(v);
        assert_eq!(v, mach.global(&g.name).unwrap(), "global {}\n{src}", g.name);
        out.insert(g.name.clone(), v);
    }
    (out, c)
}

fn errors(body: &str) -> Vec<String> {
    let src = format!("{HEAD}{body}");
    let r = analyze(&src, &TargetProfile::generic());
    assert!(r.program.is_none(), "debia rechazarse:\n{src}");
    r.diagnostics.render(&src, "t.py").lines().filter_map(|l| l.split("error[").nth(1).map(|x| x.to_string())).collect()
}

// ---- semantica -------------------------------------------------------------------------------------

#[test]
fn swap_exchanges_the_nibbles() {
    let (g, _) = differential("a: u8 = 0xA5\nb: u8 = 0\nc: u8 = 0\ndef main() -> None:\n    global a, b, c\n    b = swap(a)\n    a = swap(a)\n    c = swap(0x1F)\n");
    assert_eq!((g["a"], g["b"], g["c"]), (0x5A, 0x5A, 0xF1));
}

#[test]
fn a_sixteen_bit_shift_left_chains_the_carry_through_two_rotations() {
    // el caso de uso del issue: desplazar un u16 una posicion con RLF encadenado
    let (g, c) = differential("lo: u8 = 0x81\nhi: u8 = 0x40\nout: bool = False\ndef main() -> None:\n    global lo, hi, out\n    clear_carry()\n    lo = rol_c(lo)\n    hi = rol_c(hi)\n    out = carry()\n");
    assert_eq!((g["lo"], g["hi"], g["out"]), (0x02, 0x81, 0));
    // cada intrinseco es una instruccion: clrc, rlf lo,f, rlf hi,f
    assert!(c.listing.contains("RLF") && c.listing.matches("RLF").count() == 2, "{}", c.listing);
}

#[test]
fn rotating_right_with_the_carry_set_brings_it_in_at_bit_7() {
    let (g, _) = differential("a: u8 = 0x01\nb: u8 = 0x00\nout: bool = False\ndef main() -> None:\n    global a, b, out\n    set_carry()\n    a = ror_c(a)\n    b = ror_c(b)\n    out = carry()\n");
    // a: 0x01 con C=1 -> 0x80, C=1; b: 0x00 con C=1 -> 0x80, C=0
    assert_eq!((g["a"], g["b"], g["out"]), (0x80, 0x80, 0));
}

#[test]
fn a_rotation_can_write_to_another_variable() {
    let (g, _) = differential("src: u8 = 0x81\ndst: u8 = 0\nout: bool = False\ndef main() -> None:\n    global src, dst, out\n    clear_carry()\n    dst = rol_c(src)\n    out = carry()\n");
    assert_eq!((g["src"], g["dst"], g["out"]), (0x81, 0x02, 1));
}

#[test]
fn a_loop_doubles_a_sixteen_bit_value_with_rotations() {
    let body = "lo: u8 = 0x03\nhi: u8 = 0x00\ndef main() -> None:\n    global lo, hi\n    for i in range(12):\n        clear_carry()\n        lo = rol_c(lo)\n        hi = rol_c(hi)\n";
    let (g, _) = differential(body);
    // 3 << 12 = 0x3000
    assert_eq!((g["lo"], g["hi"]), (0x00, 0x30));
}

#[test]
fn bit_set_clear_and_test_work_on_u8_u16_and_locals() {
    let body = "w: u16 = 0\nb: u8 = 0xFF\nt1: bool = False\nt2: bool = False\nt3: bool = False\nloc: u8 = 0\ndef main() -> None:\n    global w, b, t1, t2, t3, loc\n    bit_set(w, 0)\n    bit_set(w, 10)\n    bit_set(w, 15)\n    bit_clear(w, 0)\n    bit_clear(b, 7)\n    bit_clear(b, 0)\n    t1 = bit_test(w, 10)\n    t2 = bit_test(w, 9)\n    t3 = bit_test(b, 7)\n    x: u8 = 0\n    bit_set(x, 3)\n    bit_set(x, 5)\n    loc = x\n";
    let (g, _) = differential(body);
    assert_eq!((g["w"], g["b"], g["t1"], g["t2"], g["t3"], g["loc"]), (0x8400, 0x7E, 1, 0, 0, 0x28));
}

#[test]
fn bit_test_drives_control_flow() {
    let body = "flags: u8 = 0x04\nn: u8 = 0\ndef main() -> None:\n    global n\n    if bit_test(flags, 2):\n        n += 1\n    if bit_test(flags, 3):\n        n += 10\n";
    let (g, _) = differential(body);
    assert_eq!(g["n"], 1);
}

#[test]
fn each_intrinsic_is_one_instruction_in_the_listing() {
    let (_, c) = differential("a: u8 = 0\ndef main() -> None:\n    global a\n    a = swap(a)\n    bit_set(a, 6)\n    bit_clear(a, 1)\n");
    assert!(c.listing.contains("SWAPF 0x") && c.listing.contains("BSF 0x") && c.listing.contains("BCF 0x"), "{}", c.listing);
}

// ---- reglas del acarreo ----------------------------------------------------------------------------

#[test]
fn rotating_needs_the_carry_defined_by_the_previous_statement() {
    let e = errors("a: u8 = 1\ndef main() -> None:\n    global a\n    a = rol_c(a)\n");
    assert!(e[0].starts_with("PYP2142") && e[0].contains("acarreo de entrada"), "{e:?}");
    // otra sentencia entre medias lo pierde
    let e = errors("a: u8 = 1\nz: u8 = 0\ndef main() -> None:\n    global a, z\n    clear_carry()\n    z = 1\n    a = rol_c(a)\n");
    assert!(e[0].starts_with("PYP2142"), "{e:?}");
}

#[test]
fn carry_must_follow_a_rotation_or_a_carry_setter() {
    let e = errors("c: bool = False\ndef main() -> None:\n    global c\n    c = carry()\n");
    assert!(e[0].starts_with("PYP2142") && e[0].contains("carry()"), "{e:?}");
    let e = errors("a: u8 = 1\nc: bool = False\ndef main() -> None:\n    global a, c\n    clear_carry()\n    a = a + 1\n    c = carry()\n");
    assert!(e[0].starts_with("PYP2142"), "{e:?}");
}

#[test]
fn control_flow_resets_the_carry() {
    let e = errors("a: u8 = 1\ndef main() -> None:\n    global a\n    clear_carry()\n    if a == 1:\n        a = rol_c(a)\n");
    assert!(e[0].starts_with("PYP2142"), "{e:?}");
    let e = errors("a: u8 = 1\ndef main() -> None:\n    global a\n    while a != 0:\n        a = rol_c(a)\n");
    assert!(e[0].starts_with("PYP2142"), "{e:?}");
}

#[test]
fn the_carry_does_not_cross_functions() {
    let e = errors("a: u8 = 1\ndef f() -> None:\n    clear_carry()\n\n\ndef main() -> None:\n    global a\n    a = rol_c(a)\n");
    assert!(e[0].starts_with("PYP2142"), "{e:?}");
}

#[test]
fn a_rotation_must_be_the_whole_expression_of_its_statement() {
    let e = errors("a: u8 = 1\nb: u8 = 0\ndef main() -> None:\n    global b\n    clear_carry()\n    b = rol_c(a) + 1\n");
    assert!(e[0].starts_with("PYP2142") && e[0].contains("expresion completa"), "{e:?}");
    let e = errors("a: u8 = 1\nb: u8 = 0\ndef main() -> None:\n    global b\n    clear_carry()\n    b = 1 + rol_c(a)\n");
    assert!(e[0].starts_with("PYP2142"), "{e:?}");
}

// ---- tipos y operandos -----------------------------------------------------------------------------

#[test]
fn rotations_and_bit_operations_need_variables_of_the_right_type() {
    let e = errors("a: u8 = 1\ndef main() -> None:\n    global a\n    clear_carry()\n    a = rol_c(a + 1)\n");
    assert!(e[0].starts_with("PYP2143"), "{e:?}");
    let e = errors("w: u16 = 1\ndef main() -> None:\n    global w\n    clear_carry()\n    w = rol_c(w)\n");
    assert!(e.iter().any(|x| x.starts_with("PYP2141") && x.contains("u8")), "{e:?}");
    let e = errors("K: Const[u8] = 1\ndef main() -> None:\n    bit_set(K, 0)\n");
    assert!(e[0].starts_with("PYP2143"), "{e:?}");
    let e = errors("w: u16 = 1\ndef main() -> None:\n    x: u8 = swap(w)\n");
    assert!(!e.is_empty());
}

#[test]
fn bit_numbers_are_constants_inside_the_width_of_the_variable() {
    let e = errors("a: u8 = 1\ndef main() -> None:\n    global a\n    bit_set(a, 8)\n");
    assert!(e[0].starts_with("PYP2144") && e[0].contains("0..=7"), "{e:?}");
    let e = errors("a: u8 = 1\nn: u8 = 1\ndef main() -> None:\n    global a\n    bit_set(a, n)\n");
    assert!(e[0].starts_with("PYP2144") && e[0].contains("constante"), "{e:?}");
    let e = errors("w: u16 = 1\ndef main() -> None:\n    global w\n    bit_set(w, 16)\n");
    assert!(e[0].starts_with("PYP2144"), "{e:?}");
}

#[test]
fn modifying_a_global_needs_its_global_declaration() {
    let e = errors("a: u8 = 1\ndef main() -> None:\n    bit_set(a, 1)\n");
    assert!(e[0].starts_with("PYP2023"), "{e:?}");
    // leer (bit_test) no lo necesita
    differential("a: u8 = 1\nt: bool = False\ndef main() -> None:\n    global t\n    t = bit_test(a, 0)\n");
}

#[test]
fn importing_an_unknown_name_lists_the_module_exports() {
    let r = analyze("from pypic.bits import nope\n", &TargetProfile::generic());
    let out = r.diagnostics.render("from pypic.bits import nope\n", "t.py");
    assert!(out.contains("PYP2140") && out.contains("rol_c"), "{out}");
}
