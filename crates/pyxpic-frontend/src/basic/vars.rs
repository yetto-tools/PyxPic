//! La lista de variables de un programa BASIC, como la del IDE de PICAXE: las predefinidas (`b0`..`b27`, `w0`..`w13`), las que declara `DIM` y los
//! alias de `SYMBOL`, cada una con su tipo, la linea donde se declara y si el programa la usa. Solo lee el programa (no lo compila), asi que
//! sirve mientras se escribe.

use std::collections::HashSet;

use super::parse::{self, BArg, BExpr, BExprKind, BStmt, BStmtKind, CaseItem};
use crate::diag::Diagnostics;

/// Una variable del programa.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VarInfo {
    pub name: String,
    /// `predefined` (b0..b27, w0..w13), `dim` (`DIM x AS BYTE`) o `symbol` (`SYMBOL led = RB0`).
    pub kind: &'static str,
    /// `BYTE`, `WORD`, `BIT` o `-` (un alias no tiene tipo propio).
    pub ty: String,
    /// Elementos de un arreglo (`DIM buf(16) AS BYTE`).
    pub len: Option<i128>,
    /// Linea (desde 1) donde se declara; `None` en las predefinidas.
    pub line: Option<usize>,
    /// El programa la menciona en algun lugar fuera de su declaracion.
    pub used: bool,
    /// Valor inicial de un `DIM` o lo que nombra un `SYMBOL` (`RB0`, `5`, `b3`).
    pub detail: String,
}

fn line_of(source: &str, offset: u32) -> usize {
    1 + source.as_bytes()[..(offset as usize).min(source.len())].iter().filter(|&&b| b == b'\n').count()
}

fn describe(e: &BExpr) -> String {
    match &e.kind {
        BExprKind::Int(v) => v.to_string(),
        BExprKind::Quantity(v, d) => format!("{v} {}", d.base_unit()),
        BExprKind::Name(n) => n.clone(),
        BExprKind::Pin(p, b) => format!("R{}{}", p.to_ascii_uppercase(), b),
        BExprKind::Str(s) => format!("\"{s}\""),
        BExprKind::Neg(x) => format!("-{}", describe(x)),
        _ => "expresion".to_string(),
    }
}

fn walk_expr(e: &BExpr, used: &mut HashSet<String>) {
    match &e.kind {
        BExprKind::Name(n) => {
            used.insert(n.to_ascii_uppercase());
        }
        BExprKind::Dec(x) | BExprKind::Neg(x) | BExprKind::Not(x) => walk_expr(x, used),
        BExprKind::Bin(_, l, r) | BExprKind::Cmp(_, l, r) => {
            walk_expr(l, used);
            walk_expr(r, used);
        }
        BExprKind::Call { obj, name, args } => {
            if obj.is_none() {
                used.insert(name.to_ascii_uppercase());
            }
            walk_args(args, used);
        }
        _ => {}
    }
}

fn walk_args(args: &[BArg], used: &mut HashSet<String>) {
    for a in args {
        walk_expr(&a.value, used);
    }
}

fn walk(stmts: &[BStmt], used: &mut HashSet<String>) {
    for s in stmts {
        match &s.kind {
            BStmtKind::Assign { target, index, value, .. } => {
                used.insert(target.to_ascii_uppercase());
                if let Some(i) = index {
                    walk_expr(i, used);
                }
                walk_expr(value, used);
            }
            BStmtKind::Baud(e) => walk_expr(e, used),
            BStmtKind::Symbol { value, .. } => walk_expr(value, used),
            BStmtKind::Dim { init: Some(e), .. } => walk_expr(e, used),
            BStmtKind::If { arms, orelse } => {
                for (c, body) in arms {
                    walk_expr(c, used);
                    walk(body, used);
                }
                walk(orelse, used);
            }
            BStmtKind::For { var, from, to, step, body, .. } => {
                used.insert(var.to_ascii_uppercase());
                walk_expr(from, used);
                walk_expr(to, used);
                if let Some(e) = step {
                    walk_expr(e, used);
                }
                walk(body, used);
            }
            BStmtKind::Do { pre, post, body } => {
                for (_, c) in pre.iter().chain(post.iter()) {
                    walk_expr(c, used);
                }
                walk(body, used);
            }
            BStmtKind::While { cond, body } => {
                walk_expr(cond, used);
                walk(body, used);
            }
            BStmtKind::Select { subject, cases, default } => {
                walk_expr(subject, used);
                for (items, body) in cases {
                    for it in items {
                        match it {
                            CaseItem::Value(v) => walk_expr(v, used),
                            CaseItem::Range(a, b) => {
                                walk_expr(a, used);
                                walk_expr(b, used);
                            }
                        }
                    }
                    walk(body, used);
                }
                walk(default, used);
            }
            BStmtKind::Cmd { args, .. } => walk_args(args, used),
            _ => {}
        }
    }
}

/// Las variables del programa: primero las declaradas (`DIM`, `SYMBOL`) en el orden del fuente y despues las predefinidas.
pub fn variables(source: &str) -> Vec<VarInfo> {
    let mut d = Diagnostics::default();
    let stmts = parse::parse(source, &mut d);
    let mut used = HashSet::new();
    walk(&stmts, &mut used);
    let mut out = Vec::new();
    // lo que se menciona fuera de la propia declaracion: para un DIM o un SYMBOL, cualquier otra aparicion del nombre
    for s in &stmts {
        match &s.kind {
            BStmtKind::Dim { name, bits, len, init } => out.push(VarInfo {
                name: name.clone(),
                kind: "dim",
                ty: match bits {
                    1 => "BIT",
                    16 => "WORD",
                    _ => "BYTE",
                }
                .into(),
                len: *len,
                line: Some(line_of(source, s.span.start)),
                used: used.contains(&name.to_ascii_uppercase()),
                detail: init.as_ref().map(describe).unwrap_or_default(),
            }),
            BStmtKind::Symbol { name, value } => out.push(VarInfo {
                name: name.clone(),
                kind: "symbol",
                ty: "-".into(),
                len: None,
                line: Some(line_of(source, s.span.start)),
                used: used.contains(&name.to_ascii_uppercase()),
                detail: describe(value),
            }),
            _ => {}
        }
    }
    for i in 0..28 {
        let name = format!("b{i}");
        out.push(VarInfo { used: used.contains(&name.to_ascii_uppercase()), name, kind: "predefined", ty: "BYTE".into(), len: None, line: None, detail: String::new() });
    }
    for i in 0..14 {
        let name = format!("w{i}");
        out.push(VarInfo { used: used.contains(&name.to_ascii_uppercase()), name, kind: "predefined", ty: "WORD".into(), len: None, line: None, detail: String::new() });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find<'a>(v: &'a [VarInfo], n: &str) -> &'a VarInfo {
        v.iter().find(|x| x.name == n).unwrap()
    }

    #[test]
    fn lists_predefined_declared_and_alias_variables_with_their_usage() {
        let src = "DEVICE PIC16F887\nDIM contador AS BYTE\nDIM total AS WORD = 1000\nDIM buf(16) AS BYTE\nSYMBOL led = RB0\nSYMBOL limite = 25\nmain:\n  SERRXD(b0)\n  contador = contador + 1\n  IF b0 = \"a\" THEN\n    HIGH led\n  ENDIF\n  w3 = 5\n  GOTO main\n";
        let v = variables(src);
        assert_eq!(v.iter().filter(|x| x.kind == "predefined").count(), 42);
        assert!(find(&v, "b0").used && find(&v, "w3").used);
        assert!(!find(&v, "b1").used && !find(&v, "w0").used);
        let c = find(&v, "contador");
        assert_eq!((c.kind, c.ty.as_str(), c.line, c.used), ("dim", "BYTE", Some(2), true));
        let t = find(&v, "total");
        assert_eq!((t.ty.as_str(), t.detail.as_str(), t.used), ("WORD", "1000", false));
        let b = find(&v, "buf");
        assert_eq!((b.len, b.used), (Some(16), false));
        let l = find(&v, "led");
        assert_eq!((l.kind, l.detail.as_str(), l.used, l.line), ("symbol", "RB0", true, Some(5)));
        assert_eq!((find(&v, "limite").detail.as_str(), find(&v, "limite").used), ("25", false));
    }

    #[test]
    fn usage_ignores_case_and_looks_inside_every_statement() {
        let src = "DIM n AS BYTE\nDIM m AS BYTE\nDIM k AS BYTE\nmain:\n  FOR n = 1 TO 5\n    SERTXD(#N, CR)\n  NEXT\n  SELECT CASE M\n    CASE 1\n      B1 = K\n  END SELECT\n  GOTO main\n";
        let v = variables(src);
        assert!(find(&v, "n").used && find(&v, "m").used && find(&v, "k").used && find(&v, "b1").used);
    }

    #[test]
    fn a_program_with_syntax_errors_still_lists_the_predefined_ones() {
        let v = variables("esto no es BASIC (((\n");
        assert_eq!(v.iter().filter(|x| x.kind == "predefined").count(), 42);
    }
}
