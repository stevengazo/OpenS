# OpenS

Lenguaje de programación de alto nivel y **multiparadigma** (funcional,
imperativo y orientado a objetos) para construir aplicaciones de IA (agentes,
servidores y clientes MCP, pipelines con LLMs), diseñado para que **los propios
LLMs lo escriban bien**, y que compila a **web, móvil y nativo** sin sacrificar
velocidad. Incluye un modelo de memoria explícito estilo C++ (referencias `&T`,
punteros crudos `*T` y heap con `box T`) para el código de bajo nivel.

> Estado: v0.3 — frontend léxico + parser con AST + **chequeo de tipos y
> capacidades** (lexer, parser, type checker, diagnósticos y CLI). El compilador
> corre en Linux, Windows y macOS. La especificación del lenguaje está en
> [`docs/SPEC.md`](docs/SPEC.md).

```
tool resumir_url(url: Url) -> Result<Resumen, Error>
  uses net, llm
{
  let pagina = await http.get(url)?;
  let r = await llm.ask<Resumen>(resumir(pagina.text))?;
  return ok(r);
}

server Resumidor {
  expose resumir_url
}
```

## Empezar

Requiere Rust (https://rustup.rs).

```bash
cargo test                                   # corre las pruebas
cargo run -- check examples/resumidor.ops    # revisa léxico y sintaxis
cargo run -- check examples/contador.ops     # objetos, memoria y closures
cargo run -- parse examples/contador.ops     # muestra el AST
cargo run -- check examples/errores.ops      # errores legibles, con arreglos
cargo run -- check examples/errores.ops --json
cargo run -- fix examples/errores.ops        # aplica los arreglos automáticos
cargo run -- explain E0203                   # explica un código de error
```

Extensión de archivo: `.ops`.

## Por qué Rust (y no C++)

| Criterio | Rust | C++ |
|---|---|---|
| WebAssembly (navegador, playground, edge) | `wasm32` de primera clase con cargo | Posible vía Emscripten, más fricción |
| iOS / Android | Targets oficiales, FFI con C | Bueno, pero toolchains por plataforma |
| Seguridad de memoria en el compilador | Garantizada | Manual |
| Backends nativos | Cranelift (rápido) y LLVM (inkwell) | LLVM nativo |
| Ecosistema de compiladores | rust-analyzer, swc, oxc, Ruff, Biome… | Clang, V8… |
| Gestor de paquetes | cargo | Fragmentado (CMake, vcpkg, Conan) |

C++ sigue presente donde aporta: OpenS tendrá FFI directo con C/C++ (`extern`)
para reutilizar librerías existentes (ONNX Runtime, llama.cpp, etc.).

## Arquitectura: un frontend, muchos destinos

```
             .ops
              │
   ┌──────────▼──────────┐
   │  opens_syntax       │  lexer → tokens → diagnósticos (JSON)
   │  opens_parser       │  parser → AST
   │  opens_types        │  tipos, inferencia, capacidades (`uses`)
   │  opens_ir (OIR)     │  IR intermedia, independiente del destino
   └──────────┬──────────┘
      ┌───────┼──────────────┬──────────────────┐
      ▼       ▼              ▼                  ▼
  TypeScript  WebAssembly    Nativo             Móvil
  (Node/Deno/ (navegador,    (Cranelift: dev    (lib estática +
   Bun, SDK    edge,          rápido; LLVM:      bindings Swift /
   MCP, npm)   plugins)       release óptimo)    Kotlin vía FFI)
```

Reglas de diseño para la portabilidad:

- Los crates del frontend **no tienen dependencias** y no usan E/S, para
  compilar igual en nativo, `wasm32` y móvil. El playground web corre el
  compilador completo dentro del navegador.
- Las capacidades (`uses net, fs, llm`) se resuelven por plataforma: en web,
  `fs` no existe y el compilador lo dice antes de ejecutar.
- La runtime (`opens_rt`) define una interfaz mínima (red, reloj, almacenamiento,
  LLM) que cada destino implementa.

## Hoja de ruta

- [x] **0.1** Lexer, diagnósticos con código estable, salida JSON, `opens fix`
- [x] **0.2** Parser + AST + recuperación de errores (`E02xx`, `opens parse`);
      multiparadigma (closures, `impl`/`trait`) y memoria (`&T`, `*T`, `box T`)
- [x] **0.3** Chequeo de tipos e inferencia local, `Result`/`Option`, y
      capacidades (`uses`); diagnósticos `E03xx`/`E04xx`
- [ ] **0.4** Backend TypeScript: `tool` / `server` → servidor MCP real
- [ ] **0.5** `ask<T>`, `prompt`, mocks y trazas; `opens test`
- [ ] **0.6** LSP para VS Code + formateador oficial
- [ ] **0.7** Backend WebAssembly + playground web
- [ ] **0.8** Backend nativo (Cranelift) + FFI con C/C++
- [ ] **0.9** Bindings móviles (iOS/Android)
- [ ] **Evaluación continua:** pedir a varios LLMs que escriban programas
      solo con `docs/SPEC.md` y medir cuántos compilan a la primera.

## Estructura

```
crates/
  opens_syntax/   tokens, lexer, diagnósticos (sin dependencias)
  opens_parser/   parser + AST + recuperación de errores
  opens_types/    chequeo de tipos y capacidades (`uses`)
  opens_cli/      binario `opens`
docs/SPEC.md      especificación del lenguaje
examples/         programas de ejemplo
```

## Licencia

[MIT](LICENSE) © 2026 Steven Gazo Maliaño.
