//! Chequeo de tipos y de capacidades de OpenS (hito 0.3).
//!
//! Es **conservador a propósito**: cuando no puede determinar un tipo usa
//! [`Ty::Unknown`], que es compatible con todo. Así solo reporta lo que sabe
//! con certeza y evita abrumar con falsos positivos (clave si quien corrige es
//! un LLM). Cubre: tipos desconocidos en firmas, desajustes de tipo locales,
//! `?` sobre algo que no es `Result`/`Option`, aridad de llamadas a funciones
//! conocidas, y la regla de capacidades (`uses`).

use crate::ty::Ty;
use opens_parser::ast::*;
use opens_syntax::{Diagnostic, Span};
use std::collections::{HashMap, HashSet};

/// Códigos de error de tipos (`E03xx`) y capacidades (`E04xx`).
pub mod codes {
    pub const UNKNOWN_TYPE: &str = "E0301";
    pub const TYPE_MISMATCH: &str = "E0302";
    pub const NOT_RESULT_OR_OPTION: &str = "E0303";
    pub const WRONG_ARITY: &str = "E0304";
    pub const MISSING_CAPABILITY: &str = "E0401";

    pub fn explain(code: &str) -> Option<&'static str> {
        Some(match code {
            UNKNOWN_TYPE => "Se usó un tipo que no existe: no es un tipo base (`Int`, `String`, `List`, `Result`, `Error`…), ni un `type`/`enum`/`trait` definido, ni un parámetro genérico en alcance.",
            TYPE_MISMATCH => "Dos tipos que debían coincidir no coinciden. OpenS no hace conversiones implícitas: convierte explícitamente (p. ej. `x.to_float()`).",
            NOT_RESULT_OR_OPTION => "El operador `?` solo se aplica a `Result<T, E>` o `Option<T>`.",
            WRONG_ARITY => "La llamada tiene un número de argumentos distinto al que declara la función.",
            MISSING_CAPABILITY => "Una función solo puede llamar a otra si declara al menos sus mismas capacidades (`uses`). Falta declarar alguna.",
            _ => return None,
        })
    }
}

/// Firma de una función de nivel superior (para chequear llamadas por nombre).
struct FnSig {
    params: Vec<Ty>,
    required: usize,
    total: usize,
    ret: Ty,
    uses: Vec<String>,
}

pub struct Checker {
    /// Nombres de `type`/`enum`/`trait` del usuario.
    user_types: HashSet<String>,
    /// Funciones de nivel superior por nombre (`fn`/`tool`).
    fns: HashMap<String, FnSig>,
    diags: Vec<Diagnostic>,

    // --- estado del cuerpo que se está chequeando ---
    scopes: Vec<HashMap<String, Ty>>,
    /// Capacidades declaradas por la función actual; `None` = sin restricción
    /// (p. ej. dentro de `test`).
    cur_uses: Option<HashSet<String>>,
    cur_ret: Ty,
}

/// Chequea un módulo ya parseado y devuelve los diagnósticos encontrados.
pub fn check_module(module: &Module) -> Vec<Diagnostic> {
    let mut c = Checker {
        user_types: HashSet::new(),
        fns: HashMap::new(),
        diags: Vec::new(),
        scopes: Vec::new(),
        cur_uses: None,
        cur_ret: Ty::Unit,
    };
    c.collect(module);
    c.check(module);
    c.diags
}

impl Checker {
    // ---------- paso 1: recolectar declaraciones ----------

    fn collect(&mut self, module: &Module) {
        for item in &module.items {
            match &item.kind {
                ItemKind::TypeDef(t) => {
                    self.user_types.insert(t.name.clone());
                }
                ItemKind::EnumDef(e) => {
                    self.user_types.insert(e.name.clone());
                }
                ItemKind::Trait(t) => {
                    self.user_types.insert(t.name.clone());
                }
                _ => {}
            }
        }
        // Las firmas de función se recolectan en un segundo barrido, cuando ya
        // conocemos todos los tipos (para resolver sus parámetros y retorno).
        for item in &module.items {
            if let ItemKind::Function(f) = &item.kind {
                let generics: HashSet<String> = f.generics.iter().cloned().collect();
                let params: Vec<Ty> = f
                    .params
                    .iter()
                    .map(|p| self.resolve_silent(&p.ty, &generics))
                    .collect();
                let required = f.params.iter().filter(|p| p.default.is_none()).count();
                let total = f.params.len();
                let ret = match &f.ret {
                    Some(t) => self.resolve_silent(t, &generics),
                    None => Ty::Unit,
                };
                self.fns.insert(
                    f.name.clone(),
                    FnSig {
                        params,
                        required,
                        total,
                        ret,
                        uses: f.uses.clone(),
                    },
                );
            }
        }
    }

    // ---------- paso 2: chequear ----------

    fn check(&mut self, module: &Module) {
        for item in &module.items {
            match &item.kind {
                ItemKind::Function(f) => {
                    let generics: HashSet<String> = f.generics.iter().cloned().collect();
                    self.check_function(f, &generics, None);
                }
                ItemKind::TypeDef(t) => {
                    let generics: HashSet<String> = t.generics.iter().cloned().collect();
                    for field in &t.fields {
                        self.resolve(&field.ty, &generics);
                    }
                }
                ItemKind::Trait(t) => {
                    let base: HashSet<String> = t.generics.iter().cloned().collect();
                    for m in &t.methods {
                        let mut generics = base.clone();
                        generics.extend(m.generics.iter().cloned());
                        self.check_function(m, &generics, None);
                    }
                }
                ItemKind::Impl(i) => {
                    if !self.is_known_type(&i.type_name) {
                        self.error(
                            codes::UNKNOWN_TYPE,
                            format!("`impl` sobre un tipo desconocido `{}`", i.type_name),
                            item.span,
                        );
                    }
                    if let Some(tr) = &i.trait_name {
                        if !self.user_types.contains(tr) {
                            self.error(
                                codes::UNKNOWN_TYPE,
                                format!("`trait` desconocido `{tr}`"),
                                item.span,
                            );
                        }
                    }
                    let base: HashSet<String> = i.generics.iter().cloned().collect();
                    let self_ty = Ty::Named(i.type_name.clone(), Vec::new());
                    for m in &i.methods {
                        let mut generics = base.clone();
                        generics.extend(m.generics.iter().cloned());
                        self.check_function(m, &generics, Some(self_ty.clone()));
                    }
                }
                ItemKind::Prompt(p) => {
                    let generics = HashSet::new();
                    for param in &p.params {
                        self.resolve(&param.ty, &generics);
                    }
                    // La plantilla es una expresión (normalmente un string).
                    self.cur_uses = None;
                    self.cur_ret = Ty::Unknown;
                    self.scopes.push(HashMap::new());
                    for param in &p.params {
                        let ty = self.resolve_silent(&param.ty, &generics);
                        self.bind(&param.name, ty);
                    }
                    self.infer(&p.template);
                    self.scopes.pop();
                }
                ItemKind::Extern(e) => {
                    let generics: HashSet<String> = e.func.generics.iter().cloned().collect();
                    for p in &e.func.params {
                        self.resolve(&p.ty, &generics);
                    }
                    if let Some(t) = &e.func.ret {
                        self.resolve(t, &generics);
                    }
                }
                ItemKind::Test(t) => {
                    self.cur_uses = None; // los tests no declaran capacidades
                    self.cur_ret = Ty::Unit;
                    self.check_block(&t.body);
                }
                ItemKind::EnumDef(_) | ItemKind::Import(_) | ItemKind::Server(_) => {}
            }
        }
    }

    fn check_function(&mut self, f: &Function, generics: &HashSet<String>, self_ty: Option<Ty>) {
        // Resolver (y validar) los tipos de la firma.
        for p in &f.params {
            self.resolve(&p.ty, generics);
        }
        if let Some(t) = &f.ret {
            self.resolve(t, generics);
        }

        self.cur_uses = Some(f.uses.iter().cloned().collect());
        self.cur_ret = match &f.ret {
            Some(t) => self.resolve_silent(t, generics),
            None => Ty::Unit,
        };

        self.scopes.push(HashMap::new());
        if let Some(st) = self_ty {
            self.bind("self", st);
        }
        for p in &f.params {
            let ty = self.resolve_silent(&p.ty, generics);
            self.bind(&p.name, ty);
        }
        if let Some(body) = &f.body {
            self.check_block(body);
        }
        self.scopes.pop();
    }

    // ---------- resolución de tipos ----------

    /// Resuelve un `Type` a `Ty`, reportando `E0301` si un nombre no existe.
    fn resolve(&mut self, t: &Type, generics: &HashSet<String>) -> Ty {
        match &t.kind {
            TypeKind::Unit => Ty::Unit,
            TypeKind::Ref { mutable, inner } => Ty::Ref {
                mutable: *mutable,
                inner: Box::new(self.resolve(inner, generics)),
            },
            TypeKind::Ptr { inner } => Ty::Ptr(Box::new(self.resolve(inner, generics))),
            TypeKind::Box { inner } => Ty::Boxed(Box::new(self.resolve(inner, generics))),
            TypeKind::Func { params, ret } => Ty::Func(
                params.iter().map(|p| self.resolve(p, generics)).collect(),
                Box::new(self.resolve(ret, generics)),
            ),
            TypeKind::InlineEnum(_) => Ty::Named("<enum>".to_string(), Vec::new()),
            TypeKind::Named { name, args } => {
                let args_ty: Vec<Ty> = args.iter().map(|a| self.resolve(a, generics)).collect();
                if let Some(scalar) = scalar_ty(name) {
                    return scalar;
                }
                let known = is_builtin_generic(name)
                    || self.user_types.contains(name)
                    || generics.contains(name);
                if known {
                    Ty::Named(name.clone(), args_ty)
                } else {
                    self.error(
                        codes::UNKNOWN_TYPE,
                        format!("tipo desconocido `{name}`"),
                        t.span,
                    );
                    Ty::Unknown
                }
            }
        }
    }

    /// Igual que [`resolve`] pero sin emitir diagnósticos (para recolectar
    /// firmas antes del chequeo, evitando reportar lo mismo dos veces).
    fn resolve_silent(&mut self, t: &Type, generics: &HashSet<String>) -> Ty {
        let before = self.diags.len();
        let ty = self.resolve(t, generics);
        self.diags.truncate(before);
        ty
    }

    fn is_known_type(&self, name: &str) -> bool {
        scalar_ty(name).is_some() || is_builtin_generic(name) || self.user_types.contains(name)
    }

    // ---------- sentencias ----------

    fn check_block(&mut self, block: &Block) {
        self.scopes.push(HashMap::new());
        for stmt in &block.stmts {
            self.check_stmt(stmt);
        }
        self.scopes.pop();
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Let {
                name, ty, value, ..
            } => {
                let vty = self.infer(value);
                let bind_ty = match ty {
                    Some(annot) => {
                        let generics = HashSet::new();
                        let aty = self.resolve(annot, &generics);
                        if !aty.compatible(&vty) {
                            self.error(
                                codes::TYPE_MISMATCH,
                                format!("se declaró `{aty}` pero el valor es `{vty}`"),
                                value.span,
                            );
                        }
                        aty
                    }
                    None => vty,
                };
                self.bind(name, bind_ty);
            }
            StmtKind::Assign { target, value, .. } => {
                self.infer(target);
                self.infer(value);
            }
            StmtKind::Return(opt) => {
                if let Some(e) = opt {
                    let rty = self.infer(e);
                    if !self.cur_ret.compatible(&rty) {
                        self.error(
                            codes::TYPE_MISMATCH,
                            format!(
                                "se devuelve `{rty}` pero la función declara `{}`",
                                self.cur_ret
                            ),
                            e.span,
                        );
                    }
                }
            }
            StmtKind::If(if_stmt) => self.check_if(if_stmt),
            StmtKind::While { cond, body } => {
                self.expect_bool(cond, "la condición de `while`");
                self.check_block(body);
            }
            StmtKind::For {
                binder, iter, body, ..
            } => {
                self.infer(iter);
                self.scopes.push(HashMap::new());
                self.bind(binder, Ty::Unknown);
                for s in &body.stmts {
                    self.check_stmt(s);
                }
                self.scopes.pop();
            }
            StmtKind::Match(m) => {
                self.infer(&m.scrutinee);
                for arm in &m.arms {
                    self.infer(&arm.body);
                }
            }
            StmtKind::Unsafe(block) => self.check_block(block),
            StmtKind::Assert(e) => {
                self.expect_bool(e, "`assert`");
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::Expr(e) => {
                self.infer(e);
            }
        }
    }

    fn check_if(&mut self, if_stmt: &IfStmt) {
        self.expect_bool(&if_stmt.cond, "la condición de `if`");
        self.check_block(&if_stmt.then);
        match &if_stmt.els {
            Some(Else::Block(b)) => self.check_block(b),
            Some(Else::If(nested)) => self.check_if(nested),
            None => {}
        }
    }

    fn expect_bool(&mut self, e: &Expr, what: &str) {
        let ty = self.infer(e);
        if !ty.is_unknown() && ty != Ty::Bool {
            self.error(
                codes::TYPE_MISMATCH,
                format!("{what} debe ser `Bool`, pero es `{ty}`"),
                e.span,
            );
        }
    }

    // ---------- expresiones ----------

    fn infer(&mut self, e: &Expr) -> Ty {
        match &e.kind {
            ExprKind::Int(_) => Ty::Int,
            ExprKind::Float(_) => Ty::Float,
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::NoneLit => Ty::Named("Option".to_string(), vec![Ty::Unknown]),
            ExprKind::Str(segs) => {
                for s in segs {
                    if let StrSeg::Interp(inner) = s {
                        self.infer(inner);
                    }
                }
                Ty::String
            }
            ExprKind::SelfExpr => self.lookup("self"),
            ExprKind::Ident(name) => self.lookup(name),
            ExprKind::Unit => Ty::Unit,
            ExprKind::List(items) => {
                let mut elem = Ty::Unknown;
                for it in items {
                    let t = self.infer(it);
                    if elem.is_unknown() {
                        elem = t;
                    }
                }
                Ty::Named("List".to_string(), vec![elem])
            }
            ExprKind::Map(entries) => {
                for (k, v) in entries {
                    self.infer(k);
                    self.infer(v);
                }
                Ty::Named("Map".to_string(), vec![Ty::Unknown, Ty::Unknown])
            }
            ExprKind::Tuple(items) => {
                for it in items {
                    self.infer(it);
                }
                Ty::Unknown
            }
            ExprKind::StructLit { name, fields } => {
                for (_, v) in fields {
                    self.infer(v);
                }
                Ty::Named(name.clone(), Vec::new())
            }
            ExprKind::EnumShorthand(_) => Ty::Unknown,
            ExprKind::Unary { op, expr } => {
                let t = self.infer(expr);
                match op {
                    UnOp::Not => {
                        if !t.is_unknown() && t != Ty::Bool {
                            self.error(
                                codes::TYPE_MISMATCH,
                                format!("`not` requiere `Bool`, pero recibe `{t}`"),
                                expr.span,
                            );
                        }
                        Ty::Bool
                    }
                    UnOp::Neg => {
                        if !t.is_unknown() && !t.is_numeric() {
                            self.error(
                                codes::TYPE_MISMATCH,
                                format!("`-` requiere un número, pero recibe `{t}`"),
                                expr.span,
                            );
                        }
                        t
                    }
                }
            }
            ExprKind::Binary { op, lhs, rhs } => self.infer_binary(*op, lhs, rhs),
            ExprKind::Range { start, end } => {
                self.infer(start);
                self.infer(end);
                Ty::Unknown
            }
            ExprKind::Pipe { lhs, rhs } => {
                self.infer(lhs);
                self.infer(rhs)
            }
            ExprKind::Try(inner) => {
                let t = self.infer(inner);
                match &t {
                    Ty::Named(n, args) if n == "Result" => {
                        args.first().cloned().unwrap_or(Ty::Unknown)
                    }
                    Ty::Named(n, args) if n == "Option" => {
                        args.first().cloned().unwrap_or(Ty::Unknown)
                    }
                    Ty::Unknown => Ty::Unknown,
                    other => {
                        self.error(
                            codes::NOT_RESULT_OR_OPTION,
                            format!("`?` requiere `Result` u `Option`, pero la expresión es `{other}`"),
                            e.span,
                        );
                        Ty::Unknown
                    }
                }
            }
            ExprKind::Await(inner) => self.infer(inner),
            ExprKind::Yield(inner) => {
                self.infer(inner);
                Ty::Unknown
            }
            ExprKind::Ref { mutable, expr } => Ty::Ref {
                mutable: *mutable,
                inner: Box::new(self.infer(expr)),
            },
            ExprKind::Deref(inner) => match self.infer(inner) {
                Ty::Ref { inner, .. } => *inner,
                Ty::Ptr(inner) => *inner,
                Ty::Boxed(inner) => *inner,
                _ => Ty::Unknown,
            },
            ExprKind::BoxExpr(inner) => Ty::Boxed(Box::new(self.infer(inner))),
            ExprKind::Closure { params, body } => {
                self.scopes.push(HashMap::new());
                let mut ptys = Vec::new();
                for p in params {
                    let ty = match &p.ty {
                        Some(t) => {
                            let g = HashSet::new();
                            self.resolve_silent(t, &g)
                        }
                        None => Ty::Unknown,
                    };
                    self.bind(&p.name, ty.clone());
                    ptys.push(ty);
                }
                let ret = match body.as_ref() {
                    ClosureBody::Expr(ex) => self.infer(ex),
                    ClosureBody::Block(b) => {
                        for s in &b.stmts {
                            self.check_stmt(s);
                        }
                        Ty::Unknown
                    }
                };
                self.scopes.pop();
                Ty::Func(ptys, Box::new(ret))
            }
            ExprKind::Match(m) => {
                self.infer(&m.scrutinee);
                for arm in &m.arms {
                    self.infer(&arm.body);
                }
                Ty::Unknown
            }
            ExprKind::Field { recv, .. } => {
                self.infer(recv);
                Ty::Unknown
            }
            ExprKind::Index { recv, index } => {
                self.infer(recv);
                self.infer(index);
                Ty::Unknown
            }
            ExprKind::MethodCall { recv, args, .. } => {
                self.infer(recv);
                for a in args {
                    self.infer(&a.value);
                }
                Ty::Unknown
            }
            ExprKind::Call {
                callee,
                args,
                ..
            } => self.infer_call(callee, args, e.span),
            ExprKind::Error => Ty::Unknown,
        }
    }

    fn infer_binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Ty {
        let lt = self.infer(lhs);
        let rt = self.infer(rhs);
        let span = lhs.span.to(rhs.span);
        use BinOp::*;
        match op {
            Add | Sub | Mul | Div | Rem => {
                if !lt.is_unknown() && !rt.is_unknown() {
                    if !lt.is_numeric() || !rt.is_numeric() {
                        self.error(
                            codes::TYPE_MISMATCH,
                            format!("operación aritmética entre `{lt}` y `{rt}`"),
                            span,
                        );
                    } else if lt != rt {
                        self.error(
                            codes::TYPE_MISMATCH,
                            format!("sin conversiones implícitas: `{lt}` y `{rt}` no se mezclan"),
                            span,
                        );
                    }
                }
                if lt.is_numeric() {
                    lt
                } else if rt.is_numeric() {
                    rt
                } else {
                    Ty::Unknown
                }
            }
            Eq | Ne | Lt | Le | Gt | Ge => {
                if !lt.is_unknown() && !rt.is_unknown() && !lt.compatible(&rt) {
                    self.error(
                        codes::TYPE_MISMATCH,
                        format!("comparación entre tipos distintos: `{lt}` y `{rt}`"),
                        span,
                    );
                }
                Ty::Bool
            }
            And | Or => {
                for (t, e) in [(&lt, lhs), (&rt, rhs)] {
                    if !t.is_unknown() && *t != Ty::Bool {
                        self.error(
                            codes::TYPE_MISMATCH,
                            format!("`and`/`or` requieren `Bool`, pero hay `{t}`"),
                            e.span,
                        );
                    }
                }
                Ty::Bool
            }
        }
    }

    fn infer_call(&mut self, callee: &Expr, args: &[Arg], span: Span) -> Ty {
        // Siempre inferir los argumentos (para chequear lo que haya dentro).
        let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer(&a.value)).collect();

        let ExprKind::Ident(name) = &callee.kind else {
            self.infer(callee);
            return Ty::Unknown;
        };

        // Solo validamos llamadas a funciones de nivel superior conocidas.
        let Some(sig) = self.fns.get(name) else {
            return Ty::Unknown;
        };
        let (required, total, ret, params, uses) = (
            sig.required,
            sig.total,
            sig.ret.clone(),
            sig.params.clone(),
            sig.uses.clone(),
        );
        let has_named = args.iter().any(|a| a.name.is_some());

        // Aridad (solo con argumentos posicionales).
        if !has_named && (args.len() < required || args.len() > total) {
            let esperado = if required == total {
                format!("{total}")
            } else {
                format!("entre {required} y {total}")
            };
            self.error(
                codes::WRONG_ARITY,
                format!(
                    "`{name}` espera {esperado} argumento(s), pero recibe {}",
                    args.len()
                ),
                span,
            );
        } else if !has_named && args.len() == params.len() {
            // Tipos de los argumentos posicionales.
            for (i, pty) in params.iter().enumerate() {
                if !pty.compatible(&arg_tys[i]) {
                    self.error(
                        codes::TYPE_MISMATCH,
                        format!(
                            "el argumento {} de `{name}` es `{}` pero se esperaba `{pty}`",
                            i + 1,
                            arg_tys[i]
                        ),
                        args[i].span,
                    );
                }
            }
        }

        // Capacidades: el llamador debe declarar al menos las del llamado.
        if let Some(declared) = self.cur_uses.clone() {
            let missing: Vec<&String> = uses.iter().filter(|u| !declared.contains(*u)).collect();
            if !missing.is_empty() {
                let list = missing
                    .iter()
                    .map(|s| format!("`{s}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.error(
                    codes::MISSING_CAPABILITY,
                    format!("se llama a `{name}` sin declarar la(s) capacidad(es) {list} (usa `uses`)"),
                    span,
                );
            }
        }

        ret
    }

    // ---------- alcances y diagnósticos ----------

    fn bind(&mut self, name: &str, ty: Ty) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), ty);
        }
    }

    fn lookup(&self, name: &str) -> Ty {
        for scope in self.scopes.iter().rev() {
            if let Some(t) = scope.get(name) {
                return t.clone();
            }
        }
        Ty::Unknown
    }

    fn error(&mut self, code: &'static str, msg: impl Into<String>, span: Span) {
        self.diags.push(Diagnostic::error(code, msg, span));
    }
}

// ---------- tipos base ----------

fn scalar_ty(name: &str) -> Option<Ty> {
    Some(match name {
        "Int" => Ty::Int,
        "Float" => Ty::Float,
        "Bool" => Ty::Bool,
        "String" => Ty::String,
        "Bytes" => Ty::Bytes,
        "Url" => Ty::Url,
        "Duration" => Ty::Duration,
        // `Error` es el tipo de error base del lenguaje.
        "Error" => Ty::Named("Error".to_string(), Vec::new()),
        _ => return None,
    })
}

fn is_builtin_generic(name: &str) -> bool {
    matches!(name, "List" | "Map" | "Set" | "Option" | "Result")
}
