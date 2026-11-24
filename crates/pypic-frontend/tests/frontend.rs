use pypic_frontend::{analyze, TargetProfile};

fn codes(src: &str) -> Vec<&'static str> {
    analyze(src, &TargetProfile::generic()).diagnostics.codes()
}

fn assert_ok(src: &str) {
    let r = analyze(src, &TargetProfile::generic());
    assert!(r.program.is_some(), "esperaba ok, hubo:\n{}", r.diagnostics.render(src, "t.py"));
}

fn assert_err(src: &str, code: &str) {
    let c = codes(src);
    assert!(c.contains(&code), "esperaba {code}, hubo {c:?}\n{src}");
}

#[test]
fn integer_type_names_support_annotations_casts_rom_and_legacy_names() {
    assert_ok("from pypic import uint8, int8, uint16, int16, uint32, int32\nTABLE: Rom[uint8] = [1, 2]\na: uint8 = 1\nb: int8 = -1\nc: uint16 = 256\nd: int16 = -256\ne: uint32 = 65536\nf: int32 = -65536\ndef main() -> None:\n    x: uint8 = uint8(c)\n    y: u16 = u16(x)\n    z: uint8 = TABLE[x]\n");
}

const VALID: &str = r#"
"""docstring ignorada"""
from pypic import u8, u16, Const, interrupt

LIMIT: Const[u8] = 200
counter: u16 = 0
state: bool = False

def add(a: u8, b: u8) -> u8:
    return a + b

def clamp(x: i16) -> i16:
    if x > 100:
        return 100
    elif x < -100:
        return -100
    else:
        return x

@interrupt
def isr() -> None:
    global counter
    counter += 1

def main() -> None:
    global state
    total: u8 = 0
    for i in range(10):
        total = add(total, i)
    for j in range(200, 0, -1):
        total ^= u8(j)
    while total != 0 and not state:
        total -= 1
        if total == LIMIT:
            break
    mask: u8 = (1 << 3) | 0x0F
    wide: u16 = u16(mask) << 4
    state = wide >= 100
"#;

#[test]
fn valid_program_passes() {
    assert_ok(VALID);
}

#[test]
fn constants_are_folded() {
    let r = analyze("X: Const[u8] = 3 * 5 + 1\n", &TargetProfile::generic());
    let p = r.program.unwrap();
    assert_eq!(p.globals["X"].konst, Some(16));
}

#[test]
fn literal_out_of_range() {
    assert_err("x: u8 = 256\n", "PYP2040");
    assert_err("x: i8 = -129\n", "PYP2040");
    assert_ok("x: i8 = -128\n");
}

#[test]
fn mixed_types_need_explicit_cast() {
    let src = "def f(a: u8, b: u16) -> u16:\n    return a + b\n";
    assert_err(src, "PYP2047");
    assert_ok("def f(a: u8, b: u16) -> u16:\n    return u16(a) + b\n");
}

#[test]
fn return_and_arg_types() {
    assert_err("def f() -> u8:\n    return True\n", "PYP2041");
    assert_err("def f(a: u8) -> None:\n    pass\ndef g() -> None:\n    f(1, 2)\n", "PYP2055");
    assert_err("def f() -> u8:\n    pass\n", "PYP2012");
    // la palabra de Python ya no existe: el mensaje dice cual es la de PyPIC
    assert_err("K: Final[u8] = 1\ndef main() -> None:\n    pass\n", "PYP1030");
    assert_err("def f():\n    pass\n", "PYP2004");
}

#[test]
fn conditions_must_be_bool() {
    assert_err("def f(a: u8) -> None:\n    if a:\n        pass\n", "PYP2041");
}

#[test]
fn constant_overflow_is_an_error() {
    assert_err("x: u8 = 200 + 100\n", "PYP2040");
    assert_err("X: Const[u8] = u8(200) + 100\n", "PYP2045");
    assert_err("x: u8 = 1 // 0\n", "PYP2050");
}

#[test]
fn explicit_cast_wraps() {
    let r = analyze("X: Const[u8] = u8(u16(300))\n", &TargetProfile::generic());
    assert_eq!(r.program.unwrap().globals["X"].konst, Some(44));
}

#[test]
fn division_truncates_toward_zero() {
    let r = analyze("X: Const[i8] = -7 // 2\nY: Const[i8] = -7 % 2\n", &TargetProfile::generic());
    let p = r.program.unwrap();
    assert_eq!(p.globals["X"].konst, Some(-3));
    assert_eq!(p.globals["Y"].konst, Some(-1));
}

#[test]
fn scoping_rules() {
    assert_err("def f() -> None:\n    x = 1\n", "PYP2022");
    assert_err("g: u8 = 0\ndef f() -> None:\n    g = 1\n", "PYP2023");
    assert_err("g: u8 = 0\ndef f() -> None:\n    g: u8 = 1\n", "PYP2008");
    assert_err("K: Const[u8] = 1\ndef f() -> None:\n    global K\n", "PYP2021");
    assert_err("def f() -> None:\n    x: u8\n", "PYP2007");
    assert_err("def f() -> None:\n    print(1)\n", "PYP2043");
}

#[test]
fn recursion_is_rejected() {
    assert_err("def f() -> None:\n    f()\n", "PYP2013");
    assert_err("def a() -> None:\n    b()\ndef b() -> None:\n    a()\n", "PYP2013");
}

#[test]
fn interrupt_rules() {
    assert_err("@interrupt\ndef i(a: u8) -> None:\n    pass\n", "PYP2006");
    assert_err("@interrupt\ndef i() -> None:\n    pass\n@interrupt\ndef j() -> None:\n    pass\n", "PYP2005");
    assert_err("@interrupt\ndef i() -> None:\n    pass\ndef f() -> None:\n    i()\n", "PYP2054");
}

#[test]
fn range_inference_and_checks() {
    assert_err("def f(a: u8, b: u16) -> None:\n    for i in range(a, b):\n        pass\n", "PYP2026");
    assert_err("def f() -> None:\n    for i in range(0, 10, 0):\n        pass\n", "PYP2028");
    assert_err("def f() -> None:\n    break\n", "PYP2014");
}

#[test]
fn shifts() {
    assert_err("def f(a: u8) -> u8:\n    return a << 8\n", "PYP2048");
    assert_err("def f(a: u8) -> u8:\n    return 1 << a\n", "PYP2049");
    assert_ok("def f(a: u8, n: u8) -> u8:\n    return (a << 7) | (a >> n)\n");
}

#[test]
fn unsupported_python_is_rejected_with_specific_codes() {
    assert_err("x: u8 = 1.5\n", "PYP0007");
    assert_err("def f(a: u8) -> u8:\n    return a / 2\n", "PYP1022");
    assert_err("def f(a: u8) -> u8:\n    return a ** 2\n", "PYP1022");
    assert_err("class A:\n    pass\n", "PYP1001");
    assert_err("def f() -> None:\n    class A:\n        pass\n", "PYP1010");
    assert_err("def f() -> None:\n    x: u8 = 1\n    x = [1]\n", "PYP1025");
    assert_err("import os\n", "PYP1002");
    assert_err("def f(a: u8 = 1) -> None:\n    pass\n", "PYP1003");
    assert_err("def f() -> None:\n    for i in [1, 2]:\n        pass\n", "PYP1015");
    assert_err("def f(a: u8) -> bool:\n    return 1 < a < 3\n", "PYP1023");
    assert_err("x: u8 = 0\nx = 1\n", "PYP1001");
}

#[test]
fn syntax_errors_report_all_statements() {
    let c = codes("a: u8 = 1 +\ndef g() -> None:\n    pass\nb: u8 = 2 +\n");
    assert!(c.len() >= 2, "{c:?}");
}

#[test]
fn diagnostics_have_line_and_column() {
    let src = "x: u8 = 1\ny: u8 = 300\n";
    let out = analyze(src, &TargetProfile::generic()).diagnostics.render(src, "a.py");
    assert!(out.starts_with("a.py:2:9: error[PYP2040]"), "{out}");
}

#[test]
fn target_profile_limits_types() {
    let t = TargetProfile::new("mini", 8, 16);
    let r = analyze("x: u32 = 0\n", &t);
    assert!(r.diagnostics.codes().contains(&"PYP2030"));
    let r = analyze("x: u16 = 0\n", &t);
    assert!(r.program.is_some());
    // sin limite, u32 es valido: el frontend no asume 8 bits
    assert_ok("x: u32 = 4000000000\n");
}

#[test]
fn comments_blank_lines_and_parenthesized_continuation() {
    assert_ok("# hola\n\nx: u8 = (1 +\n    2)  # fin\n\n");
}

#[test]
fn bad_indentation() {
    assert_err("def f() -> None:\n    x: u8 = 1\n  x = 2\n", "PYP0005");
    assert_err("def f() -> None:\n\tpass\n", "PYP0003");
}
