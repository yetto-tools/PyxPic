//! De PyPIC BASIC al AST comun: lo que sale es lo mismo que el parser de Python produce, asi el verificador, el IR y el backend son
//! los de siempre (mismas pruebas diferenciales, mismos diagnosticos de tipos y de perifericos).
//!
//! Lo que BASIC aporta aqui:
//!  * **Variables.** `b0`..`b27` (BYTE) y `w0`..`w13` (WORD) existen sin declarar, como en PICAXE; `DIM x AS BYTE|WORD|BIT` y
//!    `DIM t(16) AS BYTE` crean las propias. Todas son globales. `SYMBOL k = 5` es una constante, `SYMBOL x = b3` un alias y
//!    `SYMBOL led = B.0` un pin. `w0` comparte memoria con `b0` y `b1` en PICAXE; aqui no se solapan y usar los dos es un error.
//!  * **Aritmetica por ancho.** Una expresion se calcula en el ancho de sus variables (y el del destino): `b0 = b1 + b2` suma bytes,
//!    `w0 = b0 * 100` suma en 16 bits. Un valor que no cabe en el destino es un error en vez de truncarse sin avisar.
//!  * **GOSUB/RETURN** son funciones: cada etiqueta a la que se hace GOSUB y que termina en RETURN se convierte en una funcion.
//!  * **GOTO** y las etiquetas existen dentro de una funcion (nodos `Label` y `Goto` del AST).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::parse::{is_command, BArg, BCmp, BExpr, BExprKind, BOp, BStmt, BStmtKind, CaseItem};
use crate::ast::*;
use crate::diag::{Diagnostics, Span};

/// Objetos del SDK: se aceptan sin distinguir mayusculas y se escriben como el SDK los llama.
const SDK_OBJECTS: &[&str] = &["ADC", "UART", "EEPROM", "PWM1", "PWM2", "CMP1", "CMP2", "CVREF", "TMR0", "TMR1", "TMR2"];

#[derive(Clone, Debug)]
struct Var {
    name: String,
    bits: u8,
    array: Option<usize>,
}

enum Res {
    Var(Var),
    Const(i128),
    Pin(char, u8),
    Other,
}

#[derive(Default)]
struct Fn {
    assigned: BTreeSet<String>,
    loops: u32,
    is_sub: bool,
}

struct Lw<'a> {
    d: &'a mut Diagnostics,
    next_id: u32,
    vars: HashMap<String, Var>,
    decl_order: Vec<String>,
    first_use: HashMap<String, Span>,
    consts: HashMap<String, i128>,
    aliases: HashMap<String, String>,
    pins: HashMap<String, (char, u8)>,
    /// Objetos del SDK que el programa importo (para aceptar `adc.read` y `ADC.read`).
    sdk_names: HashMap<String, String>,
    explicit: Vec<(String, Span, Vec<(String, Span)>)>,
    /// Direccion de cada pin que el programa usa: `true` si se escribe, `false` si solo se lee.
    pin_out: BTreeMap<(char, u8), bool>,
    uses_pause: bool,
    uses_pause_us: bool,
    uses_adc: bool,
    adc_channels: BTreeSet<u8>,
}

pub fn lower(stmts: &[BStmt], d: &mut Diagnostics) -> Module {
    let mut lw = Lw {
        d,
        next_id: 0,
        vars: HashMap::new(),
        decl_order: Vec::new(),
        first_use: HashMap::new(),
        consts: HashMap::new(),
        aliases: HashMap::new(),
        pins: HashMap::new(),
        sdk_names: SDK_OBJECTS.iter().map(|n| (n.to_ascii_lowercase(), n.to_string())).collect(),
        explicit: Vec::new(),
        pin_out: BTreeMap::new(),
        uses_pause: false,
        uses_pause_us: false,
        uses_adc: false,
        adc_channels: BTreeSet::new(),
    };
    lw.run(stmts)
}

fn implicit(name: &str) -> Option<(char, u32)> {
    let l = name.to_ascii_lowercase();
    let kind = l.chars().next()?;
    let n: u32 = l[1..].parse().ok()?;
    match kind {
        'b' if n <= 27 => Some(('b', n)),
        'w' if n <= 13 => Some(('w', n)),
        _ => None,
    }
}

fn pin_name_of(name: &str) -> Option<(char, u8)> {
    let u = name.to_ascii_uppercase();
    let b = u.as_bytes();
    (b.len() == 3 && b[0] == b'R' && matches!(b[1], b'A'..=b'E') && matches!(b[2], b'0'..=b'7')).then(|| (b[1] as char, b[2] - b'0'))
}

impl<'a> Lw<'a> {
    // ---------------------------------------------------------------- constructores del AST
    fn ex(&mut self, span: Span, kind: ExprKind) -> Expr {
        let id = self.next_id;
        self.next_id += 1;
        Expr { id, span, kind }
    }
    fn int(&mut self, span: Span, v: i128) -> Expr {
        self.ex(span, ExprKind::Int(v))
    }
    fn name_expr(&mut self, span: Span, n: &str) -> Expr {
        self.ex(span, ExprKind::Name(n.to_string()))
    }
    fn cast(&mut self, e: Expr, bits: u8) -> Expr {
        let span = e.span;
        let f = if bits == 8 { "uint8" } else { "uint16" };
        self.ex(span, ExprKind::Call { func: f.to_string(), func_span: span, args: vec![e] })
    }
    fn method(&mut self, span: Span, obj: &str, method: &str, args: Vec<Expr>, kwargs: Vec<Kwarg>) -> Expr {
        self.ex(span, ExprKind::MethodCall { obj: obj.to_string(), obj_span: span, attr: None, method: method.to_string(), args, kwargs })
    }
    fn pin_ident(p: (char, u8)) -> String {
        format!("R{}{}", p.0, p.1)
    }
    fn err(&mut self, code: &'static str, span: Span, msg: impl Into<String>) {
        self.d.error(code, span, msg);
    }

    // ---------------------------------------------------------------- nombres
    fn resolve(&mut self, name: &str, span: Span) -> Res {
        let key = name.to_ascii_lowercase();
        if let Some(a) = self.aliases.get(&key).cloned() {
            return self.resolve(&a, span);
        }
        if let Some(&v) = self.consts.get(&key) {
            return Res::Const(v);
        }
        if let Some(&p) = self.pins.get(&key) {
            return Res::Pin(p.0, p.1);
        }
        if let Some(v) = self.vars.get(&key) {
            return Res::Var(v.clone());
        }
        if let Some((kind, _)) = implicit(name) {
            let v = Var { name: key.clone(), bits: if kind == 'b' { 8 } else { 16 }, array: None };
            self.vars.insert(key.clone(), v.clone());
            self.decl_order.push(key.clone());
            self.first_use.insert(key, span);
            return Res::Var(v);
        }
        if let Some(p) = pin_name_of(name) {
            return Res::Pin(p.0, p.1);
        }
        Res::Other
    }

    fn pin_of(&mut self, e: &BExpr) -> Option<(char, u8)> {
        match &e.kind {
            BExprKind::Pin(p, b) => Some((*p, *b)),
            BExprKind::Name(n) => match self.resolve(n, e.span) {
                Res::Pin(p, b) => Some((p, b)),
                _ => None,
            },
            _ => None,
        }
    }

    fn const_eval(&mut self, e: &BExpr) -> Option<i128> {
        match &e.kind {
            BExprKind::Int(v) => Some(*v),
            BExprKind::Name(n) => match self.resolve(n, e.span) {
                Res::Const(v) => Some(v),
                _ => None,
            },
            BExprKind::Neg(x) => self.const_eval(x).map(|v| -v),
            BExprKind::Not(x) => self.const_eval(x).map(|v| !v & 0xFFFF),
            BExprKind::Bin(op, l, r) => {
                let (a, b) = (self.const_eval(l)?, self.const_eval(r)?);
                Some(match op {
                    BOp::Add => a + b,
                    BOp::Sub => a - b,
                    BOp::Mul => a * b,
                    BOp::Div => a.checked_div(b)?,
                    BOp::Mod => a.checked_rem(b)?,
                    BOp::And => a & b,
                    BOp::Or => a | b,
                    BOp::Xor => a ^ b,
                    BOp::Shl => a.checked_shl(u32::try_from(b).ok()?)?,
                    BOp::Shr => a.checked_shr(u32::try_from(b).ok()?)?,
                })
            }
            _ => None,
        }
    }

    // ---------------------------------------------------------------- ancho de una expresion
    fn width(&mut self, e: &BExpr) -> Option<u8> {
        match &e.kind {
            BExprKind::Int(v) => (*v > 255).then_some(16),
            BExprKind::Name(n) => match self.resolve(n, e.span) {
                Res::Var(v) => Some(v.bits),
                Res::Const(v) => (v > 255).then_some(16),
                Res::Pin(..) => Some(8),
                Res::Other => None,
            },
            BExprKind::Pin(..) => Some(8),
            BExprKind::Neg(x) | BExprKind::Not(x) => self.width(x),
            BExprKind::Bin(op, l, r) => {
                let wl = self.width(l);
                if matches!(op, BOp::Shl | BOp::Shr) {
                    return wl;
                }
                let wr = self.width(r);
                wl.max(wr)
            }
            BExprKind::Cmp(_, l, r) => {
                let (a, b) = (self.width(l), self.width(r));
                a.max(b)
            }
            BExprKind::Call { obj: None, .. } => Some(8),
            BExprKind::Call { obj: Some(o), name, .. } => {
                let (o, n) = (o.to_ascii_uppercase(), name.to_ascii_lowercase());
                match (o.as_str(), n.as_str()) {
                    ("ADC", "read" | "result") => Some(16),
                    ("TMR1", "value") => Some(16),
                    _ => None,
                }
            }
        }
    }

    // ---------------------------------------------------------------- expresiones
    fn arg(&mut self, a: &BArg) -> Option<Expr> {
        match &a.value.kind {
            BExprKind::Int(v) => Some(self.int(a.value.span, *v)),
            BExprKind::Name(n) if matches!(self.resolve(n, a.value.span), Res::Other) => Some(self.name_expr(a.value.span, n)),
            _ => {
                let w = self.width(&a.value).unwrap_or(8);
                self.int_expr(&a.value, w)
            }
        }
    }

    fn call_parts(&mut self, e: &BExpr) -> Option<(String, String, Vec<Expr>, Vec<Kwarg>)> {
        let BExprKind::Call { obj: Some(obj), name, args } = &e.kind else { return None };
        // un pin con nombre (`led.on()`) se resuelve a su pin
        let obj_name = match self.resolve(obj, e.span) {
            Res::Pin(p, b) => {
                self.pin_out.entry((p, b)).or_insert(false);
                Self::pin_ident((p, b))
            }
            _ => self.sdk_names.get(&obj.to_ascii_lowercase()).cloned().unwrap_or_else(|| obj.clone()),
        };
        let (mut pos, mut kw) = (Vec::new(), Vec::new());
        for a in args {
            let v = self.arg(a)?;
            match &a.name {
                Some(n) => kw.push(Kwarg { name: n.to_ascii_lowercase(), name_span: a.value.span, value: v }),
                None => pos.push(v),
            }
        }
        Some((obj_name, name.to_ascii_lowercase(), pos, kw))
    }

    /// La expresion `e` calculada en `t` bits (8 o 16): una variable mas estrecha se amplia.
    fn int_expr(&mut self, e: &BExpr, t: u8) -> Option<Expr> {
        let sp = e.span;
        match &e.kind {
            BExprKind::Int(v) => Some(self.int(sp, *v)),
            BExprKind::Name(n) => match self.resolve(n, sp) {
                Res::Var(v) => {
                    if v.array.is_some() {
                        self.err("PYB0041", sp, format!("'{n}' es un arreglo: se lee con {n}(indice)"));
                        return None;
                    }
                    let ex = self.name_expr(sp, &v.name);
                    Some(if v.bits < t { self.cast(ex, t) } else { ex })
                }
                Res::Const(v) => Some(self.int(sp, v)),
                Res::Pin(p, b) => self.pin_read(sp, (p, b), t),
                Res::Other => Some(self.name_expr(sp, n)),
            },
            BExprKind::Pin(p, b) => self.pin_read(sp, (*p, *b), t),
            BExprKind::Neg(_) => {
                self.err("PYB0042", sp, "los valores de PyPIC BASIC no tienen signo: no hay numeros negativos (b0 = 255 es el ultimo valor de un BYTE)");
                None
            }
            BExprKind::Not(x) => {
                let v = self.int_expr(x, t)?;
                Some(self.ex(sp, ExprKind::Unary(UnOp::Invert, Box::new(v))))
            }
            BExprKind::Bin(op, l, r) => {
                let bop = match op {
                    BOp::Add => BinOp::Add,
                    BOp::Sub => BinOp::Sub,
                    BOp::Mul => BinOp::Mul,
                    BOp::Div => BinOp::Div,
                    BOp::Mod => BinOp::Mod,
                    BOp::And => BinOp::BitAnd,
                    BOp::Or => BinOp::BitOr,
                    BOp::Xor => BinOp::BitXor,
                    BOp::Shl => BinOp::Shl,
                    BOp::Shr => BinOp::Shr,
                };
                let a = self.int_expr(l, t)?;
                let b = if matches!(op, BOp::Shl | BOp::Shr) {
                    let w = self.width(r).unwrap_or(8);
                    self.int_expr(r, w)?
                } else {
                    self.int_expr(r, t)?
                };
                Some(self.ex(sp, ExprKind::Binary(bop, Box::new(a), Box::new(b))))
            }
            BExprKind::Cmp(..) => {
                self.err("PYB0043", sp, "una comparacion no es un numero: usala en un IF, WHILE o DO");
                None
            }
            BExprKind::Call { obj: None, name, args } => match self.resolve(name, sp) {
                Res::Var(v) if v.array.is_some() && args.len() == 1 => {
                    let w = self.width(&args[0].value).unwrap_or(8);
                    let idx = self.int_expr(&args[0].value, w)?;
                    let ex = self.ex(sp, ExprKind::Index { obj: v.name.clone(), obj_span: sp, index: Box::new(idx) });
                    Some(if t > 8 { self.cast(ex, t) } else { ex })
                }
                _ => {
                    self.err("PYB0044", sp, format!("'{name}(...)' no es un arreglo declarado con DIM ni una funcion del SDK (las del SDK se llaman objeto.metodo(...))"));
                    None
                }
            },
            BExprKind::Call { obj: Some(_), .. } => {
                let w = self.width(e);
                let (obj, method, args, kwargs) = self.call_parts(e)?;
                let call = self.method(sp, &obj, &method, args, kwargs);
                Some(match w {
                    Some(w) if w < t => self.cast(call, t),
                    _ => call,
                })
            }
        }
    }

    fn pin_read(&mut self, sp: Span, p: (char, u8), t: u8) -> Option<Expr> {
        self.pin_out.entry(p).or_insert(false);
        let call = self.method(sp, &Self::pin_ident(p), "value", Vec::new(), Vec::new());
        Some(if t > 8 { self.cast(call, t) } else { call })
    }

    fn boolish(e: &BExpr) -> bool {
        match &e.kind {
            BExprKind::Cmp(..) => true,
            BExprKind::Not(x) => Self::boolish(x),
            BExprKind::Bin(BOp::And | BOp::Or | BOp::Xor, l, r) => Self::boolish(l) || Self::boolish(r),
            _ => false,
        }
    }

    /// Una condicion: comparaciones unidas con AND, OR y NOT; un numero suelto vale "distinto de cero".
    fn cond(&mut self, e: &BExpr) -> Option<Expr> {
        let sp = e.span;
        match &e.kind {
            BExprKind::Cmp(op, l, r) => {
                let (wl, wr) = (self.width(l), self.width(r));
                let t = wl.max(wr).unwrap_or(8);
                let (a, b) = (self.int_expr(l, t)?, self.int_expr(r, t)?);
                let cop = match op {
                    BCmp::Eq => CmpOp::Eq,
                    BCmp::Ne => CmpOp::Ne,
                    BCmp::Lt => CmpOp::Lt,
                    BCmp::Le => CmpOp::Le,
                    BCmp::Gt => CmpOp::Gt,
                    BCmp::Ge => CmpOp::Ge,
                };
                Some(self.ex(sp, ExprKind::Compare(cop, Box::new(a), Box::new(b))))
            }
            BExprKind::Not(x) if Self::boolish(x) => {
                let c = self.cond(x)?;
                Some(self.ex(sp, ExprKind::Unary(UnOp::Not, Box::new(c))))
            }
            BExprKind::Bin(op @ (BOp::And | BOp::Or), l, r) if Self::boolish(e) => {
                let (a, b) = (self.cond(l)?, self.cond(r)?);
                let bop = if *op == BOp::And { BoolOp::And } else { BoolOp::Or };
                Some(self.ex(sp, ExprKind::Bool_(bop, vec![a, b])))
            }
            BExprKind::Bin(BOp::Xor, ..) if Self::boolish(e) => {
                self.err("PYB0045", sp, "XOR entre condiciones no esta soportado: escribelo con AND, OR y NOT");
                None
            }
            _ => {
                let w = self.width(e).unwrap_or(8);
                let v = self.int_expr(e, w)?;
                let zero = self.int(sp, 0);
                Some(self.ex(sp, ExprKind::Compare(CmpOp::Ne, Box::new(v), Box::new(zero))))
            }
        }
    }

    /// El valor que se asigna a una variable de `target_bits`: una constante tiene que caber; el resto se calcula en el ancho
    /// necesario y, si el destino es mas estrecho, se trunca con una conversion visible.
    fn assigned_value(&mut self, value: &BExpr, target_bits: u8, what: &str) -> Option<Expr> {
        if let Some(v) = self.const_eval(value) {
            let max = if target_bits == 8 { 255 } else { 65535 };
            if v < 0 || v > max {
                self.err("PYB0040", value.span, format!("{v} no cabe en {what} (0 a {max})"));
                return None;
            }
            return Some(self.int(value.span, v));
        }
        let t = self.width(value).unwrap_or(0).max(target_bits);
        let e = self.int_expr(value, t)?;
        Some(if t > target_bits { self.cast(e, target_bits) } else { e })
    }

    // ---------------------------------------------------------------- sentencias
    fn st(&self, span: Span, kind: StmtKind) -> Stmt {
        Stmt { span, kind }
    }

    fn block(&mut self, stmts: &[BStmt], cx: &mut Fn) -> Vec<Stmt> {
        let mut out = Vec::new();
        for s in stmts {
            self.stmt(s, cx, &mut out);
        }
        if out.is_empty() {
            out.push(self.st(Span::new(0, 0), StmtKind::Pass));
        }
        out
    }

    fn target_var(&mut self, name: &str, span: Span) -> Option<Var> {
        match self.resolve(name, span) {
            Res::Var(v) => Some(v),
            Res::Const(_) => {
                self.err("PYB0046", span, format!("'{name}' es una constante (SYMBOL): no se le asigna"));
                None
            }
            Res::Pin(..) => {
                self.err("PYB0046", span, format!("'{name}' es un pin: se maneja con HIGH, LOW y TOGGLE"));
                None
            }
            Res::Other => {
                self.err("PYB0047", span, format!("la variable '{name}' no existe: declarala con DIM {name} AS BYTE (b0 a b27 y w0 a w13 existen siempre)"));
                None
            }
        }
    }

    fn stmt(&mut self, s: &BStmt, cx: &mut Fn, out: &mut Vec<Stmt>) {
        let sp = s.span;
        match &s.kind {
            BStmtKind::Assign { target, target_span, index, value } => {
                let Some(v) = self.target_var(target, *target_span) else { return };
                // un arreglo se escribe por indice, sin `global`; solo las variables simples necesitan declararse
                if v.array.is_none() {
                    cx.assigned.insert(v.name.clone());
                }
                match (index, v.array) {
                    (Some(i), Some(_)) => {
                        let w = self.width(i).unwrap_or(8);
                        let (Some(idx), Some(val)) = (self.int_expr(i, w), self.assigned_value(value, 8, "un elemento BYTE")) else { return };
                        out.push(self.st(sp, StmtKind::IndexAssign { target: Ident { name: v.name.clone(), span: *target_span }, index: idx, op: None, value: val }));
                    }
                    (None, None) => {
                        let what = if v.bits == 8 { "un BYTE" } else { "un WORD" };
                        let Some(val) = self.assigned_value(value, v.bits, what) else { return };
                        out.push(self.st(sp, StmtKind::Assign { target: Ident { name: v.name.clone(), span: *target_span }, value: val }));
                    }
                    (Some(_), None) => self.err("PYB0048", *target_span, format!("'{target}' no es un arreglo: DIM {target}(tamano) AS BYTE")),
                    (None, Some(_)) => self.err("PYB0048", *target_span, format!("'{target}' es un arreglo: se asigna con {target}(indice) = valor")),
                }
            }
            BStmtKind::Label(l) => out.push(self.st(sp, StmtKind::Label(Ident { name: l.clone(), span: sp }))),
            BStmtKind::Goto(l) => out.push(self.st(sp, StmtKind::Goto(Ident { name: l.clone(), span: sp }))),
            BStmtKind::Gosub(l) => {
                let call = self.ex(sp, ExprKind::Call { func: l.clone(), func_span: sp, args: Vec::new() });
                out.push(self.st(sp, StmtKind::Expr(call)));
            }
            BStmtKind::Return => {
                if cx.is_sub {
                    out.push(self.st(sp, StmtKind::Return(None)));
                } else {
                    self.err("PYB0050", sp, "RETURN fuera de una subrutina: solo termina una etiqueta a la que se llama con GOSUB");
                }
            }
            BStmtKind::End => {
                let t = self.ex(sp, ExprKind::Bool(true));
                let pass = self.st(sp, StmtKind::Pass);
                out.push(self.st(sp, StmtKind::While { test: t, body: vec![pass] }));
            }
            BStmtKind::Exit => {
                if cx.loops == 0 {
                    self.err("PYB0051", sp, "EXIT fuera de un bucle (FOR, DO o WHILE)");
                } else {
                    out.push(self.st(sp, StmtKind::Break));
                }
            }
            BStmtKind::If { arms, orelse } => {
                // IF/ELSEIF/ELSE -> If anidados en la rama else
                let mut tail = self.block_opt(orelse, cx);
                for (c, body) in arms.iter().rev() {
                    let (Some(test), body) = (self.cond(c), self.block(body, cx)) else { return };
                    let node = self.st(c.span, StmtKind::If { test, body, orelse: tail });
                    tail = vec![node];
                }
                out.extend(tail);
            }
            BStmtKind::While { cond, body } => {
                cx.loops += 1;
                let b = self.block(body, cx);
                cx.loops -= 1;
                if let Some(test) = self.cond(cond) {
                    out.push(self.st(sp, StmtKind::While { test, body: b }));
                }
            }
            BStmtKind::Do { pre, post, body } => {
                cx.loops += 1;
                let mut b = self.block(body, cx);
                cx.loops -= 1;
                match (pre, post) {
                    (Some((until, c)), _) => {
                        let Some(mut test) = self.cond(c) else { return };
                        if *until {
                            test = self.ex(c.span, ExprKind::Unary(UnOp::Not, Box::new(test)));
                        }
                        out.push(self.st(sp, StmtKind::While { test, body: b }));
                    }
                    (None, Some((until, c))) => {
                        // DO ... LOOP WHILE c: al final se sale si c es falsa; LOOP UNTIL c: si es verdadera
                        let Some(mut test) = self.cond(c) else { return };
                        if !*until {
                            test = self.ex(c.span, ExprKind::Unary(UnOp::Not, Box::new(test)));
                        }
                        let brk = self.st(c.span, StmtKind::Break);
                        b.push(self.st(c.span, StmtKind::If { test, body: vec![brk], orelse: Vec::new() }));
                        let t = self.ex(sp, ExprKind::Bool(true));
                        out.push(self.st(sp, StmtKind::While { test: t, body: b }));
                    }
                    (None, None) => {
                        let t = self.ex(sp, ExprKind::Bool(true));
                        out.push(self.st(sp, StmtKind::While { test: t, body: b }));
                    }
                }
            }
            BStmtKind::For { var, var_span, from, to, step, body } => self.for_stmt(sp, var, *var_span, from, to, step.as_ref(), body, cx, out),
            BStmtKind::Select { subject, cases, default } => self.select_stmt(sp, subject, cases, default, cx, out),
            BStmtKind::Cmd { obj, name, name_span, args } => self.command(sp, obj.as_deref(), name, *name_span, args, cx, out),
            BStmtKind::Device(_) | BStmtKind::Clock { .. } | BStmtKind::Import { .. } | BStmtKind::Symbol { .. } | BStmtKind::Dim { .. } => {
                self.err("PYB0052", sp, "DEVICE, CLOCK, FROM...IMPORT, SYMBOL y DIM van al principio del programa, fuera de los bloques");
            }
        }
    }

    fn block_opt(&mut self, stmts: &[BStmt], cx: &mut Fn) -> Vec<Stmt> {
        if stmts.is_empty() {
            return Vec::new();
        }
        self.block(stmts, cx)
    }

    #[allow(clippy::too_many_arguments)]
    fn for_stmt(&mut self, sp: Span, var: &str, var_span: Span, from: &BExpr, to: &BExpr, step: Option<&BExpr>, body: &[BStmt], cx: &mut Fn, out: &mut Vec<Stmt>) {
        let Some(v) = self.target_var(var, var_span) else { return };
        if v.array.is_some() {
            self.err("PYB0048", var_span, format!("'{var}' es un arreglo: el FOR usa una variable simple"));
            return;
        }
        cx.assigned.insert(v.name.clone());
        let max: i128 = if v.bits == 8 { 255 } else { 65535 };
        let step_v = match step {
            None => 1,
            Some(s) => match self.const_eval(s) {
                Some(k) if k > 0 => k,
                _ => {
                    self.err("PYB0053", s.span, "el STEP de un FOR es una constante mayor que cero (la direccion la decide FROM y TO)");
                    return;
                }
            },
        };
        let (cf, ct) = (self.const_eval(from), self.const_eval(to));
        // como en PICAXE, si el inicio es mayor que el final con valores constantes, cuenta hacia abajo
        let down = matches!((cf, ct), (Some(a), Some(b)) if a > b);
        if let Some(t) = ct {
            if !down && t + step_v > max {
                self.err("PYB0054", to.span, format!("el FOR llega a {t} con paso {step_v} y {} sumaria hasta {}, que no cabe en la variable: usa una variable WORD", var, t + step_v));
                return;
            }
            if down && t < step_v {
                self.err("PYB0054", to.span, format!("el FOR hacia abajo llega a {t} con paso {step_v} y la variable pasaria por debajo de cero: usa un paso menor o termina antes"));
                return;
            }
            if t > max {
                self.err("PYB0040", to.span, format!("{t} no cabe en la variable '{var}'"));
                return;
            }
        }
        let what = if v.bits == 8 { "un BYTE" } else { "un WORD" };
        let Some(init) = self.assigned_value(from, v.bits, what) else { return };
        out.push(self.st(sp, StmtKind::Assign { target: Ident { name: v.name.clone(), span: var_span }, value: init }));
        let t = self.width(to).unwrap_or(0).max(v.bits);
        let (lhs, rhs) = (self.name_expr(var_span, &v.name), self.int_expr(to, t));
        let Some(rhs) = rhs else { return };
        let lhs = if v.bits < t { self.cast(lhs, t) } else { lhs };
        let test = self.ex(sp, ExprKind::Compare(if down { CmpOp::Ge } else { CmpOp::Le }, Box::new(lhs), Box::new(rhs)));
        cx.loops += 1;
        let mut b = self.block(body, cx);
        cx.loops -= 1;
        let (cur, k) = (self.name_expr(var_span, &v.name), self.int(var_span, step_v));
        let next = self.ex(var_span, ExprKind::Binary(if down { BinOp::Sub } else { BinOp::Add }, Box::new(cur), Box::new(k)));
        b.push(self.st(var_span, StmtKind::Assign { target: Ident { name: v.name.clone(), span: var_span }, value: next }));
        out.push(self.st(sp, StmtKind::While { test, body: b }));
    }

    fn select_stmt(&mut self, sp: Span, subject: &BExpr, cases: &[(Vec<CaseItem>, Vec<BStmt>)], default: &[BStmt], cx: &mut Fn, out: &mut Vec<Stmt>) {
        // la variable se compara una vez por caso: tiene que ser una variable (no una expresion que cueste o cambie)
        let simple = matches!(&subject.kind, BExprKind::Name(n) if matches!(self.resolve(n, subject.span), Res::Var(_))) || matches!(subject.kind, BExprKind::Pin(..));
        if !simple {
            self.err("PYB0055", subject.span, "SELECT CASE necesita una variable: guarda antes el valor en una (w0 = ...: SELECT CASE w0)");
            return;
        }
        let mut tail = self.block_opt(default, cx);
        for (items, body) in cases.iter().rev() {
            let mut tests = Vec::new();
            for item in items {
                let one = match item {
                    CaseItem::Value(v) => {
                        let e = BExpr { span: v.span, kind: BExprKind::Cmp(BCmp::Eq, Box::new(subject.clone()), Box::new(v.clone())) };
                        self.cond(&e)
                    }
                    CaseItem::Range(a, b) => {
                        let ge = BExpr { span: a.span, kind: BExprKind::Cmp(BCmp::Ge, Box::new(subject.clone()), Box::new(a.clone())) };
                        let le = BExpr { span: b.span, kind: BExprKind::Cmp(BCmp::Le, Box::new(subject.clone()), Box::new(b.clone())) };
                        let (Some(x), Some(y)) = (self.cond(&ge), self.cond(&le)) else { return };
                        Some(self.ex(a.span.to(b.span), ExprKind::Bool_(BoolOp::And, vec![x, y])))
                    }
                };
                let Some(one) = one else { return };
                tests.push(one);
            }
            let test = if tests.len() == 1 { tests.pop().unwrap() } else { self.ex(sp, ExprKind::Bool_(BoolOp::Or, tests)) };
            let body = self.block(body, cx);
            tail = vec![self.st(sp, StmtKind::If { test, body, orelse: tail })];
        }
        out.extend(tail);
    }

    #[allow(clippy::too_many_arguments)]
    fn command(&mut self, sp: Span, obj: Option<&str>, name: &str, name_span: Span, args: &[BArg], cx: &mut Fn, out: &mut Vec<Stmt>) {
        if obj.is_some() {
            let call = BExpr { span: sp, kind: BExprKind::Call { obj: obj.map(str::to_string), name: name.to_string(), args: args.to_vec() } };
            if let Some((o, m, a, k)) = self.call_parts(&call) {
                let c = self.method(sp, &o, &m, a, k);
                out.push(self.st(sp, StmtKind::Expr(c)));
            }
            return;
        }
        let upper = name.to_ascii_uppercase();
        let want = |this: &mut Self, n: usize| -> bool {
            if args.len() != n {
                this.err("PYB0060", sp, format!("{upper} necesita {n} argumento(s)"));
                false
            } else {
                true
            }
        };
        if !is_command(name) {
            self.err("PYB0061", name_span, format!("'{name}' no es un comando"));
            return;
        }
        match upper.as_str() {
            "HIGH" | "LOW" | "TOGGLE" | "OUTPUT" | "INPUT" => {
                if !want(self, 1) {
                    return;
                }
                let Some(p) = self.pin_of(&args[0].value) else {
                    self.err("PYB0062", args[0].value.span, format!("{upper} necesita un pin: B.0, o un nombre definido con SYMBOL led = B.0"));
                    return;
                };
                let as_output = upper != "INPUT";
                let entry = self.pin_out.entry(p).or_insert(as_output);
                *entry = *entry || as_output;
                let m = match upper.as_str() {
                    "HIGH" => "on",
                    "LOW" => "off",
                    "TOGGLE" => "toggle",
                    "OUTPUT" => "output",
                    _ => "input",
                };
                let call = self.method(sp, &Self::pin_ident(p), m, Vec::new(), Vec::new());
                out.push(self.st(sp, StmtKind::Expr(call)));
            }
            "PAUSE" | "PAUSEUS" => {
                if !want(self, 1) {
                    return;
                }
                let us = upper == "PAUSEUS";
                if us { self.uses_pause_us = true } else { self.uses_pause = true }
                let arg = &args[0].value;
                let value = match self.const_eval(arg) {
                    Some(v) => self.int(arg.span, v),
                    None => {
                        let Some(e) = self.int_expr(arg, 16) else { return };
                        e
                    }
                };
                let call = self.ex(sp, ExprKind::Call { func: if us { "delay_us" } else { "pause" }.to_string(), func_span: name_span, args: vec![value] });
                out.push(self.st(sp, StmtKind::Expr(call)));
            }
            "READADC" | "READADC10" => {
                if !want(self, 2) {
                    return;
                }
                let Some(ch) = self.const_eval(&args[0].value).filter(|c| (0..=13).contains(c)) else {
                    self.err("PYB0063", args[0].value.span, format!("{upper} necesita un canal constante de 0 a 13 (el canal ANn del PIC)"));
                    return;
                };
                let BExprKind::Name(vn) = &args[1].value.kind else {
                    self.err("PYB0063", args[1].value.span, format!("{upper} guarda el resultado en una variable"));
                    return;
                };
                let Some(v) = self.target_var(vn, args[1].value.span) else { return };
                if upper == "READADC10" && v.bits != 16 {
                    self.err("PYB0064", args[1].value.span, "READADC10 da 10 bits: guardalos en una variable WORD (w0, o DIM x AS WORD)");
                    return;
                }
                self.uses_adc = true;
                cx.assigned.insert(v.name.clone());
                self.adc_channels.insert(ch as u8);
                let chan = self.name_expr(args[0].value.span, &format!("AN{ch}"));
                let read = self.method(sp, "ADC", "read", vec![chan], Vec::new());
                let value = if upper == "READADC10" {
                    read
                } else {
                    // 8 bits: los ocho de arriba de los 10 (el resultado justificado a la izquierda de PICAXE)
                    let two = self.int(sp, 2);
                    let shifted = self.ex(sp, ExprKind::Binary(BinOp::Shr, Box::new(read), Box::new(two)));
                    self.cast(shifted, 8)
                };
                if v.bits == 16 && upper == "READADC" {
                    // un WORD recibe el byte ampliado
                    let wide = self.cast(value, 16);
                    out.push(self.st(sp, StmtKind::Assign { target: Ident { name: v.name.clone(), span: args[1].value.span }, value: wide }));
                } else {
                    out.push(self.st(sp, StmtKind::Assign { target: Ident { name: v.name.clone(), span: args[1].value.span }, value }));
                }
            }
            _ => unreachable!("is_command ya filtro el nombre"),
        }
    }

    // ---------------------------------------------------------------- programa
    fn run(&mut self, stmts: &[BStmt]) -> Module {
        let mut items: Vec<Item> = Vec::new();
        let mut body: Vec<&BStmt> = Vec::new();
        let mut device: Option<(String, Span)> = None;
        let mut clock: Option<Item> = None;
        for s in stmts {
            match &s.kind {
                BStmtKind::Device(n) => {
                    if device.is_some() {
                        self.err("PYB0070", s.span, "DEVICE ya esta declarado");
                    }
                    device = Some((n.clone(), s.span));
                }
                BStmtKind::Clock { kind, amount, unit } => {
                    let Some(v) = self.const_eval(amount) else {
                        self.err("PYB0071", amount.span, "la cantidad del reloj es un numero: CLOCK INTERNAL 4 MHZ");
                        continue;
                    };
                    let unit_name = match unit.to_ascii_uppercase().as_str() {
                        "MHZ" => "MHz",
                        "KHZ" => "kHz",
                        _ => "Hz",
                    };
                    let (a, u) = (self.int(amount.span, v), self.name_expr(s.span, unit_name));
                    let e = self.ex(s.span, ExprKind::Binary(BinOp::Mul, Box::new(a), Box::new(u)));
                    let ann = TypeExpr { span: s.span, kind: TypeExprKind::Clock(kind.clone(), Box::new(e)) };
                    clock = Some(Item::Var(VarDecl { span: s.span, name: Ident { name: "CLOCK".into(), span: s.span }, ann, value: None }));
                }
                BStmtKind::Import { module, names } => {
                    for (n, _) in names {
                        self.sdk_names.insert(n.to_ascii_lowercase(), n.clone());
                    }
                    self.explicit.push((module.clone(), s.span, names.clone()));
                }
                BStmtKind::Symbol { name, value } => self.symbol(name, value, s.span),
                BStmtKind::Dim { name, bits, len, init } => self.dim(name, *bits, *len, init.as_ref(), s.span),
                _ => body.push(s),
            }
        }
        // GOSUB: las etiquetas a las que se llama y que terminan en RETURN son subrutinas (funciones)
        let mut gosubs = BTreeSet::new();
        for s in &body {
            collect_gosubs(s, &mut gosubs);
        }
        let labels: BTreeSet<&str> = body.iter().filter_map(|s| if let BStmtKind::Label(l) = &s.kind { Some(l.as_str()) } else { None }).collect();
        for g in &gosubs {
            if !labels.contains(g.as_str()) {
                self.err("PYB0072", stmts.iter().find(|s| matches!(&s.kind, BStmtKind::Gosub(x) if x == g)).map_or(Span::new(0, 0), |s| s.span), format!("GOSUB {g}: no hay una etiqueta '{g}:' al principio del programa"));
            }
        }
        let mut main_body: Vec<&BStmt> = Vec::new();
        let mut subs: Vec<(String, Span, Vec<&BStmt>)> = Vec::new();
        let mut it = body.iter().peekable();
        while let Some(s) = it.next() {
            match &s.kind {
                BStmtKind::Label(l) if gosubs.contains(l) => {
                    let mut sub: Vec<&BStmt> = Vec::new();
                    let mut closed = false;
                    while let Some(n) = it.peek() {
                        if matches!(&n.kind, BStmtKind::Label(x) if gosubs.contains(x)) {
                            break;
                        }
                        let n = it.next().unwrap();
                        sub.push(n);
                        if matches!(n.kind, BStmtKind::Return) {
                            closed = true;
                            break;
                        }
                    }
                    if !closed {
                        self.err("PYB0073", s.span, format!("la subrutina '{l}' no termina con RETURN"));
                    }
                    subs.push((l.clone(), s.span, sub));
                }
                _ => main_body.push(s),
            }
        }
        let mut funcs: Vec<Item> = Vec::new();
        for (name, span, sub) in &subs {
            let owned: Vec<BStmt> = sub.iter().map(|s| (*s).clone()).collect();
            let mut cx = Fn { is_sub: true, ..Default::default() };
            let b = self.block(&owned, &mut cx);
            funcs.push(self.func(name, *span, b, cx.assigned));
        }
        let owned: Vec<BStmt> = main_body.iter().map(|s| (*s).clone()).collect();
        let mut cx = Fn::default();
        let mut b = self.block(&owned, &mut cx);
        // arranque de main: direccion de cada pin que se usa y el ADC encendido
        let mut prologue: Vec<Stmt> = Vec::new();
        let sp0 = Span::new(0, 0);
        let pins: Vec<((char, u8), bool)> = self.pin_out.iter().map(|(p, o)| (*p, *o)).collect();
        for (p, as_output) in pins {
            let c = self.method(sp0, &Self::pin_ident(p), if as_output { "output" } else { "input" }, Vec::new(), Vec::new());
            prologue.push(self.st(sp0, StmtKind::Expr(c)));
        }
        if self.uses_adc {
            let c = self.method(sp0, "ADC", "start", Vec::new(), Vec::new());
            prologue.push(self.st(sp0, StmtKind::Expr(c)));
        }
        prologue.append(&mut b);
        funcs.push(self.func("main", sp0, prologue, cx.assigned));

        // w0 comparte memoria con b0 y b1 en PICAXE: aqui no se solapan, asi que usar los dos seria un error silencioso
        let used: Vec<String> = self.decl_order.clone();
        for n in &used {
            if let Some(('w', k)) = implicit(n) {
                for b in [2 * k, 2 * k + 1] {
                    let bn = format!("b{b}");
                    if used.contains(&bn) {
                        let span = self.first_use.get(&bn).copied().unwrap_or(Span::new(0, 0));
                        self.err("PYB0074", span, format!("'{bn}' y '{n}' comparten memoria en PICAXE ({n} = b{}:b{}) y PyPIC BASIC todavia no las solapa: usa solo una de las dos", 2 * k, 2 * k + 1));
                    }
                }
            }
        }

        // modulos: dispositivo, gpio (pines), system.time (pausas), los del programa y las declaraciones
        let mut gpio: BTreeSet<String> = self.pin_out.keys().map(|p| Self::pin_ident(*p)).collect();
        let mut explicit: Vec<(String, Span, Vec<(String, Span)>)> = Vec::new();
        for (m, sp, names) in std::mem::take(&mut self.explicit) {
            if m == "pypic.periph.gpio" {
                gpio.extend(names.into_iter().map(|(n, _)| n));
            } else {
                explicit.push((m, sp, names));
            }
        }
        if let Some((n, sp)) = &device {
            items.push(Item::Import { span: *sp, module: "pypic.device".into(), names: vec![Ident { name: n.clone(), span: *sp }] });
        }
        if !gpio.is_empty() {
            items.push(Item::Import { span: Span::new(0, 0), module: "pypic.periph.gpio".into(), names: gpio.into_iter().map(|n| Ident { name: n, span: Span::new(0, 0) }).collect() });
        }
        let mut time: Vec<&str> = Vec::new();
        if self.uses_pause {
            time.push("pause");
        }
        if self.uses_pause_us {
            time.push("delay_us");
        }
        if !time.is_empty() {
            items.push(Item::Import { span: Span::new(0, 0), module: "pypic.system.time".into(), names: time.into_iter().map(|n| Ident { name: n.to_string(), span: Span::new(0, 0) }).collect() });
        }
        if self.uses_adc {
            // ADC y los canales usados; si el programa ya importo alguno, el suyo manda (un nombre no se importa dos veces)
            let have: BTreeSet<String> = explicit.iter().filter(|(m, ..)| m == "pypic.periph.adc").flat_map(|(_, _, ns)| ns.iter().map(|(n, _)| n.clone())).collect();
            let mut names: Vec<String> = vec!["ADC".to_string()];
            names.extend(self.adc_channels.iter().map(|c| format!("AN{c}")));
            names.retain(|n| !have.contains(n));
            if !names.is_empty() {
                items.push(Item::Import { span: Span::new(0, 0), module: "pypic.periph.adc".into(), names: names.into_iter().map(|n| Ident { name: n, span: Span::new(0, 0) }).collect() });
            }
        }
        for (m, sp, names) in explicit {
            items.push(Item::Import { span: sp, module: m, names: names.into_iter().map(|(n, s)| Ident { name: n, span: s }).collect() });
        }
        items.extend(clock);
        let order = self.decl_order.clone();
        for key in order {
            let v = self.vars[&key].clone();
            let span = self.first_use.get(&key).copied().unwrap_or(Span::new(0, 0));
            let base = TypeExpr { span, kind: TypeExprKind::Name(if v.bits == 8 { "uint8" } else { "uint16" }.to_string()) };
            match v.array {
                Some(n) => {
                    let ann = TypeExpr { span, kind: TypeExprKind::Array(Box::new(base), n as i128) };
                    items.push(Item::Var(VarDecl { span, name: Ident { name: v.name.clone(), span }, ann, value: None }));
                }
                None => {
                    let zero = self.int(span, 0);
                    items.push(Item::Var(VarDecl { span, name: Ident { name: v.name.clone(), span }, ann: base, value: Some(zero) }));
                }
            }
        }
        items.extend(funcs);
        Module { items, expr_count: self.next_id }
    }

    fn func(&mut self, name: &str, span: Span, mut body: Vec<Stmt>, assigned: BTreeSet<String>) -> Item {
        if !assigned.is_empty() {
            let names: Vec<Ident> = assigned.into_iter().map(|n| Ident { name: n, span }).collect();
            body.insert(0, Stmt { span, kind: StmtKind::Global(names) });
        }
        Item::Func(FuncDef { span, name: Ident { name: name.to_string(), span }, decorators: Vec::new(), params: Vec::new(), ret: Some(TypeExpr { span, kind: TypeExprKind::None }), body, asm: None })
    }

    fn declare(&mut self, name: &str, span: Span) -> bool {
        let key = name.to_ascii_lowercase();
        if self.vars.contains_key(&key) || self.consts.contains_key(&key) || self.pins.contains_key(&key) || self.aliases.contains_key(&key) || implicit(name).is_some() {
            self.err("PYB0080", span, format!("'{name}' ya esta definido (b0 a b27, w0 a w13 y los nombres de SYMBOL o DIM no se repiten)"));
            return false;
        }
        true
    }

    fn symbol(&mut self, name: &str, value: &BExpr, span: Span) {
        if !self.declare(name, span) {
            return;
        }
        let key = name.to_ascii_lowercase();
        if let BExprKind::Pin(p, b) = &value.kind {
            self.pins.insert(key, (*p, *b));
        } else if let Some(v) = self.const_eval(value) {
            self.consts.insert(key, v);
        } else if let BExprKind::Name(n) = &value.kind {
            if matches!(self.resolve(n, value.span), Res::Var(_)) {
                let target = self.vars.get(&n.to_ascii_lowercase()).map(|v| v.name.clone()).unwrap_or_else(|| n.to_ascii_lowercase());
                self.aliases.insert(key, target);
            } else {
                self.err("PYB0081", value.span, format!("SYMBOL {name} = ...: se esperaba un numero, una variable (b0, w1) o un pin (B.0)"));
            }
        } else {
            self.err("PYB0081", value.span, format!("SYMBOL {name} = ...: se esperaba un numero, una variable (b0, w1) o un pin (B.0)"));
        }
    }

    fn dim(&mut self, name: &str, bits: u8, len: Option<i128>, init: Option<&BExpr>, span: Span) {
        if !self.declare(name, span) {
            return;
        }
        if len.is_some() && bits != 8 {
            self.err("PYB0082", span, "los arreglos son de BYTE por ahora (DIM tabla(16) AS BYTE)");
            return;
        }
        if init.is_some() {
            self.err("PYB0083", span, "DIM con valor inicial todavia no esta soportado: asigna el valor al empezar el programa");
        }
        let key = name.to_ascii_lowercase();
        self.vars.insert(key.clone(), Var { name: name.to_string(), bits, array: len.map(|n| n as usize) });
        self.decl_order.push(key.clone());
        self.first_use.insert(key, span);
    }
}

fn collect_gosubs(s: &BStmt, out: &mut BTreeSet<String>) {
    match &s.kind {
        BStmtKind::Gosub(l) => {
            out.insert(l.clone());
        }
        BStmtKind::If { arms, orelse } => {
            arms.iter().for_each(|(_, b)| b.iter().for_each(|x| collect_gosubs(x, out)));
            orelse.iter().for_each(|x| collect_gosubs(x, out));
        }
        BStmtKind::For { body, .. } | BStmtKind::While { body, .. } | BStmtKind::Do { body, .. } => body.iter().for_each(|x| collect_gosubs(x, out)),
        BStmtKind::Select { cases, default, .. } => {
            cases.iter().for_each(|(_, b)| b.iter().for_each(|x| collect_gosubs(x, out)));
            default.iter().for_each(|x| collect_gosubs(x, out));
        }
        _ => {}
    }
}
