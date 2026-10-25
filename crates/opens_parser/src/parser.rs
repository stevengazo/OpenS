//! Parser descendente-recursivo de OpenS.
//!
//! Igual que el lexer, **nunca se detiene en el primer error**: emite un
//! diagnóstico, se sincroniza en el siguiente límite razonable (fin de ítem o
//! de sentencia) y sigue, para reportar todos los problemas de una vez. Esto es
//! clave cuando quien corrige es un agente de IA leyendo el JSON.

use crate::ast::*;
use opens_syntax::token::{Keyword, StrPart, Token, TokenKind};
use opens_syntax::{Diagnostic, Span};

/// Códigos de error de sintaxis (rango `E02xx`).
pub mod codes {
    pub const UNEXPECTED_TOKEN: &str = "E0201";
    pub const EXPECTED_ITEM: &str = "E0202";
    pub const EXPECTED_TYPE: &str = "E0203";
    pub const EXPECTED_EXPR: &str = "E0204";
    pub const UNCLOSED_DELIM: &str = "E0205";
    pub const EXPECTED_IDENT: &str = "E0206";
    pub const EXPECTED_NAME: &str = "E0207";

    pub fn explain(code: &str) -> Option<&'static str> {
        Some(match code {
            UNEXPECTED_TOKEN => "Apareció un token que no encaja en la gramática en ese lugar.",
            EXPECTED_ITEM => "A nivel superior solo pueden ir: import, fn, tool, type, enum, trait, impl, prompt, server, extern y test (con `pub`/`export` opcional).",
            EXPECTED_TYPE => "Se esperaba un tipo: `Int`, `List<T>`, `&T`, `*T`, `box T`, `fn(T) -> U`, `()`, o un nombre de tipo.",
            EXPECTED_EXPR => "Se esperaba una expresión (un valor, una llamada, un operador unario, etc.).",
            UNCLOSED_DELIM => "Falta cerrar un delimitador: `)`, `}` o `]`.",
            EXPECTED_IDENT => "Se esperaba un identificador.",
            EXPECTED_NAME => "Se esperaba el nombre de una declaración.",
            _ => return None,
        })
    }
}

pub struct ParseResult {
    pub module: Module,
    pub diagnostics: Vec<Diagnostic>,
}

/// Parsea una lista de tokens (la de `opens_syntax::lex`) en un [`Module`].
pub fn parse(tokens: Vec<Token>) -> ParseResult {
    let mut p = Parser::new(tokens);
    let items = p.parse_module();
    ParseResult {
        module: Module { items },
        diagnostics: p.diags,
    }
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    diags: Vec<Diagnostic>,
    /// Mientras es `true`, un `Nombre {` no se interpreta como literal de
    /// struct (p. ej. en la condición de un `if`/`while`/`for`).
    no_struct: bool,
}

impl Parser {
    fn new(mut toks: Vec<Token>) -> Self {
        // `lex` siempre deja un `Eof` al final; si no, lo añadimos.
        if !matches!(toks.last().map(|t| &t.kind), Some(TokenKind::Eof)) {
            let end = toks.last().map(|t| t.span.end).unwrap_or(0);
            toks.push(Token {
                kind: TokenKind::Eof,
                span: Span::new(end, end),
                newline_before: true,
            });
        }
        Parser {
            toks,
            pos: 0,
            diags: Vec::new(),
            no_struct: false,
        }
    }

    // ---------- cursor ----------

    fn peek(&self) -> &TokenKind {
        &self.toks[self.pos].kind
    }

    fn peek2(&self) -> Option<&TokenKind> {
        self.toks.get(self.pos + 1).map(|t| &t.kind)
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), TokenKind::Eof)
    }

    /// ¿El token actual viene tras un salto de línea? (terminador implícito).
    fn newline_before(&self) -> bool {
        self.toks[self.pos].newline_before
    }

    fn at(&self, kind: &TokenKind) -> bool {
        self.peek() == kind
    }

    fn at_kw(&self, kw: Keyword) -> bool {
        matches!(self.peek(), TokenKind::Keyword(k) if *k == kw)
    }

    /// Un comentario `///` previo (se adjunta como documentación al ítem).
    fn eat_doc(&mut self) -> Option<String> {
        let mut doc: Option<String> = None;
        while let TokenKind::DocComment(text) = self.peek() {
            let line = text.clone();
            match &mut doc {
                Some(d) => {
                    d.push('\n');
                    d.push_str(&line);
                }
                None => doc = Some(line),
            }
            self.pos += 1;
        }
        doc
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if !self.at_eof() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, kw: Keyword) -> bool {
        if self.at_kw(kw) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    // ---------- diagnósticos ----------

    fn error(&mut self, code: &'static str, msg: impl Into<String>, span: Span) {
        self.diags.push(Diagnostic::error(code, msg, span));
    }

    fn error_help(
        &mut self,
        code: &'static str,
        msg: impl Into<String>,
        span: Span,
        help: impl Into<String>,
    ) {
        self.diags
            .push(Diagnostic::error(code, msg, span).with_help(help));
    }

    /// Consume `kind` o reporta que faltaba, sin avanzar (el llamador decide
    /// cómo recuperarse).
    fn expect(&mut self, kind: TokenKind, code: &'static str) -> bool {
        if self.eat(&kind) {
            true
        } else {
            let want = kind.symbol().map(|s| format!("`{s}`")).unwrap_or_default();
            self.error(
                code,
                format!("se esperaba {} pero se encontró {}", want, self.peek().describe()),
                self.span(),
            );
            false
        }
    }

    fn expect_ident(&mut self) -> Option<(String, Span)> {
        match self.peek().clone() {
            TokenKind::Ident(name) => {
                let span = self.span();
                self.pos += 1;
                Some((name, span))
            }
            _ => {
                self.error(
                    codes::EXPECTED_IDENT,
                    format!("se esperaba un identificador pero se encontró {}", self.peek().describe()),
                    self.span(),
                );
                None
            }
        }
    }

    // ---------- nivel módulo ----------

    fn parse_module(&mut self) -> Vec<Item> {
        let mut items = Vec::new();
        while !self.at_eof() {
            let before = self.pos;
            if let Some(item) = self.parse_item() {
                items.push(item);
            }
            // Garantía de progreso: si no avanzamos, saltamos un token.
            if self.pos == before && !self.at_eof() {
                self.pos += 1;
            }
        }
        items
    }

    fn parse_item(&mut self) -> Option<Item> {
        let start = self.span();
        let doc = self.eat_doc();
        let is_pub = self.eat_kw(Keyword::Pub) || self.eat_kw(Keyword::Export);

        let kind = match self.peek() {
            TokenKind::Keyword(Keyword::Import) => self.parse_import(),
            TokenKind::Keyword(Keyword::Fn) => self.parse_function(FnFlavor::Fn).map(ItemKind::Function),
            TokenKind::Keyword(Keyword::Tool) => self.parse_function(FnFlavor::Tool).map(ItemKind::Function),
            TokenKind::Keyword(Keyword::Type) => self.parse_type_def(),
            TokenKind::Keyword(Keyword::Enum) => self.parse_enum_def(),
            TokenKind::Keyword(Keyword::Trait) => self.parse_trait(),
            TokenKind::Keyword(Keyword::Impl) => self.parse_impl(),
            TokenKind::Keyword(Keyword::Prompt) => self.parse_prompt(),
            TokenKind::Keyword(Keyword::Server) => self.parse_server(),
            TokenKind::Keyword(Keyword::Extern) => self.parse_extern(),
            TokenKind::Keyword(Keyword::Test) => self.parse_test(),
            _ => {
                self.error_help(
                    codes::EXPECTED_ITEM,
                    format!("se esperaba una declaración pero se encontró {}", self.peek().describe()),
                    self.span(),
                    "a nivel superior van: import, fn, tool, type, enum, trait, impl, prompt, server, extern, test",
                );
                self.sync_item();
                return None;
            }
        };

        let kind = kind?;
        let span = start.to(self.prev_span());
        Some(Item {
            doc,
            is_pub,
            kind,
            span,
        })
    }

    /// Salta hasta el comienzo del próximo ítem para recuperarse.
    fn sync_item(&mut self) {
        while !self.at_eof() {
            if matches!(
                self.peek(),
                TokenKind::Keyword(
                    Keyword::Import
                        | Keyword::Fn
                        | Keyword::Tool
                        | Keyword::Type
                        | Keyword::Enum
                        | Keyword::Trait
                        | Keyword::Impl
                        | Keyword::Prompt
                        | Keyword::Server
                        | Keyword::Extern
                        | Keyword::Test
                        | Keyword::Pub
                        | Keyword::Export
                ) | TokenKind::DocComment(_)
            ) {
                return;
            }
            self.pos += 1;
        }
    }

    fn parse_import(&mut self) -> Option<ItemKind> {
        self.bump(); // import
        let (first, _) = self.expect_ident()?;
        let mut path = vec![first];
        while self.eat(&TokenKind::Dot) {
            let (seg, _) = self.expect_ident()?;
            path.push(seg);
        }
        let alias = if self.at(&TokenKind::Ident("as".into())) {
            // `as` no es palabra clave; se reconoce como identificador.
            self.pos += 1;
            self.expect_ident().map(|(n, _)| n)
        } else {
            None
        };
        Some(ItemKind::Import(Import { path, alias }))
    }

    fn parse_type_def(&mut self) -> Option<ItemKind> {
        self.bump(); // type
        let (name, _) = self.expect_ident()?;
        let generics = self.parse_generic_params();
        self.expect(TokenKind::LBrace, codes::UNEXPECTED_TOKEN);
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            let before = self.pos;
            if let Some(f) = self.parse_field() {
                fields.push(f);
            }
            self.eat(&TokenKind::Comma);
            if self.pos == before {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RBrace, codes::UNCLOSED_DELIM);
        Some(ItemKind::TypeDef(TypeDef {
            name,
            generics,
            fields,
        }))
    }

    fn parse_field(&mut self) -> Option<Field> {
        let start = self.span();
        let doc = self.eat_doc();
        let attrs = self.parse_attrs();
        let (name, _) = self.expect_ident()?;
        self.expect(TokenKind::Colon, codes::UNEXPECTED_TOKEN);
        let ty = self.parse_type()?;
        // Anotaciones también permitidas tras el tipo: `edad: Int @min(0)`.
        let mut attrs = attrs;
        attrs.extend(self.parse_attrs());
        Some(Field {
            doc,
            attrs,
            name,
            ty,
            span: start.to(self.prev_span()),
        })
    }

    fn parse_enum_def(&mut self) -> Option<ItemKind> {
        self.bump(); // enum
        let (name, _) = self.expect_ident()?;
        let variants = self.parse_variant_list();
        Some(ItemKind::EnumDef(EnumDef { name, variants }))
    }

    fn parse_variant_list(&mut self) -> Vec<Variant> {
        let mut variants = Vec::new();
        self.expect(TokenKind::LBrace, codes::UNEXPECTED_TOKEN);
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            if let Some((name, span)) = self.expect_ident() {
                variants.push(Variant { name, span });
            } else {
                self.pos += 1;
            }
            self.eat(&TokenKind::Comma);
        }
        self.expect(TokenKind::RBrace, codes::UNCLOSED_DELIM);
        variants
    }

    fn parse_trait(&mut self) -> Option<ItemKind> {
        self.bump(); // trait
        let (name, _) = self.expect_ident()?;
        let generics = self.parse_generic_params();
        self.expect(TokenKind::LBrace, codes::UNEXPECTED_TOKEN);
        let mut methods = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            let before = self.pos;
            if self.at_kw(Keyword::Fn) || self.at_kw(Keyword::Tool) {
                let flavor = if self.at_kw(Keyword::Tool) {
                    FnFlavor::Tool
                } else {
                    FnFlavor::Fn
                };
                if let Some(m) = self.parse_function(flavor) {
                    methods.push(m);
                }
            } else {
                self.error(
                    codes::UNEXPECTED_TOKEN,
                    "dentro de un `trait` solo van firmas de método (`fn`)",
                    self.span(),
                );
            }
            if self.pos == before {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RBrace, codes::UNCLOSED_DELIM);
        Some(ItemKind::Trait(TraitDef {
            name,
            generics,
            methods,
        }))
    }

    fn parse_impl(&mut self) -> Option<ItemKind> {
        self.bump(); // impl
        let generics = self.parse_generic_params();
        let (first, _) = self.expect_ident()?;
        // `impl Trait for Tipo` o `impl Tipo`.
        let (trait_name, type_name) = if self.at(&TokenKind::Ident("for".into())) {
            self.pos += 1;
            let (ty, _) = self.expect_ident()?;
            (Some(first), ty)
        } else {
            (None, first)
        };
        self.expect(TokenKind::LBrace, codes::UNEXPECTED_TOKEN);
        let mut methods = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            let before = self.pos;
            let _doc = self.eat_doc();
            let _is_pub = self.eat_kw(Keyword::Pub);
            let flavor = if self.at_kw(Keyword::Tool) {
                FnFlavor::Tool
            } else {
                FnFlavor::Fn
            };
            if self.at_kw(Keyword::Fn) || self.at_kw(Keyword::Tool) {
                if let Some(m) = self.parse_function(flavor) {
                    methods.push(m);
                }
            } else {
                self.error(
                    codes::UNEXPECTED_TOKEN,
                    "dentro de un `impl` solo van métodos (`fn`)",
                    self.span(),
                );
            }
            if self.pos == before {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RBrace, codes::UNCLOSED_DELIM);
        Some(ItemKind::Impl(ImplBlock {
            trait_name,
            type_name,
            generics,
            methods,
        }))
    }

    fn parse_prompt(&mut self) -> Option<ItemKind> {
        self.bump(); // prompt
        let (name, _) = self.expect_ident()?;
        let params = self.parse_params();
        self.expect(TokenKind::LBrace, codes::UNEXPECTED_TOKEN);
        let template = self.parse_expr();
        self.expect(TokenKind::RBrace, codes::UNCLOSED_DELIM);
        Some(ItemKind::Prompt(Prompt {
            name,
            params,
            template,
        }))
    }

    fn parse_server(&mut self) -> Option<ItemKind> {
        self.bump(); // server
        let (name, _) = self.expect_ident()?;
        self.expect(TokenKind::LBrace, codes::UNEXPECTED_TOKEN);
        let mut exposes = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            if self.eat_kw(Keyword::Expose) {
                loop {
                    if let Some((n, _)) = self.expect_ident() {
                        exposes.push(n);
                    }
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
            } else {
                self.error(
                    codes::UNEXPECTED_TOKEN,
                    "dentro de un `server` solo va `expose nombre, ...`",
                    self.span(),
                );
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RBrace, codes::UNCLOSED_DELIM);
        Some(ItemKind::Server(ServerDef { name, exposes }))
    }

    fn parse_extern(&mut self) -> Option<ItemKind> {
        self.bump(); // extern
        let abi = match self.peek().clone() {
            TokenKind::Str(parts) => {
                self.pos += 1;
                flatten_str(&parts)
            }
            _ => {
                self.error(
                    codes::UNEXPECTED_TOKEN,
                    "tras `extern` va el ABI como string, p. ej. `extern \"c\"`",
                    self.span(),
                );
                String::new()
            }
        };
        let func = self.parse_function(FnFlavor::Fn)?;
        Some(ItemKind::Extern(ExternFn { abi, func }))
    }

    fn parse_test(&mut self) -> Option<ItemKind> {
        self.bump(); // test
        let name = match self.peek().clone() {
            TokenKind::Str(parts) => {
                self.pos += 1;
                flatten_str(&parts)
            }
            _ => {
                self.error(
                    codes::EXPECTED_NAME,
                    "un `test` lleva un nombre entre comillas: `test \"...\" { }`",
                    self.span(),
                );
                String::new()
            }
        };
        let body = self.parse_block();
        Some(ItemKind::Test(TestDef { name, body }))
    }

    // ---------- funciones ----------

    fn parse_function(&mut self, flavor: FnFlavor) -> Option<Function> {
        let start = self.span();
        self.bump(); // fn / tool
        let (name, _) = self.expect_ident()?;
        let generics = self.parse_generic_params();

        // Parámetros, con posible receptor `self` al inicio.
        self.expect(TokenKind::LParen, codes::UNEXPECTED_TOKEN);
        let self_kind = self.try_parse_self();
        let mut params = Vec::new();
        if self_kind.is_some() {
            self.eat(&TokenKind::Comma);
        }
        while !self.at(&TokenKind::RParen) && !self.at_eof() {
            let before = self.pos;
            if let Some(p) = self.parse_param() {
                params.push(p);
            }
            if !self.eat(&TokenKind::Comma) {
                break;
            }
            if self.pos == before {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RParen, codes::UNCLOSED_DELIM);

        let ret = if self.eat(&TokenKind::Arrow) {
            self.parse_type()
        } else {
            None
        };

        let mut uses = Vec::new();
        if self.eat_kw(Keyword::Uses) {
            loop {
                if let Some((n, _)) = self.expect_ident() {
                    uses.push(n);
                }
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }

        // Cuerpo opcional: las firmas de `trait` y `extern` terminan sin `{`.
        let body = if self.at(&TokenKind::LBrace) {
            Some(self.parse_block())
        } else {
            None
        };

        Some(Function {
            flavor,
            name,
            generics,
            self_kind,
            params,
            ret,
            uses,
            body,
            span: start.to(self.prev_span()),
        })
    }

    /// Reconoce `self`, `&self` o `&var self` al inicio de la lista de params.
    fn try_parse_self(&mut self) -> Option<SelfKind> {
        if self.at_kw(Keyword::SelfKw) {
            self.pos += 1;
            return Some(SelfKind::Value);
        }
        if self.at(&TokenKind::Amp) {
            // Miramos adelante sin consumir salvo que sea un `self`.
            let save = self.pos;
            self.pos += 1;
            let mutable = self.eat_kw(Keyword::Var);
            if self.at_kw(Keyword::SelfKw) {
                self.pos += 1;
                return Some(if mutable { SelfKind::RefVar } else { SelfKind::Ref });
            }
            self.pos = save; // no era un receptor; es un parámetro normal.
        }
        None
    }

    fn parse_param(&mut self) -> Option<Param> {
        let start = self.span();
        let attrs = self.parse_attrs();
        let (name, _) = self.expect_ident()?;
        self.expect(TokenKind::Colon, codes::UNEXPECTED_TOKEN);
        let ty = self.parse_type()?;
        let mut attrs = attrs;
        attrs.extend(self.parse_attrs());
        let default = if self.eat(&TokenKind::Eq) {
            Some(self.parse_expr())
        } else {
            None
        };
        Some(Param {
            attrs,
            name,
            ty,
            default,
            span: start.to(self.prev_span()),
        })
    }

    fn parse_params(&mut self) -> Vec<Param> {
        let mut params = Vec::new();
        if !self.expect(TokenKind::LParen, codes::UNEXPECTED_TOKEN) {
            return params;
        }
        while !self.at(&TokenKind::RParen) && !self.at_eof() {
            let before = self.pos;
            if let Some(p) = self.parse_param() {
                params.push(p);
            }
            if !self.eat(&TokenKind::Comma) {
                break;
            }
            if self.pos == before {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RParen, codes::UNCLOSED_DELIM);
        params
    }

    fn parse_generic_params(&mut self) -> Vec<String> {
        let mut generics = Vec::new();
        if self.eat(&TokenKind::Lt) {
            while !self.at(&TokenKind::Gt) && !self.at_eof() {
                if let Some((n, _)) = self.expect_ident() {
                    generics.push(n);
                }
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(TokenKind::Gt, codes::UNCLOSED_DELIM);
        }
        generics
    }

    fn parse_attrs(&mut self) -> Vec<Attr> {
        let mut attrs = Vec::new();
        while self.at(&TokenKind::At) {
            let start = self.span();
            self.pos += 1; // @
            let Some((name, _)) = self.expect_ident() else {
                break;
            };
            let mut args = Vec::new();
            if self.eat(&TokenKind::LParen) {
                while !self.at(&TokenKind::RParen) && !self.at_eof() {
                    args.push(self.parse_expr());
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RParen, codes::UNCLOSED_DELIM);
            }
            attrs.push(Attr {
                name,
                args,
                span: start.to(self.prev_span()),
            });
        }
        attrs
    }

    // ---------- tipos ----------

    fn parse_type(&mut self) -> Option<Type> {
        let start = self.span();
        match self.peek().clone() {
            TokenKind::Amp => {
                self.pos += 1;
                let mutable = self.eat_kw(Keyword::Var);
                let inner = Box::new(self.parse_type()?);
                Some(Type {
                    kind: TypeKind::Ref { mutable, inner },
                    span: start.to(self.prev_span()),
                })
            }
            TokenKind::Star => {
                self.pos += 1;
                let inner = Box::new(self.parse_type()?);
                Some(Type {
                    kind: TypeKind::Ptr { inner },
                    span: start.to(self.prev_span()),
                })
            }
            TokenKind::Keyword(Keyword::Box) => {
                self.pos += 1;
                let inner = Box::new(self.parse_type()?);
                Some(Type {
                    kind: TypeKind::Box { inner },
                    span: start.to(self.prev_span()),
                })
            }
            TokenKind::Keyword(Keyword::Enum) => {
                self.pos += 1;
                let variants = self
                    .parse_variant_list()
                    .into_iter()
                    .map(|v| v.name)
                    .collect();
                Some(Type {
                    kind: TypeKind::InlineEnum(variants),
                    span: start.to(self.prev_span()),
                })
            }
            TokenKind::Keyword(Keyword::Fn) => {
                self.pos += 1;
                self.expect(TokenKind::LParen, codes::UNEXPECTED_TOKEN);
                let mut params = Vec::new();
                while !self.at(&TokenKind::RParen) && !self.at_eof() {
                    if let Some(t) = self.parse_type() {
                        params.push(t);
                    }
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RParen, codes::UNCLOSED_DELIM);
                self.expect(TokenKind::Arrow, codes::UNEXPECTED_TOKEN);
                let ret = Box::new(self.parse_type()?);
                Some(Type {
                    kind: TypeKind::Func { params, ret },
                    span: start.to(self.prev_span()),
                })
            }
            TokenKind::LParen => {
                // `()` unidad.
                self.pos += 1;
                self.expect(TokenKind::RParen, codes::UNCLOSED_DELIM);
                Some(Type {
                    kind: TypeKind::Unit,
                    span: start.to(self.prev_span()),
                })
            }
            TokenKind::Ident(name) => {
                self.pos += 1;
                let mut args = Vec::new();
                if self.eat(&TokenKind::Lt) {
                    while !self.at(&TokenKind::Gt) && !self.at_eof() {
                        if let Some(t) = self.parse_type() {
                            args.push(t);
                        }
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(TokenKind::Gt, codes::UNCLOSED_DELIM);
                }
                Some(Type {
                    kind: TypeKind::Named { name, args },
                    span: start.to(self.prev_span()),
                })
            }
            _ => {
                self.error(
                    codes::EXPECTED_TYPE,
                    format!("se esperaba un tipo pero se encontró {}", self.peek().describe()),
                    self.span(),
                );
                None
            }
        }
    }

    // ---------- bloques y sentencias ----------

    fn parse_block(&mut self) -> Block {
        let start = self.span();
        self.expect(TokenKind::LBrace, codes::UNEXPECTED_TOKEN);
        let mut stmts = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            let before = self.pos;
            if let Some(s) = self.parse_stmt() {
                stmts.push(s);
            }
            self.eat(&TokenKind::Semi); // separador opcional
            if self.pos == before {
                // No se consumió nada: sincronizar para no colgarse.
                self.sync_stmt();
                if self.pos == before {
                    self.pos += 1;
                }
            }
        }
        self.expect(TokenKind::RBrace, codes::UNCLOSED_DELIM);
        Block {
            stmts,
            span: start.to(self.prev_span()),
        }
    }

    /// Sincroniza al próximo inicio de sentencia o al `}` de cierre.
    fn sync_stmt(&mut self) {
        while !self.at_eof() {
            if matches!(self.peek(), TokenKind::RBrace | TokenKind::Semi) {
                return;
            }
            if matches!(
                self.peek(),
                TokenKind::Keyword(
                    Keyword::Let
                        | Keyword::Var
                        | Keyword::Return
                        | Keyword::If
                        | Keyword::While
                        | Keyword::For
                        | Keyword::Match
                        | Keyword::Break
                        | Keyword::Continue
                        | Keyword::Unsafe
                        | Keyword::Assert
                )
            ) {
                return;
            }
            self.pos += 1;
        }
    }

    fn parse_stmt(&mut self) -> Option<Stmt> {
        let start = self.span();
        let kind = match self.peek() {
            TokenKind::Keyword(Keyword::Let) => self.parse_let(false),
            TokenKind::Keyword(Keyword::Var) => self.parse_let(true),
            TokenKind::Keyword(Keyword::Return) => {
                self.bump();
                let value = if self.starts_expr() {
                    Some(self.parse_expr())
                } else {
                    None
                };
                StmtKind::Return(value)
            }
            TokenKind::Keyword(Keyword::Break) => {
                self.bump();
                StmtKind::Break
            }
            TokenKind::Keyword(Keyword::Continue) => {
                self.bump();
                StmtKind::Continue
            }
            TokenKind::Keyword(Keyword::If) => StmtKind::If(self.parse_if()),
            TokenKind::Keyword(Keyword::While) => {
                self.bump();
                let cond = self.parse_cond();
                let body = self.parse_block();
                StmtKind::While { cond, body }
            }
            TokenKind::Keyword(Keyword::For) => self.parse_for(),
            TokenKind::Keyword(Keyword::Match) => StmtKind::Match(self.parse_match()),
            TokenKind::Keyword(Keyword::Unsafe) => {
                self.bump();
                StmtKind::Unsafe(self.parse_block())
            }
            TokenKind::Keyword(Keyword::Assert) => {
                self.bump();
                StmtKind::Assert(self.parse_expr())
            }
            _ => {
                // Expresión suelta o asignación.
                let expr = self.parse_expr();
                if let Some(op) = self.assign_op() {
                    let value = self.parse_expr();
                    StmtKind::Assign {
                        target: expr,
                        op,
                        value,
                    }
                } else {
                    StmtKind::Expr(expr)
                }
            }
        };
        Some(Stmt {
            kind,
            span: start.to(self.prev_span()),
        })
    }

    fn parse_let(&mut self, mutable: bool) -> StmtKind {
        self.bump(); // let / var
        let name = self.expect_ident().map(|(n, _)| n).unwrap_or_default();
        let ty = if self.eat(&TokenKind::Colon) {
            self.parse_type()
        } else {
            None
        };
        self.expect(TokenKind::Eq, codes::UNEXPECTED_TOKEN);
        let value = self.parse_expr();
        StmtKind::Let {
            mutable,
            name,
            ty,
            value,
        }
    }

    fn parse_if(&mut self) -> IfStmt {
        self.bump(); // if
        let cond = self.parse_cond();
        let then = self.parse_block();
        let els = if self.eat_kw(Keyword::Else) {
            if self.at_kw(Keyword::If) {
                Some(Else::If(Box::new(self.parse_if())))
            } else {
                Some(Else::Block(self.parse_block()))
            }
        } else {
            None
        };
        IfStmt { cond, then, els }
    }

    fn parse_for(&mut self) -> StmtKind {
        self.bump(); // for
        let is_await = self.eat_kw(Keyword::Await);
        let binder = self.expect_ident().map(|(n, _)| n).unwrap_or_default();
        self.eat_kw(Keyword::In);
        let iter = self.parse_cond();
        let body = self.parse_block();
        StmtKind::For {
            is_await,
            binder,
            iter,
            body,
        }
    }

    /// Expresión de condición: se prohíben los literales de struct para que
    /// `if x { }` no confunda la `{` con el inicio de un `Nombre { ... }`.
    fn parse_cond(&mut self) -> Expr {
        let saved = self.no_struct;
        self.no_struct = true;
        let e = self.parse_expr();
        self.no_struct = saved;
        e
    }

    fn assign_op(&mut self) -> Option<AssignOp> {
        let op = match self.peek() {
            TokenKind::Eq => AssignOp::Set,
            TokenKind::PlusEq => AssignOp::Add,
            TokenKind::MinusEq => AssignOp::Sub,
            TokenKind::StarEq => AssignOp::Mul,
            TokenKind::SlashEq => AssignOp::Div,
            _ => return None,
        };
        self.pos += 1;
        Some(op)
    }

    // ---------- expresiones ----------

    fn starts_expr(&self) -> bool {
        matches!(
            self.peek(),
            TokenKind::Int(_)
                | TokenKind::Float(_)
                | TokenKind::Str(_)
                | TokenKind::Ident(_)
                | TokenKind::LParen
                | TokenKind::LBracket
                | TokenKind::LBrace
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Amp
                | TokenKind::Dot
                | TokenKind::At
        ) || matches!(
            self.peek(),
            TokenKind::Keyword(
                Keyword::True
                    | Keyword::False
                    | Keyword::None
                    | Keyword::Not
                    | Keyword::Await
                    | Keyword::Yield
                    | Keyword::Box
                    | Keyword::Fn
                    | Keyword::Match
                    | Keyword::SelfKw
            )
        )
    }

    fn parse_expr(&mut self) -> Expr {
        self.parse_pipe()
    }

    /// `|>` — la más baja de las binarias.
    fn parse_pipe(&mut self) -> Expr {
        let mut lhs = self.parse_or();
        while self.at(&TokenKind::Pipe) {
            self.pos += 1;
            let rhs = self.parse_or();
            let span = lhs.span.to(rhs.span);
            lhs = Expr {
                kind: ExprKind::Pipe {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            };
        }
        lhs
    }

    fn parse_or(&mut self) -> Expr {
        let mut lhs = self.parse_and();
        while self.at_kw(Keyword::Or) {
            self.pos += 1;
            let rhs = self.parse_and();
            lhs = bin(BinOp::Or, lhs, rhs);
        }
        lhs
    }

    fn parse_and(&mut self) -> Expr {
        let mut lhs = self.parse_cmp();
        while self.at_kw(Keyword::And) {
            self.pos += 1;
            let rhs = self.parse_cmp();
            lhs = bin(BinOp::And, lhs, rhs);
        }
        lhs
    }

    fn parse_cmp(&mut self) -> Expr {
        let mut lhs = self.parse_range();
        loop {
            let op = match self.peek() {
                TokenKind::EqEq => BinOp::Eq,
                TokenKind::NotEq => BinOp::Ne,
                TokenKind::Lt => BinOp::Lt,
                TokenKind::Le => BinOp::Le,
                TokenKind::Gt => BinOp::Gt,
                TokenKind::Ge => BinOp::Ge,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_range();
            lhs = bin(op, lhs, rhs);
        }
        lhs
    }

    fn parse_range(&mut self) -> Expr {
        let lhs = self.parse_add();
        if self.at(&TokenKind::DotDot) {
            self.pos += 1;
            let rhs = self.parse_add();
            let span = lhs.span.to(rhs.span);
            return Expr {
                kind: ExprKind::Range {
                    start: Box::new(lhs),
                    end: Box::new(rhs),
                },
                span,
            };
        }
        lhs
    }

    fn parse_add(&mut self) -> Expr {
        let mut lhs = self.parse_mul();
        loop {
            let op = match self.peek() {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_mul();
            lhs = bin(op, lhs, rhs);
        }
        lhs
    }

    fn parse_mul(&mut self) -> Expr {
        let mut lhs = self.parse_unary();
        loop {
            let op = match self.peek() {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Rem,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_unary();
            lhs = bin(op, lhs, rhs);
        }
        lhs
    }

    fn parse_unary(&mut self) -> Expr {
        let start = self.span();
        if self.at_kw(Keyword::Not) {
            self.pos += 1;
            let e = self.parse_unary();
            return unary(UnOp::Not, e, start);
        }
        if self.at(&TokenKind::Minus) {
            self.pos += 1;
            let e = self.parse_unary();
            return unary(UnOp::Neg, e, start);
        }
        if self.at(&TokenKind::Amp) {
            self.pos += 1;
            let mutable = self.eat_kw(Keyword::Var);
            let e = self.parse_unary();
            let span = start.to(e.span);
            return Expr {
                kind: ExprKind::Ref {
                    mutable,
                    expr: Box::new(e),
                },
                span,
            };
        }
        if self.at(&TokenKind::Star) {
            self.pos += 1;
            let e = self.parse_unary();
            let span = start.to(e.span);
            return Expr {
                kind: ExprKind::Deref(Box::new(e)),
                span,
            };
        }
        if self.at_kw(Keyword::Box) {
            self.pos += 1;
            let e = self.parse_unary();
            let span = start.to(e.span);
            return Expr {
                kind: ExprKind::BoxExpr(Box::new(e)),
                span,
            };
        }
        if self.at_kw(Keyword::Await) {
            self.pos += 1;
            let e = self.parse_unary();
            let span = start.to(e.span);
            return Expr {
                kind: ExprKind::Await(Box::new(e)),
                span,
            };
        }
        if self.at_kw(Keyword::Yield) {
            self.pos += 1;
            let e = self.parse_unary();
            let span = start.to(e.span);
            return Expr {
                kind: ExprKind::Yield(Box::new(e)),
                span,
            };
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Expr {
        let mut e = self.parse_primary();
        loop {
            // Un encadenamiento (`.`, `(`, `[`, `?`) no cruza un salto de
            // línea: ahí termina la expresión y empieza otra sentencia/rama.
            if self.newline_before() {
                break;
            }
            match self.peek() {
                TokenKind::Dot => {
                    self.pos += 1;
                    // `.variante` como forma corta solo si `e` ya fue un error
                    // no aplica; aquí es acceso a campo o método.
                    let Some((name, _)) = self.expect_ident() else {
                        break;
                    };
                    if self.at(&TokenKind::LParen) || self.at(&TokenKind::Lt) {
                        let generics = self.try_type_args_before_call();
                        let args = self.parse_args();
                        let span = e.span.to(self.prev_span());
                        e = Expr {
                            kind: ExprKind::MethodCall {
                                recv: Box::new(e),
                                method: name,
                                generics,
                                args,
                            },
                            span,
                        };
                    } else {
                        let span = e.span.to(self.prev_span());
                        e = Expr {
                            kind: ExprKind::Field {
                                recv: Box::new(e),
                                name,
                            },
                            span,
                        };
                    }
                }
                TokenKind::LParen => {
                    let args = self.parse_args();
                    let span = e.span.to(self.prev_span());
                    e = Expr {
                        kind: ExprKind::Call {
                            callee: Box::new(e),
                            generics: Vec::new(),
                            args,
                        },
                        span,
                    };
                }
                TokenKind::Lt => {
                    // Posible llamada genérica `f<T>(...)`.
                    let generics = self.try_type_args_before_call();
                    if generics.is_empty() {
                        break;
                    }
                    let args = self.parse_args();
                    let span = e.span.to(self.prev_span());
                    e = Expr {
                        kind: ExprKind::Call {
                            callee: Box::new(e),
                            generics,
                            args,
                        },
                        span,
                    };
                }
                TokenKind::LBracket => {
                    self.pos += 1;
                    let index = self.parse_expr();
                    self.expect(TokenKind::RBracket, codes::UNCLOSED_DELIM);
                    let span = e.span.to(self.prev_span());
                    e = Expr {
                        kind: ExprKind::Index {
                            recv: Box::new(e),
                            index: Box::new(index),
                        },
                        span,
                    };
                }
                TokenKind::Question => {
                    self.pos += 1;
                    let span = e.span.to(self.prev_span());
                    e = Expr {
                        kind: ExprKind::Try(Box::new(e)),
                        span,
                    };
                }
                _ => break,
            }
        }
        e
    }

    /// Intenta `< Type, ... >` seguido de `(`. Si no encaja, no consume nada.
    fn try_type_args_before_call(&mut self) -> Vec<Type> {
        if !self.at(&TokenKind::Lt) {
            return Vec::new();
        }
        let save_pos = self.pos;
        let save_diags = self.diags.len();
        self.pos += 1;
        let mut args = Vec::new();
        let mut ok = true;
        while !self.at(&TokenKind::Gt) && !self.at_eof() {
            match self.parse_type() {
                Some(t) => args.push(t),
                None => {
                    ok = false;
                    break;
                }
            }
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        if ok && self.eat(&TokenKind::Gt) && self.at(&TokenKind::LParen) {
            args
        } else {
            // No era una lista de genéricos: retroceder y tratar `<` como comparación.
            self.pos = save_pos;
            self.diags.truncate(save_diags);
            Vec::new()
        }
    }

    fn parse_args(&mut self) -> Vec<Arg> {
        let mut args = Vec::new();
        if !self.expect(TokenKind::LParen, codes::UNEXPECTED_TOKEN) {
            return args;
        }
        while !self.at(&TokenKind::RParen) && !self.at_eof() {
            let start = self.span();
            // Argumento con nombre: `ident:` (pero no `ident::` ni `==`).
            let name = if matches!(self.peek(), TokenKind::Ident(_))
                && matches!(self.peek2(), TokenKind::Colon)
            {
                let n = match self.peek().clone() {
                    TokenKind::Ident(n) => n,
                    _ => unreachable!(),
                };
                self.pos += 2; // ident :
                Some(n)
            } else {
                None
            };
            let value = self.parse_expr();
            args.push(Arg {
                name,
                span: start.to(self.prev_span()),
                value,
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen, codes::UNCLOSED_DELIM);
        args
    }

    fn parse_primary(&mut self) -> Expr {
        let start = self.span();
        match self.peek().clone() {
            TokenKind::Int(n) => {
                self.pos += 1;
                lit(ExprKind::Int(n), start)
            }
            TokenKind::Float(n) => {
                self.pos += 1;
                lit(ExprKind::Float(n), start)
            }
            TokenKind::Keyword(Keyword::True) => {
                self.pos += 1;
                lit(ExprKind::Bool(true), start)
            }
            TokenKind::Keyword(Keyword::False) => {
                self.pos += 1;
                lit(ExprKind::Bool(false), start)
            }
            TokenKind::Keyword(Keyword::None) => {
                self.pos += 1;
                lit(ExprKind::NoneLit, start)
            }
            TokenKind::Keyword(Keyword::SelfKw) => {
                self.pos += 1;
                lit(ExprKind::SelfExpr, start)
            }
            TokenKind::Keyword(Keyword::Match) => {
                let m = self.parse_match();
                Expr {
                    span: m.span,
                    kind: ExprKind::Match(Box::new(m)),
                }
            }
            TokenKind::Keyword(Keyword::Fn) => self.parse_closure(),
            TokenKind::Str(parts) => {
                self.pos += 1;
                let segs = self.lower_string(&parts);
                lit(ExprKind::Str(segs), start.to(self.prev_span()))
            }
            TokenKind::Dot => {
                // `.variante`
                self.pos += 1;
                let name = self.expect_ident().map(|(n, _)| n).unwrap_or_default();
                lit(ExprKind::EnumShorthand(name), start.to(self.prev_span()))
            }
            TokenKind::Ident(name) => {
                self.pos += 1;
                // Literal de struct `Nombre { ... }` (salvo en condiciones).
                if self.at(&TokenKind::LBrace) && !self.no_struct {
                    return self.parse_struct_lit(name, start);
                }
                lit(ExprKind::Ident(name), start)
            }
            TokenKind::LParen => {
                self.pos += 1;
                if self.eat(&TokenKind::RParen) {
                    return lit(ExprKind::Unit, start.to(self.prev_span()));
                }
                let saved = self.no_struct;
                self.no_struct = false;
                let first = self.parse_expr();
                // ¿Tupla?
                if self.at(&TokenKind::Comma) {
                    let mut items = vec![first];
                    while self.eat(&TokenKind::Comma) {
                        if self.at(&TokenKind::RParen) {
                            break;
                        }
                        items.push(self.parse_expr());
                    }
                    self.no_struct = saved;
                    self.expect(TokenKind::RParen, codes::UNCLOSED_DELIM);
                    return Expr {
                        span: start.to(self.prev_span()),
                        kind: ExprKind::Tuple(items),
                    };
                }
                self.no_struct = saved;
                self.expect(TokenKind::RParen, codes::UNCLOSED_DELIM);
                // Paréntesis de agrupación: devolvemos la expresión interna.
                first
            }
            TokenKind::LBracket => {
                self.pos += 1;
                let saved = self.no_struct;
                self.no_struct = false;
                let mut items = Vec::new();
                while !self.at(&TokenKind::RBracket) && !self.at_eof() {
                    items.push(self.parse_expr());
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.no_struct = saved;
                self.expect(TokenKind::RBracket, codes::UNCLOSED_DELIM);
                Expr {
                    span: start.to(self.prev_span()),
                    kind: ExprKind::List(items),
                }
            }
            TokenKind::LBrace => self.parse_map(start),
            _ => {
                self.error(
                    codes::EXPECTED_EXPR,
                    format!("se esperaba una expresión pero se encontró {}", self.peek().describe()),
                    self.span(),
                );
                lit(ExprKind::Error, start)
            }
        }
    }

    fn parse_struct_lit(&mut self, name: String, start: Span) -> Expr {
        self.pos += 1; // {
        let saved = self.no_struct;
        self.no_struct = false;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            let Some((field, _)) = self.expect_ident() else {
                break;
            };
            // `campo: valor`, o taquigrafía `campo` (valor = variable homónima).
            let value = if self.eat(&TokenKind::Colon) {
                self.parse_expr()
            } else {
                lit(ExprKind::Ident(field.clone()), self.prev_span())
            };
            fields.push((field, value));
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.no_struct = saved;
        self.expect(TokenKind::RBrace, codes::UNCLOSED_DELIM);
        Expr {
            span: start.to(self.prev_span()),
            kind: ExprKind::StructLit { name, fields },
        }
    }

    fn parse_map(&mut self, start: Span) -> Expr {
        self.pos += 1; // {
        let saved = self.no_struct;
        self.no_struct = false;
        let mut entries = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            let key = self.parse_expr();
            self.expect(TokenKind::Colon, codes::UNEXPECTED_TOKEN);
            let value = self.parse_expr();
            entries.push((key, value));
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.no_struct = saved;
        self.expect(TokenKind::RBrace, codes::UNCLOSED_DELIM);
        Expr {
            span: start.to(self.prev_span()),
            kind: ExprKind::Map(entries),
        }
    }

    fn parse_closure(&mut self) -> Expr {
        let start = self.span();
        self.pos += 1; // fn
        self.expect(TokenKind::LParen, codes::UNEXPECTED_TOKEN);
        let mut params = Vec::new();
        while !self.at(&TokenKind::RParen) && !self.at_eof() {
            let p_start = self.span();
            let Some((name, _)) = self.expect_ident() else {
                break;
            };
            let ty = if self.eat(&TokenKind::Colon) {
                self.parse_type()
            } else {
                None
            };
            params.push(ClosureParam {
                name,
                ty,
                span: p_start.to(self.prev_span()),
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen, codes::UNCLOSED_DELIM);
        let body = if self.eat(&TokenKind::FatArrow) {
            ClosureBody::Expr(self.parse_expr())
        } else {
            ClosureBody::Block(self.parse_block())
        };
        Expr {
            span: start.to(self.prev_span()),
            kind: ExprKind::Closure {
                params,
                body: Box::new(body),
            },
        }
    }

    fn parse_match(&mut self) -> MatchExpr {
        let start = self.span();
        self.bump(); // match
        let scrutinee = Box::new(self.parse_cond());
        self.expect(TokenKind::LBrace, codes::UNEXPECTED_TOKEN);
        let mut arms = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            let before = self.pos;
            let arm_start = self.span();
            let pat = self.parse_pattern();
            self.expect(TokenKind::FatArrow, codes::UNEXPECTED_TOKEN);
            let body = self.parse_expr();
            arms.push(MatchArm {
                pat,
                span: arm_start.to(self.prev_span()),
                body,
            });
            self.eat(&TokenKind::Comma);
            if self.pos == before {
                self.pos += 1;
            }
        }
        self.expect(TokenKind::RBrace, codes::UNCLOSED_DELIM);
        MatchExpr {
            scrutinee,
            arms,
            span: start.to(self.prev_span()),
        }
    }

    fn parse_pattern(&mut self) -> Pattern {
        match self.peek().clone() {
            TokenKind::Dot => {
                self.pos += 1;
                let name = self.expect_ident().map(|(n, _)| n).unwrap_or_default();
                Pattern::EnumVariant(name)
            }
            TokenKind::Ident(name) if name == "_" => {
                self.pos += 1;
                Pattern::Wildcard
            }
            TokenKind::Ident(name) => {
                self.pos += 1;
                Pattern::Binding(name)
            }
            TokenKind::Int(_)
            | TokenKind::Float(_)
            | TokenKind::Str(_)
            | TokenKind::Keyword(Keyword::True)
            | TokenKind::Keyword(Keyword::False)
            | TokenKind::Keyword(Keyword::None) => Pattern::Literal(self.parse_primary()),
            _ => {
                self.error(
                    codes::UNEXPECTED_TOKEN,
                    format!("patrón inválido: {}", self.peek().describe()),
                    self.span(),
                );
                Pattern::Wildcard
            }
        }
    }

    /// Convierte el string del lexer (texto + interpolaciones de tokens) en
    /// segmentos con expresiones ya parseadas.
    fn lower_string(&mut self, parts: &[StrPart]) -> Vec<StrSeg> {
        let mut segs = Vec::new();
        for p in parts {
            match p {
                StrPart::Text(t) => segs.push(StrSeg::Text(t.clone())),
                StrPart::Interp(tokens) => {
                    let e = self.parse_interp(tokens);
                    segs.push(StrSeg::Interp(Box::new(e)));
                }
            }
        }
        segs
    }

    fn parse_interp(&mut self, tokens: &[Token]) -> Expr {
        let mut sub = Parser::new(tokens.to_vec());
        let e = sub.parse_expr();
        self.diags.append(&mut sub.diags);
        e
    }
}

// ---------- helpers de construcción ----------

fn lit(kind: ExprKind, span: Span) -> Expr {
    Expr { kind, span }
}

fn bin(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    let span = lhs.span.to(rhs.span);
    Expr {
        kind: ExprKind::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
        span,
    }
}

fn unary(op: UnOp, e: Expr, start: Span) -> Expr {
    let span = start.to(e.span);
    Expr {
        kind: ExprKind::Unary {
            op,
            expr: Box::new(e),
        },
        span,
    }
}

/// Aplana un string del lexer a texto plano, ignorando interpolaciones
/// (sirve para el ABI de `extern` y el nombre de `test`).
fn flatten_str(parts: &[StrPart]) -> String {
    let mut s = String::new();
    for p in parts {
        if let StrPart::Text(t) = p {
            s.push_str(t);
        }
    }
    s
}
