//! # opens_parser
//!
//! Convierte los tokens de `opens_syntax` en un AST (`ast::Module`), con
//! recuperación de errores: ante un error de sintaxis emite un diagnóstico,
//! se sincroniza y continúa, para reportar todos los problemas de una pasada.
//!
//! Sin dependencias (aparte de `opens_syntax`), para compilar igual en nativo,
//! `wasm32` y móvil.

pub mod ast;
pub mod parser;

pub use parser::{parse, ParseResult};

/// Atajo: lexa y parsea una fuente, uniendo los diagnósticos de ambas fases.
/// Si el lexer encuentra errores, no se parsea (los tokens serían ruido).
pub fn parse_source(src: &str) -> (Option<ast::Module>, Vec<opens_syntax::Diagnostic>) {
    let lexed = opens_syntax::lex(src);
    if lexed.diagnostics.iter().any(|d| {
        d.severity == opens_syntax::Severity::Error
    }) {
        return (None, lexed.diagnostics);
    }
    let result = parse(lexed.tokens);
    (Some(result.module), result.diagnostics)
}

#[cfg(test)]
mod tests {
    use super::ast::*;
    use super::*;
    use opens_syntax::lex;

    fn parse_ok(src: &str) -> Module {
        let r = parse(lex(src).tokens);
        assert!(
            r.diagnostics.is_empty(),
            "diagnósticos inesperados en `{src}`: {:?}",
            r.diagnostics
        );
        r.module
    }

    fn codes_of(src: &str) -> Vec<&'static str> {
        parse(lex(src).tokens)
            .diagnostics
            .iter()
            .map(|d| d.code)
            .collect()
    }

    #[test]
    fn funcion_simple() {
        let m = parse_ok("fn sumar(a: Int, b: Int) -> Int { return a + b }");
        assert_eq!(m.items.len(), 1);
        let ItemKind::Function(f) = &m.items[0].kind else {
            panic!("no es función");
        };
        assert_eq!(f.name, "sumar");
        assert_eq!(f.params.len(), 2);
        assert_eq!(f.params[0].name, "a");
        assert!(matches!(&f.ret, Some(Type { kind: TypeKind::Named { name, .. }, .. }) if name == "Int"));
        assert_eq!(f.body.as_ref().unwrap().stmts.len(), 1);
    }

    #[test]
    fn pub_tool_con_uses_y_capacidades() {
        let m = parse_ok(
            "pub tool clima(ciudad: String) -> Result<Clima, Error> uses net, llm { return ok(1) }",
        );
        assert!(m.items[0].is_pub);
        let ItemKind::Function(f) = &m.items[0].kind else {
            panic!();
        };
        assert_eq!(f.flavor, FnFlavor::Tool);
        assert_eq!(f.uses, vec!["net".to_string(), "llm".to_string()]);
        let Some(Type { kind: TypeKind::Named { name, args }, .. }) = &f.ret else {
            panic!();
        };
        assert_eq!(name, "Result");
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn doc_comment_se_adjunta() {
        let m = parse_ok("/// Suma dos números.\nfn s(a: Int, b: Int) -> Int { return a + b }");
        assert_eq!(m.items[0].doc.as_deref(), Some("Suma dos números."));
    }

    #[test]
    fn tipo_con_campos_y_anotaciones() {
        let m = parse_ok(
            "type Usuario {\n  nombre: String\n  edad: Int @min(0)\n  correo: Option<String>\n}",
        );
        let ItemKind::TypeDef(t) = &m.items[0].kind else {
            panic!();
        };
        assert_eq!(t.fields.len(), 3);
        assert_eq!(t.fields[1].name, "edad");
        assert_eq!(t.fields[1].attrs.len(), 1);
        assert_eq!(t.fields[1].attrs[0].name, "min");
    }

    #[test]
    fn enum_inline_en_campo() {
        let m = parse_ok("type R {\n  s: enum { positivo, neutral, negativo }\n}");
        let ItemKind::TypeDef(t) = &m.items[0].kind else {
            panic!();
        };
        assert!(matches!(&t.fields[0].ty.kind, TypeKind::InlineEnum(v) if v.len() == 3));
    }

    #[test]
    fn referencias_punteros_y_box() {
        // Referencia inmutable, mutable, puntero crudo y heap.
        let m = parse_ok(
            "fn f(a: &Int, b: &var Int, c: *Int, d: box String) -> Int { return 0 }",
        );
        let ItemKind::Function(f) = &m.items[0].kind else {
            panic!();
        };
        assert!(matches!(f.params[0].ty.kind, TypeKind::Ref { mutable: false, .. }));
        assert!(matches!(f.params[1].ty.kind, TypeKind::Ref { mutable: true, .. }));
        assert!(matches!(f.params[2].ty.kind, TypeKind::Ptr { .. }));
        assert!(matches!(f.params[3].ty.kind, TypeKind::Box { .. }));
    }

    #[test]
    fn expresiones_de_memoria() {
        let m = parse_ok(
            "fn f() { let p = &x; let q = &var y; let v = *p; let h = box 3; unsafe { let z = *q } }",
        );
        let ItemKind::Function(f) = &m.items[0].kind else {
            panic!();
        };
        let stmts = &f.body.as_ref().unwrap().stmts;
        let StmtKind::Let { value, .. } = &stmts[0].kind else {
            panic!();
        };
        assert!(matches!(value.kind, ExprKind::Ref { mutable: false, .. }));
        let StmtKind::Let { value, .. } = &stmts[1].kind else {
            panic!();
        };
        assert!(matches!(value.kind, ExprKind::Ref { mutable: true, .. }));
        let StmtKind::Let { value, .. } = &stmts[2].kind else {
            panic!();
        };
        assert!(matches!(value.kind, ExprKind::Deref(_)));
        let StmtKind::Let { value, .. } = &stmts[3].kind else {
            panic!();
        };
        assert!(matches!(value.kind, ExprKind::BoxExpr(_)));
        assert!(matches!(stmts[4].kind, StmtKind::Unsafe(_)));
    }

    #[test]
    fn oop_impl_y_trait() {
        let m = parse_ok(
            "trait Saludable { fn saludar(&self) -> String }\n\
             impl Saludable for Usuario {\n\
               fn saludar(&self) -> String { return \"hola\" }\n\
             }",
        );
        let ItemKind::Trait(t) = &m.items[0].kind else {
            panic!("no es trait");
        };
        assert_eq!(t.methods.len(), 1);
        assert_eq!(t.methods[0].self_kind, Some(SelfKind::Ref));
        assert!(t.methods[0].body.is_none());

        let ItemKind::Impl(i) = &m.items[1].kind else {
            panic!("no es impl");
        };
        assert_eq!(i.trait_name.as_deref(), Some("Saludable"));
        assert_eq!(i.type_name, "Usuario");
        assert!(i.methods[0].body.is_some());
    }

    #[test]
    fn self_por_valor_y_mutable() {
        let m = parse_ok("impl T { fn a(self) {} fn b(&var self) {} fn c(x: Int) {} }");
        let ItemKind::Impl(i) = &m.items[0].kind else {
            panic!();
        };
        assert_eq!(i.methods[0].self_kind, Some(SelfKind::Value));
        assert_eq!(i.methods[1].self_kind, Some(SelfKind::RefVar));
        assert_eq!(i.methods[2].self_kind, None);
        assert_eq!(i.methods[2].params.len(), 1);
    }

    #[test]
    fn closures_y_tipo_funcion() {
        let m = parse_ok(
            "fn f(g: fn(Int) -> Int) { let doble = fn(x) => x * 2; let s = fn(a: Int) { return a } }",
        );
        let ItemKind::Function(f) = &m.items[0].kind else {
            panic!();
        };
        assert!(matches!(f.params[0].ty.kind, TypeKind::Func { .. }));
        let stmts = &f.body.as_ref().unwrap().stmts;
        let StmtKind::Let { value, .. } = &stmts[0].kind else {
            panic!();
        };
        assert!(matches!(value.kind, ExprKind::Closure { .. }));
    }

    #[test]
    fn llamada_generica_ask() {
        let m = parse_ok("fn f() { let r = await llm.ask<Resumen>(p)? }");
        let ItemKind::Function(f) = &m.items[0].kind else {
            panic!();
        };
        let StmtKind::Let { value, .. } = &f.body.as_ref().unwrap().stmts[0].kind else {
            panic!();
        };
        // await ( (llm.ask<Resumen>(p)) ? )
        assert!(matches!(value.kind, ExprKind::Await(_)));
    }

    #[test]
    fn menor_que_sigue_siendo_comparacion() {
        // `a < b` no debe confundirse con genéricos.
        let m = parse_ok("fn f() { let c = a < b }");
        let ItemKind::Function(f) = &m.items[0].kind else {
            panic!();
        };
        let StmtKind::Let { value, .. } = &f.body.as_ref().unwrap().stmts[0].kind else {
            panic!();
        };
        assert!(matches!(
            value.kind,
            ExprKind::Binary { op: BinOp::Lt, .. }
        ));
    }

    #[test]
    fn match_con_variantes_y_comodin() {
        let m = parse_ok(
            "fn f() { match estado {\n  .activo => uno()\n  .suspendido => dos()\n  _ => tres()\n} }",
        );
        let ItemKind::Function(f) = &m.items[0].kind else {
            panic!();
        };
        let StmtKind::Match(me) = &f.body.as_ref().unwrap().stmts[0].kind else {
            panic!();
        };
        assert_eq!(me.arms.len(), 3);
        assert!(matches!(me.arms[0].pat, Pattern::EnumVariant(_)));
        assert!(matches!(me.arms[2].pat, Pattern::Wildcard));
    }

    #[test]
    fn struct_lit_y_condicion_no_chocan() {
        // En la condición del `if`, la `{` abre el bloque, no un struct.
        let m = parse_ok("fn f() { if lista { return } let u = Usuario { nombre: \"x\" } }");
        let ItemKind::Function(f) = &m.items[0].kind else {
            panic!();
        };
        let stmts = &f.body.as_ref().unwrap().stmts;
        assert!(matches!(stmts[0].kind, StmtKind::If(_)));
        let StmtKind::Let { value, .. } = &stmts[1].kind else {
            panic!();
        };
        assert!(matches!(value.kind, ExprKind::StructLit { .. }));
    }

    #[test]
    fn string_con_interpolacion_parsea_expr() {
        let m = parse_ok("fn f() { let s = \"hola {nombre.apellido}\" }");
        let ItemKind::Function(f) = &m.items[0].kind else {
            panic!();
        };
        let StmtKind::Let { value, .. } = &f.body.as_ref().unwrap().stmts[0].kind else {
            panic!();
        };
        let ExprKind::Str(segs) = &value.kind else {
            panic!();
        };
        assert!(matches!(segs[0], StrSeg::Text(_)));
        let StrSeg::Interp(e) = &segs[1] else {
            panic!();
        };
        assert!(matches!(e.kind, ExprKind::Field { .. }));
    }

    #[test]
    fn for_await_stream() {
        let m = parse_ok("fn f() { for await parte in llm.stream(p) { print(parte) } }");
        let ItemKind::Function(f) = &m.items[0].kind else {
            panic!();
        };
        let StmtKind::For { is_await, binder, .. } = &f.body.as_ref().unwrap().stmts[0].kind else {
            panic!();
        };
        assert!(*is_await);
        assert_eq!(binder, "parte");
    }

    #[test]
    fn server_import_y_prompt() {
        let m = parse_ok(
            "import mi.util as u\n\
             prompt saludar(n: String) { \"hola {n}\" }\n\
             server S { expose a, b }",
        );
        assert_eq!(m.items.len(), 3);
        let ItemKind::Import(imp) = &m.items[0].kind else {
            panic!();
        };
        assert_eq!(imp.path, vec!["mi".to_string(), "util".to_string()]);
        assert_eq!(imp.alias.as_deref(), Some("u"));
        assert!(matches!(m.items[1].kind, ItemKind::Prompt(_)));
        let ItemKind::Server(s) = &m.items[2].kind else {
            panic!();
        };
        assert_eq!(s.exposes, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn extern_c() {
        let m = parse_ok("extern \"c\" fn sqrt(x: Float) -> Float");
        let ItemKind::Extern(e) = &m.items[0].kind else {
            panic!();
        };
        assert_eq!(e.abi, "c");
        assert_eq!(e.func.name, "sqrt");
        assert!(e.func.body.is_none());
    }

    #[test]
    fn test_item() {
        let m = parse_ok("test \"suma\" { assert sumar(2, 3) == 5 }");
        let ItemKind::Test(t) = &m.items[0].kind else {
            panic!();
        };
        assert_eq!(t.name, "suma");
        assert!(matches!(t.body.stmts[0].kind, StmtKind::Assert(_)));
    }

    #[test]
    fn pipe_y_rango() {
        let m = parse_ok("fn f() { let r = 0..10 |> map(doble) }");
        let ItemKind::Function(f) = &m.items[0].kind else {
            panic!();
        };
        let StmtKind::Let { value, .. } = &f.body.as_ref().unwrap().stmts[0].kind else {
            panic!();
        };
        assert!(matches!(value.kind, ExprKind::Pipe { .. }));
    }

    // -------- recuperación de errores --------

    #[test]
    fn recupera_y_sigue_tras_item_malo() {
        // Un ítem basura entre dos funciones válidas: se reporta uno y se
        // parsean las dos funciones.
        let src = "fn a() { return 1 }  @#!?  fn b() { return 2 }";
        let r = parse(lex(src).tokens);
        // El lexer marca `#`; aun así el parser debe recuperar.
        let m = r.module;
        let fns = m
            .items
            .iter()
            .filter(|i| matches!(i.kind, ItemKind::Function(_)))
            .count();
        assert_eq!(fns, 2, "módulo: {:?}", m.items);
    }

    #[test]
    fn falta_tipo_de_retorno() {
        // `-> {` sin tipo produce E0203 pero no cuelga.
        assert!(codes_of("fn f() -> { return 1 }").contains(&parser::codes::EXPECTED_TYPE));
    }

    #[test]
    fn parentesis_sin_cerrar() {
        assert!(codes_of("fn f() { let x = (1 + 2 }").contains(&parser::codes::UNCLOSED_DELIM));
    }

    #[test]
    fn todos_los_codigos_de_sintaxis_tienen_explicacion() {
        for c in [
            "E0201", "E0202", "E0203", "E0204", "E0205", "E0206", "E0207",
        ] {
            assert!(parser::codes::explain(c).is_some(), "{c}");
        }
    }

    #[test]
    fn ejemplo_resumidor_completo() {
        // El ejemplo del repositorio debe parsear sin errores.
        let src = include_str!("../../../examples/resumidor.ops");
        let (module, diags) = parse_source(src);
        assert!(diags.is_empty(), "diagnósticos: {diags:?}");
        assert!(module.is_some());
        let m = module.unwrap();
        assert!(m.items.iter().any(|i| matches!(i.kind, ItemKind::Server(_))));
        assert!(m.items.iter().any(|i| matches!(i.kind, ItemKind::Prompt(_))));
    }
}
