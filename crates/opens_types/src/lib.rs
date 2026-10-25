//! # opens_types
//!
//! Chequeo de tipos y de capacidades de OpenS (hito 0.3). Trabaja sobre el AST
//! de `opens_parser` y produce los mismos `Diagnostic` que el resto del
//! compilador (códigos `E03xx` de tipos y `E04xx` de capacidades).
//!
//! Es conservador: ante la duda no reporta (ver [`check`]).

pub mod check;
pub mod ty;

pub use check::{check_module, codes};
pub use ty::Ty;

/// Lexa, parsea y chequea una fuente. Devuelve todos los diagnósticos; si una
/// fase temprana falla con errores, no pasa a la siguiente (evita ruido).
pub fn check_source(src: &str) -> Vec<opens_syntax::Diagnostic> {
    let lexed = opens_syntax::lex(src);
    if lexed
        .diagnostics
        .iter()
        .any(|d| d.severity == opens_syntax::Severity::Error)
    {
        return lexed.diagnostics;
    }
    let parsed = opens_parser::parse(lexed.tokens);
    if parsed
        .diagnostics
        .iter()
        .any(|d| d.severity == opens_syntax::Severity::Error)
    {
        return parsed.diagnostics;
    }
    check_module(&parsed.module)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes_of(src: &str) -> Vec<&'static str> {
        let lexed = opens_syntax::lex(src);
        assert!(lexed.diagnostics.is_empty(), "léxico: {:?}", lexed.diagnostics);
        let parsed = opens_parser::parse(lexed.tokens);
        assert!(parsed.diagnostics.is_empty(), "sintaxis: {:?}", parsed.diagnostics);
        check_module(&parsed.module)
            .iter()
            .map(|d| d.code)
            .collect()
    }

    #[test]
    fn ejemplos_sin_errores_de_tipo() {
        for src in [
            include_str!("../../../examples/resumidor.ops"),
            include_str!("../../../examples/contador.ops"),
        ] {
            assert_eq!(codes_of(src), Vec::<&str>::new(), "en:\n{src}");
        }
    }

    #[test]
    fn tipo_desconocido_en_firma() {
        assert!(codes_of("fn f(x: Foo) { }").contains(&codes::UNKNOWN_TYPE));
    }

    #[test]
    fn let_con_anotacion_incompatible() {
        assert!(codes_of("fn f() { let x: Int = \"s\"; }").contains(&codes::TYPE_MISMATCH));
    }

    #[test]
    fn condicion_no_booleana() {
        assert!(codes_of("fn f() { if 1 { } }").contains(&codes::TYPE_MISMATCH));
    }

    #[test]
    fn aritmetica_mezcla_int_y_float() {
        assert!(codes_of("fn f() { let x = 1 + 2.0; }").contains(&codes::TYPE_MISMATCH));
    }

    #[test]
    fn try_sobre_no_result() {
        assert!(codes_of("fn f() { let x = 5?; }").contains(&codes::NOT_RESULT_OR_OPTION));
    }

    #[test]
    fn retorno_incompatible() {
        assert!(
            codes_of("fn f() -> Int { return \"x\"; }").contains(&codes::TYPE_MISMATCH)
        );
    }

    #[test]
    fn aridad_incorrecta() {
        assert!(
            codes_of("fn g(a: Int) { } fn f() { g(1, 2); }").contains(&codes::WRONG_ARITY)
        );
    }

    #[test]
    fn tipo_de_argumento_incorrecto() {
        assert!(
            codes_of("fn g(a: Int) { } fn f() { g(\"x\"); }").contains(&codes::TYPE_MISMATCH)
        );
    }

    #[test]
    fn capacidad_faltante() {
        let src = "fn fetch() -> Int uses net { return 0; } fn f() -> Int { return fetch(); }";
        assert!(codes_of(src).contains(&codes::MISSING_CAPABILITY));
    }

    #[test]
    fn capacidad_declarada_ok() {
        let src = "fn fetch() -> Int uses net { return 0; } fn f() -> Int uses net { return fetch(); }";
        assert_eq!(codes_of(src), Vec::<&str>::new());
    }

    #[test]
    fn url_acepta_literal_string() {
        // Pasar un string a un parámetro `Url` no es un error.
        let src = "fn abrir(u: Url) { } fn f() { abrir(\"https://x.com\"); }";
        assert_eq!(codes_of(src), Vec::<&str>::new());
    }

    #[test]
    fn todos_los_codigos_tienen_explicacion() {
        for c in ["E0301", "E0302", "E0303", "E0304", "E0401"] {
            assert!(codes::explain(c).is_some(), "{c}");
        }
    }
}
