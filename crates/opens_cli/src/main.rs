//! `opens`: herramienta de línea de comandos de OpenS.

use opens_syntax::diagnostic::{apply_fixes, codes, render_human, render_json, Diagnostic, Severity};
use opens_syntax::{lex, StrPart, Token, TokenKind};
use std::process::ExitCode;

const USAGE: &str = "\
opens — compilador del lenguaje OpenS (v0.3: léxico + sintaxis + tipos)

USO:
  opens check <archivo> [--json]   Revisa léxico, sintaxis, tipos y capacidades
  opens fix <archivo>              Aplica los arreglos automáticos en el archivo
  opens lex <archivo> [--json]     Muestra los tokens (útil para depurar)
  opens parse <archivo>            Muestra el AST (útil para depurar)
  opens explain <código>           Explica un código de error (ej. E0106, E0203)
  opens version                    Muestra la versión

Con --json, la salida sigue el esquema `opens.diagnostics/v1`,
pensado para que agentes de IA lean, corrijan y recompilen solos.
";

/// Explica un código de error de léxico (E01xx), sintaxis (E02xx), tipos
/// (E03xx) o capacidades (E04xx).
fn explain(code: &str) -> Option<&'static str> {
    codes::explain(code)
        .or_else(|| opens_parser::parser::codes::explain(code))
        .or_else(|| opens_types::codes::explain(code))
}

/// Analiza una fuente en cascada: léxico → sintaxis → tipos/capacidades.
/// Cada fase solo corre si la anterior no tuvo errores (así no hay ruido).
fn analyze(src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let lexed = lex(src);
    if lexed.diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return (lexed.tokens, lexed.diagnostics);
    }
    let parsed = opens_parser::parse(lexed.tokens.clone());
    if parsed.diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return (lexed.tokens, parsed.diagnostics);
    }
    let type_diags = opens_types::check_module(&parsed.module);
    (lexed.tokens, type_diags)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let pos: Vec<&str> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(String::as_str)
        .collect();

    match pos.as_slice() {
        ["check", file] => check(file, json),
        ["fix", file] => fix(file),
        ["lex", file] => dump_tokens(file, json),
        ["parse", file] => dump_ast(file),
        ["explain", code] => match explain(code) {
            Some(text) => {
                println!("{code}: {text}");
                ExitCode::SUCCESS
            }
            None => {
                eprintln!("código desconocido: {code}");
                ExitCode::from(2)
            }
        },
        ["version"] => {
            println!("opens {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            print!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn read(file: &str) -> Result<String, ExitCode> {
    std::fs::read_to_string(file).map_err(|e| {
        eprintln!("no se pudo leer `{file}`: {e}");
        ExitCode::from(2)
    })
}

fn check(file: &str, json: bool) -> ExitCode {
    let src = match read(file) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let (_tokens, diagnostics) = analyze(&src);
    if json {
        println!("{}", render_json(file, &src, &diagnostics));
    } else if diagnostics.is_empty() {
        println!("✓ {file}: sin errores");
    } else {
        eprint!("{}", render_human(file, &src, &diagnostics));
        let fixable = diagnostics.iter().filter(|d| d.fix.is_some()).count();
        eprintln!(
            "{} error(es); {} se pueden arreglar con `opens fix {}`",
            diagnostics.len(),
            fixable,
            file
        );
    }
    if diagnostics.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn fix(file: &str) -> ExitCode {
    let src = match read(file) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let r = lex(&src);
    let n = r.diagnostics.iter().filter(|d| d.fix.is_some()).count();
    if n == 0 {
        println!("nada que arreglar en {file}");
        return ExitCode::SUCCESS;
    }
    let fixed = apply_fixes(&src, &r.diagnostics);
    if let Err(e) = std::fs::write(file, fixed) {
        eprintln!("no se pudo escribir `{file}`: {e}");
        return ExitCode::from(2);
    }
    println!("✓ {n} arreglo(s) aplicados en {file}");
    ExitCode::SUCCESS
}

fn dump_tokens(file: &str, json: bool) -> ExitCode {
    let src = match read(file) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let r = lex(&src);
    if json {
        let items: Vec<String> = r.tokens.iter().map(token_json).collect();
        println!("[{}]", items.join(","));
    } else {
        for t in &r.tokens {
            print_token(t, 0);
        }
    }
    if !r.diagnostics.is_empty() {
        eprint!("{}", render_human(file, &src, &r.diagnostics));
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn dump_ast(file: &str) -> ExitCode {
    let src = match read(file) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let (module, diags) = opens_parser::parse_source(&src);
    match module {
        Some(m) => {
            // El AST deriva `Debug`; con `{:#?}` sale legible e indentado.
            println!("{m:#?}");
        }
        None => {
            eprintln!("no se parseó por errores léxicos:");
        }
    }
    if !diags.is_empty() {
        eprint!("{}", render_human(file, &src, &diags));
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn print_token(t: &Token, indent: usize) {
    let pad = "  ".repeat(indent);
    match &t.kind {
        TokenKind::Str(parts) => {
            println!("{pad}{:>5}..{:<5} Str", t.span.start, t.span.end);
            for p in parts {
                match p {
                    StrPart::Text(s) => println!("{pad}  texto {s:?}"),
                    StrPart::Interp(toks) => {
                        println!("{pad}  interpolación:");
                        for it in toks {
                            print_token(it, indent + 2);
                        }
                    }
                }
            }
        }
        k => println!(
            "{pad}{:>5}..{:<5} {}",
            t.span.start,
            t.span.end,
            k.describe()
        ),
    }
}

fn token_json(t: &Token) -> String {
    let kind = match &t.kind {
        TokenKind::Ident(_) => "ident",
        TokenKind::Int(_) => "int",
        TokenKind::Float(_) => "float",
        TokenKind::Str(_) => "string",
        TokenKind::Keyword(_) => "keyword",
        TokenKind::DocComment(_) => "doc",
        TokenKind::Eof => "eof",
        _ => "punct",
    };
    format!(
        "{{\"kind\":\"{}\",\"start\":{},\"end\":{}}}",
        kind, t.span.start, t.span.end
    )
}
