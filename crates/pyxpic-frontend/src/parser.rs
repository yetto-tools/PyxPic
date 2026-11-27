//! Parser descendente recursivo del subconjunto PyxPic.
//!
//! Todo lo que Python permite pero PyxPic no (clases, listas, floats, lambdas,
//! `try`, etc.) se rechaza aqui con un diagnostico especifico.

use crate::ast::*;
use crate::diag::{Diagnostics, Span};
use crate::lexer::{lex, Tok, Token};

type R<T> = Result<T, ()>;

const UNSUPPORTED_KEYWORDS: &[&str] =
    &["class", "try", "except", "finally", "with", "del", "assert", "raise", "yield", "async", "await", "match", "nonlocal", "lambda", "import", "from", "def", "else", "elif", "is", "in", "as"];

fn aug_op(op: &str) -> BinOp {
    match op {
        "+" => BinOp::Add,
        "-" => BinOp::Sub,
        "*" => BinOp::Mul,
        "//" => BinOp::Div,
        "%" => BinOp::Mod,
        "&" => BinOp::BitAnd,
        "|" => BinOp::BitOr,
        "^" => BinOp::BitXor,
        "<<" => BinOp::Shl,
        _ => BinOp::Shr,
    }
}

pub fn parse(src: &str, d: &mut Diagnostics) -> Module {
    let toks = lex(src, d);
    let mut p = Parser { toks, pos: 0, d, next_id: 0 };
    let items = p.module();
    Module { items, expr_count: p.next_id }
}

struct Parser<'a> {
    toks: Vec<Token>,
    pos: usize,
    d: &'a mut Diagnostics,
    next_id: u32,
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Ident(s) => format!("'{s}'"),
        Tok::Int(_) | Tok::Quantity(..) | Tok::Decimal(..) => "un numero".into(),
        Tok::Str(..) => "una cadena".into(),
        Tok::FStr(..) => "una f-string".into(),
        Tok::Newline => "fin de linea".into(),
        Tok::Indent => "una indentacion".into(),
        Tok::Dedent => "el fin del bloque".into(),
        Tok::Eof => "fin del archivo".into(),
        Tok::Other(c) => format!("'{c}'"),
        other => format!("{other:?}"),
    }
}

impl<'a> Parser<'a> {
    // ------------------------------------------------------------ utilidades
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }
    fn peek_at(&self, n: usize) -> &Tok {
        &self.toks[(self.pos + n).min(self.toks.len() - 1)].tok
    }
    fn span(&self) -> Span {
        self.toks[self.pos].span
    }
    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }
    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }
    fn at(&self, t: &Tok) -> bool {
        self.peek() == t
    }
    fn at_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Tok::Ident(s) if s == kw)
    }
    fn eat(&mut self, t: &Tok) -> bool {
        if self.at(t) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn err<T>(&mut self, code: &'static str, span: Span, msg: impl Into<String>) -> R<T> {
        self.d.error(code, span, msg);
        Err(())
    }
    fn unexpected<T>(&mut self, expected: &str) -> R<T> {
        let found = describe(self.peek());
        let sp = self.span();
        self.err("PYP1100", sp, format!("se esperaba {expected}, se encontro {found}"))
    }
    fn expect(&mut self, t: Tok, what: &str) -> R<Span> {
        if self.at(&t) {
            Ok(self.bump().span)
        } else {
            self.unexpected(what)
        }
    }
    fn ident(&mut self, what: &str) -> R<Ident> {
        if let Tok::Ident(s) = self.peek().clone() {
            if !UNSUPPORTED_KEYWORDS.contains(&s.as_str()) {
                let span = self.bump().span;
                return Ok(Ident { name: s, span });
            }
        }
        self.unexpected(what)
    }
    /// Descarta los tokens hasta el fin de la linea (tras un error ya reportado en ella).
    fn skip_to_line_end(&mut self) {
        while !self.at(&Tok::Newline) && !self.at(&Tok::Eof) {
            self.bump();
        }
    }
    fn mk(&mut self, span: Span, kind: ExprKind) -> Expr {
        let id = self.next_id;
        self.next_id += 1;
        Expr { id, span, kind }
    }

    /// Recuperacion: descarta el resto de la linea y, si le sigue un bloque
    /// indentado, tambien el bloque entero.
    fn sync(&mut self) {
        loop {
            match self.peek() {
                Tok::Eof => return,
                Tok::Newline => {
                    self.bump();
                    break;
                }
                _ => {
                    self.bump();
                }
            }
        }
        if self.at(&Tok::Indent) {
            let mut depth = 0;
            loop {
                match self.peek() {
                    Tok::Indent => depth += 1,
                    Tok::Dedent => {
                        depth -= 1;
                        if depth == 0 {
                            self.bump();
                            return;
                        }
                    }
                    Tok::Eof => return,
                    _ => {}
                }
                self.bump();
            }
        }
    }

    fn end_stmt(&mut self) -> R<()> {
        match self.peek() {
            Tok::Newline => {
                self.bump();
                Ok(())
            }
            Tok::Eof | Tok::Dedent => Ok(()),
            Tok::Other(';') => {
                let sp = self.span();
                self.err("PYP1101", sp, "';' no soportado: una sentencia por linea")
            }
            Tok::Assign => {
                let sp = self.span();
                self.err("PYP1012", sp, "asignacion multiple no soportada")
            }
            _ => self.unexpected("fin de linea"),
        }
    }

    // ---------------------------------------------------------------- modulo
    fn module(&mut self) -> Vec<Item> {
        let mut items = Vec::new();
        let mut first = true;
        while !self.at(&Tok::Eof) {
            let before = self.pos;
            let r = self.item(first, &mut items);
            first = false;
            if r.is_err() {
                self.sync();
            }
            if self.pos == before {
                self.bump();
            }
        }
        items
    }

    fn item(&mut self, first: bool, items: &mut Vec<Item>) -> R<()> {
        match self.peek().clone() {
            Tok::At => items.push(Item::Func(self.func()?)),
            Tok::Ident(s) if s == "def" => items.push(Item::Func(self.func()?)),
            Tok::Ident(s) if s == "from" => items.push(self.import_from()?),
            Tok::Ident(s) if s == "import" => {
                let sp = self.span();
                return self.err("PYP1002", sp, "'import' no soportado todavia; solo 'from pyxpic import <tipos>'");
            }
            Tok::Str(..) if first => {
                self.bump();
                self.end_stmt()?;
            }
            Tok::Ident(s) if !UNSUPPORTED_KEYWORDS.contains(&s.as_str()) && *self.peek_at(1) == Tok::Colon => {
                items.push(Item::Var(self.var_decl()?));
            }
            _ => {
                let sp = self.span();
                return self.err("PYP1001", sp, "a nivel de modulo solo se permiten declaraciones tipadas (x: u8 = 0), funciones e imports de 'pyxpic'");
            }
        }
        Ok(())
    }

    fn import_from(&mut self) -> R<Item> {
        let start = self.bump().span; // from
        let mut module = self.ident("un nombre de modulo")?;
        while *self.peek() == Tok::Other('.') {
            self.bump();
            let part = self.ident("un nombre de modulo")?;
            module.name = format!("{}.{}", module.name, part.name);
            module.span = module.span.to(part.span);
        }
        if !self.at_kw("import") {
            return self.unexpected("'import'");
        }
        self.bump();
        let mut names = vec![self.ident("un nombre")?];
        while self.eat(&Tok::Comma) {
            names.push(self.ident("un nombre")?);
        }
        if self.at_kw("as") {
            let sp = self.span();
            return self.err("PYP1002", sp, "'as' no soportado en imports");
        }
        let span = start.to(self.prev_span());
        self.end_stmt()?;
        Ok(Item::Import { span, module: module.name, names })
    }

    fn func(&mut self) -> R<FuncDef> {
        let start = self.span();
        let mut decorators = Vec::new();
        while self.eat(&Tok::At) {
            decorators.push(self.ident("el nombre del decorador")?);
            if self.at(&Tok::LParen) {
                let sp = self.span();
                return self.err("PYP1004", sp, "los decoradores con argumentos no estan soportados");
            }
            self.expect(Tok::Newline, "fin de linea")?;
        }
        if !self.at_kw("def") {
            return self.unexpected("'def'");
        }
        self.bump();
        let name = self.ident("el nombre de la funcion")?;
        self.expect(Tok::LParen, "'('")?;
        let mut params = Vec::new();
        while !self.at(&Tok::RParen) {
            if matches!(self.peek(), Tok::Star | Tok::StarStar) {
                let sp = self.span();
                return self.err("PYP1003", sp, "*args y **kwargs no estan soportados");
            }
            let pname = self.ident("el nombre del parametro")?;
            if !self.at(&Tok::Colon) {
                return self.err("PYP2003", pname.span, format!("el parametro '{}' necesita anotacion de tipo", pname.name));
            }
            self.bump();
            let ann = self.type_expr(false)?;
            if self.at(&Tok::Assign) {
                let sp = self.span();
                return self.err("PYP1003", sp, "los valores por defecto no estan soportados");
            }
            params.push(Param { name: pname, ann });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::RParen, "')'")?;
        let ret = if self.eat(&Tok::Arrow) { Some(self.type_expr(true)?) } else { None };
        let (body, asm) = if decorators.iter().any(|d| d.name == "asm_function") { (Vec::new(), Some(self.asm_body()?)) } else { (self.block(true)?, None) };
        let span = start.to(self.prev_span());
        Ok(FuncDef { span, name, decorators, params, ret, body, asm })
    }

    /// Cuerpo de una funcion `@asm_function`: una unica cadena con el ensamblador.
    fn asm_body(&mut self) -> R<AsmSource> {
        self.expect(Tok::Colon, "':'")?;
        self.expect(Tok::Newline, "una linea nueva")?;
        self.expect(Tok::Indent, "un bloque indentado")?;
        let tok = self.bump();
        let Tok::Str(text, quote) = tok.tok else {
            return self.err("PYP2110", tok.span, "el cuerpo de una funcion @asm_function es una unica cadena con el ensamblador");
        };
        self.end_stmt()?;
        if !self.at(&Tok::Dedent) {
            let sp = self.span();
            return self.err("PYP2110", sp, "una funcion @asm_function solo contiene la cadena de ensamblador");
        }
        self.bump();
        Ok(AsmSource { text, offset: tok.span.start as usize + quote as usize, first_line: 0, span: tok.span })
    }

    fn type_expr(&mut self, allow_none: bool) -> R<TypeExpr> {
        let sp = self.span();
        match self.peek().clone() {
            Tok::Ident(s) if s == "None" && allow_none => {
                self.bump();
                Ok(TypeExpr { span: sp, kind: TypeExprKind::None })
            }
            // una sola forma: la palabra de Python no dice nada en un PIC
            Tok::Ident(s) if s == "Final" => self.err("PYP1030", sp, "'Final' no existe en PyxPic: una constante se declara Const[u32] (el valor se fija al compilar y no ocupa RAM)"),
            Tok::Ident(s) if s == "Const" && !matches!(self.peek_at(1), Tok::LBracket) => {
                self.bump();
                Ok(TypeExpr { span: sp, kind: TypeExprKind::ConstAuto })
            }
            Tok::Ident(s) if s == "Const" => {
                self.bump();
                self.expect(Tok::LBracket, "'[' (Const[tipo])")?;
                let inner = self.type_expr(false)?;
                let end = self.expect(Tok::RBracket, "']'")?;
                Ok(TypeExpr { span: sp.to(end), kind: TypeExprKind::Const(Box::new(inner)) })
            }
            Tok::Ident(s) if (s == "Fixed" || s == "QFixed") && matches!(self.peek_at(1), Tok::LBracket) => {
                self.bump();
                self.bump();
                let base = self.ident("el tipo entero del punto fijo (Fixed[int16, 2])")?;
                self.expect(Tok::Comma, "',' (Fixed[tipo, decimales])")?;
                let scale = match self.peek().clone() {
                    Tok::Int(n) => {
                        self.bump();
                        n
                    }
                    _ => return self.unexpected("el numero de decimales (Fixed[int16, 2])"),
                };
                let end = self.expect(Tok::RBracket, "']'")?;
                Ok(TypeExpr { span: sp.to(end), kind: TypeExprKind::Fixed { binary: s == "QFixed", base: base.name, scale } })
            }
            Tok::Ident(s) if s == "Rom" => {
                self.bump();
                self.expect(Tok::LBracket, "'[' (Rom[tipo])")?;
                let inner = self.type_expr(false)?;
                let end = self.expect(Tok::RBracket, "']'")?;
                Ok(TypeExpr { span: sp.to(end), kind: TypeExprKind::Rom(Box::new(inner)) })
            }
            Tok::Ident(s) if crate::clock::ClockKind::parse(&s).is_some() => {
                self.bump();
                if self.at(&Tok::LParen) || self.at(&Tok::LBracket) {
                    self.d.error("PYP2190", sp, format!("la frecuencia del reloj se declara aparte: 'CLOCK: {s}' y, en otra linea, 'FREQUENCY: 8MHz'"));
                    self.skip_to_line_end();
                }
                Ok(TypeExpr { span: sp, kind: TypeExprKind::Clock(s) })
            }
            Tok::Ident(s) if s == "ExternalClock" => {
                self.bump();
                self.d.error("PYP2190", sp, "ExternalClock ya no existe: usa 'CLOCK: External' (senal de reloj en CLKIN) o 'CLOCK: Crystal'");
                Ok(TypeExpr { span: sp, kind: TypeExprKind::Name(s) })
            }
            Tok::Ident(s) if !UNSUPPORTED_KEYWORDS.contains(&s.as_str()) => {
                self.bump();
                if self.at(&Tok::LBracket) {
                    if let Tok::Int(n) = self.peek_at(1).clone() {
                        self.bump();
                        self.bump();
                        let end = self.expect(Tok::RBracket, "']'")?;
                        let elem = TypeExpr { span: sp, kind: TypeExprKind::Name(s) };
                        return Ok(TypeExpr { span: sp.to(end), kind: TypeExprKind::Array(Box::new(elem), n) });
                    }
                }
                Ok(TypeExpr { span: sp, kind: TypeExprKind::Name(s) })
            }
            _ => self.err("PYP2001", sp, "tipo no valido; usa u8, i8, u16, i16, u32, i32 o bool"),
        }
    }

    fn var_decl(&mut self) -> R<VarDecl> {
        let name = self.ident("un nombre")?;
        self.expect(Tok::Colon, "':'")?;
        // `FREQUENCY: 8MHz` lleva una expresion constante, no un tipo; `OSC_MODE: HS` un nombre de modo
        let ann = if name.name == "FREQUENCY" {
            let sp = self.span();
            let e = self.expr()?;
            TypeExpr { span: sp.to(self.prev_span()), kind: TypeExprKind::Frequency(Box::new(e)) }
        } else if matches!(name.name.as_str(), "PLL" | "CLOCK_DIVIDER" | "CLOCK_OUTPUT" | "FAIL_SAFE_CLOCK" | "CLOCK_SWITCHING") {
            let sp = self.span();
            if let Tok::Ident(w) = self.peek().clone() {
                self.bump();
                TypeExpr { span: sp, kind: TypeExprKind::Setting(Some(w), None) }
            } else {
                let e = self.expr()?;
                TypeExpr { span: sp.to(self.prev_span()), kind: TypeExprKind::Setting(None, Some(Box::new(e))) }
            }
        } else if name.name == "OSC_MODE" {
            let m = self.ident("el modo del oscilador (Auto, LP, XT, HS, EC...)")?;
            TypeExpr { span: m.span, kind: TypeExprKind::OscMode(m.name) }
        } else {
            self.type_expr(false)?
        };
        let value = if self.eat(&Tok::Assign) { Some(self.expr()?) } else { None };
        let span = name.span.to(self.prev_span());
        self.end_stmt()?;
        Ok(VarDecl { span, name, ann, value })
    }

    // ------------------------------------------------------------ sentencias
    /// `:` NEWLINE INDENT stmts DEDENT
    fn block(&mut self, allow_docstring: bool) -> R<Vec<Stmt>> {
        self.expect(Tok::Colon, "':'")?;
        if !self.at(&Tok::Newline) {
            let sp = self.span();
            return self.err("PYP1102", sp, "el cuerpo debe ir en una linea nueva e indentada");
        }
        self.bump();
        if !self.at(&Tok::Indent) {
            return self.unexpected("un bloque indentado");
        }
        self.bump();
        let mut out = Vec::new();
        let mut first = allow_docstring;
        while !matches!(self.peek(), Tok::Dedent | Tok::Eof) {
            let before = self.pos;
            if first && matches!(self.peek(), Tok::Str(..)) {
                self.bump();
                if self.end_stmt().is_err() {
                    self.sync();
                }
            } else {
                match self.stmt() {
                    Ok(s) => out.push(s),
                    Err(()) => self.sync(),
                }
            }
            first = false;
            if self.pos == before {
                self.bump();
            }
        }
        self.eat(&Tok::Dedent);
        Ok(out)
    }

    fn stmt(&mut self) -> R<Stmt> {
        let start = self.span();
        if let Tok::Ident(s) = self.peek().clone() {
            match s.as_str() {
                "if" => return self.if_stmt(),
                "while" => {
                    self.bump();
                    let test = self.expr()?;
                    let body = self.block(false)?;
                    if self.at_kw("else") {
                        let sp = self.span();
                        return self.err("PYP1014", sp, "'while ... else' no soportado");
                    }
                    return Ok(Stmt { span: start.to(self.prev_span()), kind: StmtKind::While { test, body } });
                }
                "for" => return self.for_stmt(),
                "return" => {
                    self.bump();
                    let v = if matches!(self.peek(), Tok::Newline | Tok::Eof | Tok::Dedent) { None } else { Some(self.expr()?) };
                    self.end_stmt()?;
                    return Ok(Stmt { span: start.to(self.prev_span()), kind: StmtKind::Return(v) });
                }
                "break" | "continue" | "pass" => {
                    self.bump();
                    self.end_stmt()?;
                    let kind = match s.as_str() {
                        "break" => StmtKind::Break,
                        "continue" => StmtKind::Continue,
                        _ => StmtKind::Pass,
                    };
                    return Ok(Stmt { span: start, kind });
                }
                "global" => {
                    self.bump();
                    let mut names = vec![self.ident("un nombre")?];
                    while self.eat(&Tok::Comma) {
                        names.push(self.ident("un nombre")?);
                    }
                    self.end_stmt()?;
                    return Ok(Stmt { span: start.to(self.prev_span()), kind: StmtKind::Global(names) });
                }
                kw if UNSUPPORTED_KEYWORDS.contains(&kw) => {
                    return self.err("PYP1010", start, format!("'{kw}' no esta soportado en el subconjunto PyxPic"));
                }
                _ => {}
            }
            // sentencia simple que empieza por un nombre
            match self.peek_at(1).clone() {
                Tok::LBracket => {
                    let target = self.ident("un nombre")?;
                    self.bump(); // '['
                    let index = self.expr()?;
                    self.expect(Tok::RBracket, "']'")?;
                    let op = match self.peek().clone() {
                        Tok::Assign => None,
                        Tok::AugAssign(o) => Some(aug_op(o)),
                        _ => return self.err("PYP1011", index.span, "una lectura suelta buf[i] no tiene efecto; para escribir usa buf[i] = valor"),
                    };
                    self.bump();
                    let value = self.expr()?;
                    self.end_stmt()?;
                    return Ok(Stmt { span: start.to(value.span), kind: StmtKind::IndexAssign { target, index, op, value } });
                }
                Tok::Colon => {
                    let decl = self.var_decl()?;
                    return Ok(Stmt { span: decl.span, kind: StmtKind::Decl(decl) });
                }
                Tok::Assign => {
                    let target = self.ident("un nombre")?;
                    self.bump();
                    let value = self.expr()?;
                    self.end_stmt()?;
                    return Ok(Stmt { span: start.to(value.span), kind: StmtKind::Assign { target, value } });
                }
                Tok::AugAssign(op) => {
                    let target = self.ident("un nombre")?;
                    self.bump();
                    let value = self.expr()?;
                    self.end_stmt()?;
                    let op = match op {
                        "+" => BinOp::Add,
                        "-" => BinOp::Sub,
                        "*" => BinOp::Mul,
                        "//" => BinOp::Div,
                        "%" => BinOp::Mod,
                        "&" => BinOp::BitAnd,
                        "|" => BinOp::BitOr,
                        "^" => BinOp::BitXor,
                        "<<" => BinOp::Shl,
                        _ => BinOp::Shr,
                    };
                    return Ok(Stmt { span: start.to(value.span), kind: StmtKind::AugAssign { target, op, value } });
                }
                _ => {}
            }
        }
        // sentencia-expresion: solo llamadas
        let e = self.expr()?;
        if !matches!(e.kind, ExprKind::Call { .. } | ExprKind::MethodCall { .. }) {
            return self.err("PYP1011", e.span, "una expresion suelta no tiene efecto; solo se permiten llamadas");
        }
        self.end_stmt()?;
        Ok(Stmt { span: e.span, kind: StmtKind::Expr(e) })
    }

    fn if_stmt(&mut self) -> R<Stmt> {
        let start = self.bump().span; // if / elif
        let test = self.expr()?;
        let body = self.block(false)?;
        let orelse = if self.at_kw("elif") {
            vec![self.if_stmt()?]
        } else if self.at_kw("else") {
            self.bump();
            self.block(false)?
        } else {
            Vec::new()
        };
        Ok(Stmt { span: start.to(self.prev_span()), kind: StmtKind::If { test, body, orelse } })
    }

    fn for_stmt(&mut self) -> R<Stmt> {
        let start = self.bump().span;
        let var = self.ident("el nombre de la variable del bucle")?;
        if !self.at_kw("in") {
            return self.unexpected("'in'");
        }
        self.bump();
        let iter_span = self.span();
        if !self.at_kw("range") || *self.peek_at(1) != Tok::LParen {
            return self.err("PYP1015", iter_span, "'for' solo admite 'range(stop)', 'range(start, stop)' o 'range(start, stop, step)'");
        }
        self.bump();
        let args = self.call_args()?;
        let iter_span = iter_span.to(self.prev_span());
        if args.is_empty() || args.len() > 3 {
            return self.err("PYP1015", iter_span, "range() recibe de 1 a 3 argumentos");
        }
        let body = self.block(false)?;
        if self.at_kw("else") {
            let sp = self.span();
            return self.err("PYP1014", sp, "'for ... else' no soportado");
        }
        Ok(Stmt { span: start.to(self.prev_span()), kind: StmtKind::For { var, iter_span, args, body } })
    }

    // ----------------------------------------------------------- expresiones
    pub fn expr(&mut self) -> R<Expr> {
        self.or_expr()
    }

    fn or_expr(&mut self) -> R<Expr> {
        let first = self.and_expr()?;
        if !self.at_kw("or") {
            return Ok(first);
        }
        let mut vals = vec![first];
        while self.at_kw("or") {
            self.bump();
            vals.push(self.and_expr()?);
        }
        let span = vals[0].span.to(vals.last().unwrap().span);
        Ok(self.mk(span, ExprKind::Bool_(BoolOp::Or, vals)))
    }

    fn and_expr(&mut self) -> R<Expr> {
        let first = self.not_expr()?;
        if !self.at_kw("and") {
            return Ok(first);
        }
        let mut vals = vec![first];
        while self.at_kw("and") {
            self.bump();
            vals.push(self.not_expr()?);
        }
        let span = vals[0].span.to(vals.last().unwrap().span);
        Ok(self.mk(span, ExprKind::Bool_(BoolOp::And, vals)))
    }

    fn not_expr(&mut self) -> R<Expr> {
        if self.at_kw("not") {
            let start = self.bump().span;
            let e = self.not_expr()?;
            let span = start.to(e.span);
            return Ok(self.mk(span, ExprKind::Unary(UnOp::Not, Box::new(e))));
        }
        self.comparison()
    }

    fn cmp_op(&self) -> Option<CmpOp> {
        Some(match self.peek() {
            Tok::Eq => CmpOp::Eq,
            Tok::Ne => CmpOp::Ne,
            Tok::Lt => CmpOp::Lt,
            Tok::Le => CmpOp::Le,
            Tok::Gt => CmpOp::Gt,
            Tok::Ge => CmpOp::Ge,
            _ => return None,
        })
    }

    fn comparison(&mut self) -> R<Expr> {
        let left = self.bit_or()?;
        if self.at_kw("is") || self.at_kw("in") {
            let sp = self.span();
            return self.err("PYP1023", sp, "los operadores 'is' e 'in' no estan soportados");
        }
        let Some(op) = self.cmp_op() else { return Ok(left) };
        self.bump();
        let right = self.bit_or()?;
        if self.cmp_op().is_some() {
            let sp = self.span();
            return self.err("PYP1023", sp, "comparaciones encadenadas no soportadas; usa 'and'");
        }
        let span = left.span.to(right.span);
        Ok(self.mk(span, ExprKind::Compare(op, Box::new(left), Box::new(right))))
    }

    fn binary(&mut self, op: BinOp, l: Expr, r: Expr) -> Expr {
        let span = l.span.to(r.span);
        self.mk(span, ExprKind::Binary(op, Box::new(l), Box::new(r)))
    }

    fn bit_or(&mut self) -> R<Expr> {
        let mut l = self.bit_xor()?;
        while self.eat(&Tok::Pipe) {
            let r = self.bit_xor()?;
            l = self.binary(BinOp::BitOr, l, r);
        }
        Ok(l)
    }
    fn bit_xor(&mut self) -> R<Expr> {
        let mut l = self.bit_and()?;
        while self.eat(&Tok::Caret) {
            let r = self.bit_and()?;
            l = self.binary(BinOp::BitXor, l, r);
        }
        Ok(l)
    }
    fn bit_and(&mut self) -> R<Expr> {
        let mut l = self.shift()?;
        while self.eat(&Tok::Amp) {
            let r = self.shift()?;
            l = self.binary(BinOp::BitAnd, l, r);
        }
        Ok(l)
    }
    fn shift(&mut self) -> R<Expr> {
        let mut l = self.arith()?;
        loop {
            let op = match self.peek() {
                Tok::Shl => BinOp::Shl,
                Tok::Shr => BinOp::Shr,
                _ => return Ok(l),
            };
            self.bump();
            let r = self.arith()?;
            l = self.binary(op, l, r);
        }
    }
    fn arith(&mut self) -> R<Expr> {
        let mut l = self.term()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => return Ok(l),
            };
            self.bump();
            let r = self.term()?;
            l = self.binary(op, l, r);
        }
    }
    fn term(&mut self) -> R<Expr> {
        let mut l = self.factor()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::SlashSlash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                Tok::Slash => BinOp::TrueDiv,
                Tok::At => {
                    let sp = self.span();
                    return self.err("PYP1022", sp, "'@' no esta soportado");
                }
                _ => return Ok(l),
            };
            self.bump();
            let r = self.factor()?;
            l = self.binary(op, l, r);
        }
    }

    fn factor(&mut self) -> R<Expr> {
        let op = match self.peek() {
            Tok::Minus => UnOp::Neg,
            Tok::Plus => UnOp::Pos,
            Tok::Tilde => UnOp::Invert,
            _ => return self.atom(),
        };
        let start = self.bump().span;
        let e = self.factor()?;
        let span = start.to(e.span);
        Ok(self.mk(span, ExprKind::Unary(op, Box::new(e))))
    }

    /// Argumentos de una llamada. Los de nombre (`prescaler=16`) solo se admiten en los metodos de los
    /// perifericos del SDK (`allow_kw`); van despues de los posicionales y sin repetirse.
    fn call_args_kw(&mut self, allow_kw: bool) -> R<(Vec<Expr>, Vec<Kwarg>)> {
        self.expect(Tok::LParen, "'('")?;
        let mut args = Vec::new();
        let mut kwargs: Vec<Kwarg> = Vec::new();
        while !self.at(&Tok::RParen) {
            if matches!(self.peek(), Tok::Ident(_)) && *self.peek_at(1) == Tok::Assign {
                let sp = self.span();
                if !allow_kw {
                    return self.err("PYP1024", sp, "los argumentos con nombre solo se admiten en los metodos de los perifericos del SDK (TMR2.start(prescaler=16, ...))");
                }
                let name = self.ident("el nombre del argumento")?;
                self.bump(); // '='
                if kwargs.iter().any(|k| k.name == name.name) {
                    return self.err("PYP1028", name.span, format!("el argumento '{}' esta repetido", name.name));
                }
                let value = self.expr()?;
                kwargs.push(Kwarg { name: name.name, name_span: name.span, value });
            } else {
                if !kwargs.is_empty() {
                    let sp = self.span();
                    return self.err("PYP1027", sp, "un argumento posicional no puede ir despues de uno con nombre");
                }
                args.push(self.expr()?);
            }
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::RParen, "')'")?;
        Ok((args, kwargs))
    }

    /// Parte el contenido de una f-string en texto y campos `{nombre:formato}`; `{{` y `}}` son llaves literales. Dentro de un campo
    /// solo se admite el nombre de una variable (no expresiones), para que el costo de cada campo se vea en el formato.
    fn fstring_parts(&mut self, inner: &str, sp: Span) -> Vec<FPart> {
        let base = sp.start as usize + 2; // `f` y la comilla
        let b = inner.as_bytes();
        let mut parts = Vec::new();
        let mut lit = String::new();
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'{' if b.get(i + 1) == Some(&b'{') => {
                    lit.push('{');
                    i += 2;
                }
                b'}' if b.get(i + 1) == Some(&b'}') => {
                    lit.push('}');
                    i += 2;
                }
                b'{' => {
                    let Some(close) = inner[i..].find('}').map(|k| i + k) else {
                        self.d.error("PYP1030", Span::new(base + i, base + inner.len()), "falta la '}' que cierra el campo de la f-string");
                        return parts;
                    };
                    if !lit.is_empty() {
                        parts.push(FPart::Lit(std::mem::take(&mut lit)));
                    }
                    let body = &inner[i + 1..close];
                    let (name, spec) = body.split_once(':').unwrap_or((body, ""));
                    let fsp = Span::new(base + i, base + close + 1);
                    let valid = !name.is_empty() && !name.as_bytes()[0].is_ascii_digit() && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_');
                    if !valid {
                        self.d.error("PYP1030", fsp, format!("en un campo de f-string solo se admite el nombre de una variable, no '{name}': calculalo antes en una variable"));
                    } else if let Some(spec) = self.fspec(spec, fsp) {
                        let expr = self.mk(Span::new(base + i + 1, base + i + 1 + name.len()), ExprKind::Name(name.to_string()));
                        parts.push(FPart::Field { expr: Box::new(expr), spec });
                    }
                    i = close + 1;
                }
                b'}' => {
                    self.d.error("PYP1030", Span::new(base + i, base + i + 1), "'}' suelta en la f-string: para una llave literal escribe '}}'");
                    i += 1;
                }
                _ => {
                    let ch = inner[i..].chars().next().unwrap();
                    lit.push(ch);
                    i += ch.len_utf8();
                }
            }
        }
        if !lit.is_empty() {
            parts.push(FPart::Lit(lit));
        }
        parts
    }

    /// `[0][ancho][d|X|x|b]`
    fn fspec(&mut self, s: &str, sp: Span) -> Option<FSpec> {
        let (mut kind, mut rest) = (FKind::Dec, s);
        if let Some(last) = s.chars().last().filter(|c| c.is_ascii_alphabetic()) {
            kind = match last {
                'd' => FKind::Dec,
                'X' => FKind::HexUpper,
                'x' => FKind::HexLower,
                'b' => FKind::Bin,
                other => {
                    self.d.error("PYP1031", sp, format!("el formato '{other}' no existe (admite: d, X, x, b)"));
                    return None;
                }
            };
            rest = &s[..s.len() - 1];
        }
        let zero = rest.starts_with('0') && rest.len() > 1;
        let digits = if zero { &rest[1..] } else { rest };
        let width = if digits.is_empty() {
            0
        } else if digits.bytes().all(|c| c.is_ascii_digit()) && digits.len() <= 2 && digits.parse::<u32>().is_ok_and(|w| (1..=32).contains(&w)) {
            digits.parse::<u8>().unwrap()
        } else {
            self.d.error("PYP1031", sp, format!("el ancho '{digits}' no es valido: un numero de 1 a 32 (por ejemplo {{v:04X}})"));
            return None;
        };
        if rest == "0" {
            self.d.error("PYP1031", sp, "el formato '0' necesita un ancho: {v:05}");
            return None;
        }
        Some(FSpec { kind, width, zero })
    }

    fn call_args(&mut self) -> R<Vec<Expr>> {
        Ok(self.call_args_kw(false)?.0)
    }

    fn atom(&mut self) -> R<Expr> {
        let sp = self.span();
        let e = match self.peek().clone() {
            Tok::Int(v) => {
                self.bump();
                self.mk(sp, ExprKind::Int(v))
            }
            Tok::Quantity(v, dim) => {
                self.bump();
                self.mk(sp, ExprKind::Quantity(v, dim))
            }
            Tok::Decimal(n, d) => {
                self.bump();
                self.mk(sp, ExprKind::Decimal(n, d))
            }
            // `Fixed[int16, 2](x)`: la conversion al tipo de punto fijo
            Tok::Ident(s) if (s == "Fixed" || s == "QFixed") && matches!(self.peek_at(1), Tok::LBracket) => {
                let ty = self.type_expr(false)?;
                self.expect(Tok::LParen, "'(' (Fixed[int16, 2](valor))")?;
                let arg = self.expr()?;
                let end = self.expect(Tok::RParen, "')'")?;
                self.mk(sp.to(end), ExprKind::FixedCast(Box::new(ty), Box::new(arg)))
            }
            Tok::LParen => {
                self.bump();
                let e = self.expr()?;
                if self.at(&Tok::Comma) {
                    let c = self.span();
                    return self.err("PYP1025", c, "las tuplas no estan soportadas");
                }
                self.expect(Tok::RParen, "')'")?;
                e
            }
            Tok::Str(text, _) => {
                self.bump();
                self.mk(sp, ExprKind::Str(text))
            }
            Tok::FStr(inner) => {
                self.bump();
                let parts = self.fstring_parts(&inner, sp);
                self.mk(sp, ExprKind::FStr(parts))
            }
            Tok::LBracket => {
                self.bump();
                let mut items = Vec::new();
                while !self.at(&Tok::RBracket) {
                    items.push(self.expr()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                let end = self.expect(Tok::RBracket, "']'")?;
                self.mk(sp.to(end), ExprKind::List(items))
            }
            Tok::Ident(s) => match s.as_str() {
                "True" | "False" => {
                    self.bump();
                    self.mk(sp, ExprKind::Bool(s == "True"))
                }
                "None" => return self.err("PYP1021", sp, "'None' solo puede usarse como tipo de retorno"),
                kw if UNSUPPORTED_KEYWORDS.contains(&kw) || kw == "not" => {
                    return self.err("PYP1020", sp, format!("'{kw}' no esta soportado en una expresion"));
                }
                _ => {
                    self.bump();
                    if self.at(&Tok::LBracket) {
                        self.bump();
                        let index = self.expr()?;
                        let end = self.expect(Tok::RBracket, "']'")?;
                        self.mk(sp.to(end), ExprKind::Index { obj: s, obj_span: sp, index: Box::new(index) })
                    } else if self.at(&Tok::LParen) {
                        let args = self.call_args()?;
                        let span = sp.to(self.prev_span());
                        self.mk(span, ExprKind::Call { func: s, func_span: sp, args })
                    } else if *self.peek() == Tok::Other('.') {
                        self.bump();
                        let attr = self.ident("un nombre despues del punto")?;
                        if self.at(&Tok::LParen) {
                            let (args, kwargs) = self.call_args_kw(true)?;
                            let span = sp.to(self.prev_span());
                            self.mk(span, ExprKind::MethodCall { obj: s, obj_span: sp, attr: None, method: attr.name, args, kwargs })
                        } else if *self.peek() == Tok::Other('.') {
                            // REG.BIT.metodo(...)
                            self.bump();
                            let method = self.ident("un metodo despues del bit")?;
                            if !self.at(&Tok::LParen) {
                                return self.err("PYP1026", sp.to(method.span), "el acceso a atributos (a.b.c) no esta soportado; un bit se usa con un metodo, p. ej. STATUS.RP0.set()");
                            }
                            let (args, kwargs) = self.call_args_kw(true)?;
                            let span = sp.to(self.prev_span());
                            self.mk(span, ExprKind::MethodCall { obj: s, obj_span: sp, attr: Some(attr), method: method.name, args, kwargs })
                        } else {
                            return self.err("PYP1026", sp.to(attr.span), "el acceso a atributos (a.b) no esta soportado; los pines usan metodos, p. ej. RD0.toggle()");
                        }
                    } else {
                        self.mk(sp, ExprKind::Name(s))
                    }
                }
            },
            _ => return self.unexpected("una expresion"),
        };
        match self.peek() {
            Tok::Other('.') => {
                let s = self.span();
                self.err("PYP1026", s, "el acceso a atributos (a.b) no esta soportado")
            }
            Tok::LBracket => {
                let s = self.span();
                self.err("PYP1026", s, "solo se puede indexar el nombre de una tabla (TABLA[i]) y una sola vez")
            }
            Tok::LParen => {
                let s = self.span();
                self.err("PYP1024", s, "solo se pueden llamar funciones por su nombre")
            }
            Tok::StarStar => {
                let s = self.span();
                self.err("PYP1022", s, "'**' no esta soportado")
            }
            _ => Ok(e),
        }
    }
}
