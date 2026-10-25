//! # opens_syntax
//!
//! Frontend léxico de OpenS: tokens, lexer y diagnósticos.
//!
//! Este crate no tiene dependencias para poder compilarse a cualquier
//! plataforma: nativo, `wasm32-unknown-unknown` (playground web) y móvil.

pub mod diagnostic;
pub mod lexer;
pub mod span;
pub mod token;

pub use diagnostic::{Diagnostic, Severity};
pub use lexer::{lex, LexResult};
pub use span::Span;
pub use token::{Keyword, StrPart, Token, TokenKind};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::{apply_fixes, codes, render_json};

    fn kinds(src: &str) -> Vec<TokenKind> {
        let r = lex(src);
        assert!(
            r.diagnostics.is_empty(),
            "diagnósticos: {:?}",
            r.diagnostics
        );
        r.tokens.into_iter().map(|t| t.kind).collect()
    }

    fn codes_of(src: &str) -> Vec<&'static str> {
        lex(src).diagnostics.iter().map(|d| d.code).collect()
    }

    #[test]
    fn keywords_e_identificadores() {
        use TokenKind::*;
        assert_eq!(
            kinds("tool buscar uses net"),
            vec![
                Keyword(token::Keyword::Tool),
                Ident("buscar".into()),
                Keyword(token::Keyword::Uses),
                Ident("net".into()),
                Eof
            ]
        );
    }

    #[test]
    fn identificadores_unicode() {
        assert_eq!(kinds("año")[0], TokenKind::Ident("año".into()));
    }

    #[test]
    fn numeros() {
        use TokenKind::*;
        assert_eq!(
            kinds("42 1_000 2.5 0xFF 0b101 2e3"),
            vec![
                Int(42),
                Int(1000),
                Float(2.5),
                Int(255),
                Int(5),
                Float(2000.0),
                Eof
            ]
        );
    }

    #[test]
    fn rango_no_es_decimal() {
        use TokenKind::*;
        assert_eq!(kinds("1..10"), vec![Int(1), DotDot, Int(10), Eof]);
    }

    #[test]
    fn operadores() {
        use TokenKind::*;
        assert_eq!(
            kinds("-> => == != <= >= |> ? @"),
            vec![Arrow, FatArrow, EqEq, NotEq, Le, Ge, Pipe, Question, At, Eof]
        );
    }

    #[test]
    fn string_con_interpolacion() {
        let k = kinds("\"hola {nombre}!\"");
        let TokenKind::Str(parts) = &k[0] else {
            panic!()
        };
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], StrPart::Text("hola ".into()));
        let StrPart::Interp(inner) = &parts[1] else {
            panic!()
        };
        assert_eq!(inner[0].kind, TokenKind::Ident("nombre".into()));
        assert_eq!(parts[2], StrPart::Text("!".into()));
    }

    #[test]
    fn interpolacion_anidada() {
        let k = kinds("\"{ f({a: 1}) }\"");
        let TokenKind::Str(parts) = &k[0] else {
            panic!()
        };
        let StrPart::Interp(inner) = &parts[0] else {
            panic!()
        };
        assert_eq!(inner.len(), 8); // f ( { a : 1 } )
    }

    #[test]
    fn escapes() {
        let k = kinds(r#""a\n\{x\}\u{1F600}""#);
        assert_eq!(k[0], TokenKind::Str(vec![StrPart::Text("a\n{x}😀".into())]));
    }

    #[test]
    fn string_multilinea() {
        let src = "\"\"\"\n  Hola\n  mundo\n  \"\"\"";
        let k = kinds(src);
        assert_eq!(
            k[0],
            TokenKind::Str(vec![StrPart::Text("  Hola\n  mundo".into())])
        );
    }

    #[test]
    fn comentarios() {
        use TokenKind::*;
        assert_eq!(
            kinds("a // nada\n/* bloque /* anidado */ */ b\n/// doc\nc"),
            vec![
                Ident("a".into()),
                Ident("b".into()),
                DocComment("doc".into()),
                Ident("c".into()),
                Eof
            ]
        );
    }

    #[test]
    fn operadores_extranjeros_con_arreglo() {
        let src = "if a && b || !c { x++ }";
        let r = lex(src);
        assert_eq!(r.diagnostics.len(), 4);
        assert!(r
            .diagnostics
            .iter()
            .all(|d| d.code == codes::FOREIGN_OPERATOR));
        let fixed = apply_fixes(src, &r.diagnostics);
        assert_eq!(fixed, "if a and b or not c { x += 1 }");
        assert!(lex(&fixed).diagnostics.is_empty());
    }

    #[test]
    fn igualdad_estricta_de_js() {
        let r = lex("a === b");
        assert_eq!(r.diagnostics[0].code, codes::FOREIGN_OPERATOR);
        assert_eq!(r.tokens[1].kind, TokenKind::EqEq);
    }

    #[test]
    fn comillas_simples() {
        let src = "let s = 'hola'";
        let r = lex(src);
        assert_eq!(r.diagnostics[0].code, codes::SINGLE_QUOTE_STRING);
        assert_eq!(apply_fixes(src, &r.diagnostics), "let s = \"hola\"");
    }

    #[test]
    fn string_sin_cerrar() {
        let src = "let s = \"hola\nlet x = 1";
        let r = lex(src);
        assert_eq!(r.diagnostics[0].code, codes::UNTERMINATED_STRING);
        assert_eq!(
            apply_fixes(src, &r.diagnostics),
            "let s = \"hola\"\nlet x = 1"
        );
        // El lexer se recupera y sigue tokenizando la línea siguiente.
        assert!(r
            .tokens
            .iter()
            .any(|t| t.kind == TokenKind::Ident("x".into())));
    }

    #[test]
    fn numero_invalido() {
        assert_eq!(codes_of("12abc"), vec![codes::INVALID_NUMBER]);
        assert_eq!(
            codes_of("99999999999999999999"),
            vec![codes::INVALID_NUMBER]
        );
    }

    #[test]
    fn caracter_inesperado_y_recuperacion() {
        let r = lex("a # b");
        assert_eq!(r.diagnostics[0].code, codes::UNEXPECTED_CHAR);
        assert_eq!(r.tokens.len(), 3); // a b Eof
    }

    #[test]
    fn json_estable() {
        let src = "x && y";
        let r = lex(src);
        let json = render_json("main.ops", src, &r.diagnostics);
        assert!(json.starts_with("{\"schema\":\"opens.diagnostics/v1\""));
        assert!(json.contains("\"code\":\"E0106\""));
        assert!(json.contains("\"line\":1,\"col\":3"));
        assert!(json.contains("\"ok\":false"));
    }

    #[test]
    fn todos_los_codigos_tienen_explicacion() {
        for c in [
            "E0101", "E0102", "E0103", "E0104", "E0105", "E0106", "E0107", "E0108",
        ] {
            assert!(codes::explain(c).is_some(), "{c}");
        }
    }
}
