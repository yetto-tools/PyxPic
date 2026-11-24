//! Parser descendente recursivo del subconjunto PyPIC.
//!
//! Todo lo que Python permite pero PyPIC no (clases, listas, floats, lambdas,
//! `try`, etc.) se rechaza aqui con un diagnostico especifico.

use crate::ast::*;
use crate::diag::{Diagnostics, Span};
use crate::lexer::{lex, Tok, Token};

type R<T> = Result<T, ()>;

const UNSUPPORTED_KEYWORDS: &[&str] = &[
    "class", "try", "except", "finally", "with", "del", "assert", "raise", "yield", "async",
    "await", "match", "nonlocal", "lambda", "import", "from", "def", "else", "elif", "is", "in",
    "as",
];

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
        Tok::Int(_) => "un numero".into(),
        Tok::Str(..) => "una cadena".into(),
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
                return self.err("PYP1002", sp, "'import' no soportado todavia; solo 'from pypic import <tipos>'");
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
                return self.err(
                    "PYP1001",
                    sp,
                    "a nivel de modulo solo se permiten declaraciones tipadas (x: u8 = 0), funciones e imports de 'pypic'",
                );
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
        let (body, asm) = if decorators.iter().any(|d| d.name == "asm_function") {
            (Vec::new(), Some(self.asm_body()?))
        } else {
            (self.block(true)?, None)
        };
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
            Tok::Ident(s) if s == "Final" => {
                self.bump();
                self.expect(Tok::LBracket, "'[' (Final[tipo])")?;
                let inner = self.type_expr(false)?;
                let end = self.expect(Tok::RBracket, "']'")?;
                Ok(TypeExpr { span: sp.to(end), kind: TypeExprKind::Final(Box::new(inner)) })
            }
            Tok::Ident(s) if !UNSUPPORTED_KEYWORDS.contains(&s.as_str()) => {
                self.bump();
                Ok(TypeExpr { span: sp, kind: TypeExprKind::Name(s) })
            }
            _ => self.err("PYP2001", sp, "tipo no valido; usa u8, i8, u16, i16, u32, i32 o bool"),
        }
    }

    fn var_decl(&mut self) -> R<VarDecl> {
        let name = self.ident("un nombre")?;
        self.expect(Tok::Colon, "':'")?;
        let ann = self.type_expr(false)?;
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
                    return self.err("PYP1010", start, format!("'{kw}' no esta soportado en el subconjunto PyPIC"));
                }
                _ => {}
            }
            // sentencia simple que empieza por un nombre
            match self.peek_at(1).clone() {
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
                Tok::Slash => {
                    let sp = self.span();
                    return self.err("PYP1022", sp, "'/' no existe: usa '//' (division entera)");
                }
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
            Tok::Str(..) => return self.err("PYP1021", sp, "las cadenas solo se admiten como cuerpo de una funcion @asm_function"),
            Tok::LBracket => return self.err("PYP1025", sp, "las listas no estan soportadas"),
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
                    if self.at(&Tok::LParen) {
                        let args = self.call_args()?;
                        let span = sp.to(self.prev_span());
                        self.mk(span, ExprKind::Call { func: s, func_span: sp, args })
                    } else if *self.peek() == Tok::Other('.') {
                        self.bump();
                        let attr = self.ident("un nombre despues del punto")?;
                        if self.at(&Tok::LParen) {
                            let (args, kwargs) = self.call_args_kw(true)?;
                            let span = sp.to(self.prev_span());
                            self.mk(span, ExprKind::MethodCall { obj: s, obj_span: sp, method: attr.name, args, kwargs })
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
                self.err("PYP1026", s, "el indexado (a[i]) no esta soportado todavia")
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
