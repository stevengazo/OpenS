# Especificación de OpenS — v0.1 (borrador)

Este documento está escrito para que **una persona o un LLM** pueda aprender el
lenguaje completo leyéndolo una sola vez. Regla editorial: si algo necesita una
excepción para explicarse, se cambia el lenguaje, no la explicación.

## 0. Principios

1. **Una sola forma de hacer cada cosa.**
2. **Explícito sobre implícito**: sin conversiones automáticas, sin `null`
   sorpresa, sin excepciones invisibles.
3. **Las firmas públicas siempre tienen tipos**; dentro de las funciones se infieren.
4. **Los efectos se declaran** (`uses`): una función no toca red, disco ni un
   LLM sin decirlo.
5. **Todo error del compilador tiene código, ubicación y, si se puede, arreglo
   automático**, en texto y en JSON.

## 1. Léxico

- Archivos UTF-8, extensión `.ops`.
- Bloques con llaves `{ }`. La indentación no tiene significado.
- No se usan `;` al final de línea (se permiten para varias sentencias en una línea).
- Comentarios: `// línea`, `/* bloque /* anidable */ */`, `/// documentación`.
- Identificadores: letras Unicode, dígitos y `_` (no pueden empezar con dígito).
  Convención: `snake_case` para valores y funciones, `PascalCase` para tipos.

### Palabras clave

```
let var fn type enum import export pub
tool server client expose prompt agent uses
if else match for in while break continue return
async await stream yield
and or not true false none
test assert unsafe extern
```

### Literales

| Literal | Ejemplos |
|---|---|
| Entero (`Int`, 64 bits) | `42`, `1_000_000`, `0xFF`, `0b1010` |
| Decimal (`Float`, 64 bits) | `3.5`, `2e10`, `1.5E-3` |
| Booleano | `true`, `false` |
| Ausencia | `none` |
| String | `"hola {nombre}"` |
| String multilínea | `"""` … `"""` |
| Lista | `[1, 2, 3]` |
| Mapa | `{"a": 1, "b": 2}` |
| Rango | `0..10` (excluye el 10) |

**Strings**: siempre comillas dobles. `{expr}` interpola; `\{` es una llave
literal. Escapes: `\n \t \r \0 \\ \" \{ \} \u{1F600}`. En `"""`, el salto de
línea tras la apertura y la indentación antes del cierre se eliminan.

### Operadores

| Categoría | Operadores |
|---|---|
| Aritméticos | `+ - * / %` |
| Comparación | `== != < <= > >=` |
| Lógicos | `and or not` |
| Asignación | `= += -= *= /=` |
| Otros | `?` (propagar error), `|>` (encadenar), `..` (rango), `->` (tipo de retorno), `=>` (rama de `match`), `@` (anotación) |

**No existen** (el compilador da error con arreglo automático):
`&& || ! === !== ++ --` y strings con comilla simple.

## 2. Tipos

```
Int  Float  Bool  String  Bytes  Url  Duration
List<T>  Map<K, V>  Set<T>
Option<T>        // valor o none
Result<T, E>     // ok(valor) o err(error)
```

- No hay conversiones implícitas: `"5" + 1` es un error; usa `"5".to_int()?` o `1.to_string()`.
- `none` solo es válido en un `Option<T>`.

### Tipos de usuario

```
type Usuario {
  nombre: String
  edad: Int @min(0)
  correo: Option<String>
}

enum Estado { activo, suspendido, eliminado }
```

Las anotaciones (`@min`, `@max`, `@pattern`, `@doc`) se validan en tiempo de
ejecución y se exportan al JSON Schema de las herramientas MCP.

## 3. Variables y funciones

```
let x = 10          // inmutable
var total = 0       // mutable
total += x

fn sumar(a: Int, b: Int) -> Int {
  return a + b
}

pub fn dividir(a: Float, b: Float) -> Result<Float, String> {
  if b == 0.0 { return err("división entre cero") }
  return ok(a / b)
}
```

- `return` es obligatorio (no hay retorno implícito de la última expresión).
- Parámetros con valor por defecto: `fn f(x: Int = 1)`.
- Argumentos con nombre: `f(x: 2)`.

## 4. Control de flujo

```
if edad >= 18 and activo { … } else { … }

for item in lista { … }
for i in 0..10 { … }
while cola.len() > 0 { … }

match estado {
  .activo => print("ok")
  .suspendido => print("pausa")
  _ => print("otro")
}
```

`match` sobre un `enum` debe ser exhaustivo.

## 5. Errores

Los errores son valores. `?` devuelve el error al llamador si lo hay:

```
fn leer_config() -> Result<Config, Error> uses fs {
  let texto = fs.read("config.json")?
  let cfg = json.parse<Config>(texto)?
  return ok(cfg)
}
```

## 6. Async y streams

```
let r = await http.get(url)?

for await parte in llm.stream(p) {
  print(parte)
}
```

Toda función que usa `await` es asíncrona automáticamente; no se escribe `async`
en la firma (la palabra queda reservada).

## 7. Capacidades (`uses`)

```
fn guardar(d: Datos) -> Result<(), Error> uses fs, net { … }
```

Capacidades base: `net`, `fs`, `env`, `llm`, `time`, `random`, `ffi`.
Una función solo puede llamar a otra si declara al menos las mismas
capacidades. Así, revisando las firmas se sabe qué puede hacer un agente.

## 8. Construcciones de IA

### `prompt`

Plantilla tipada; devuelve un `Prompt`, no un `String` suelto.

```
prompt clasificar(texto: String, categorias: List<String>) {
  """
  Clasifica el texto en una de estas categorías: {categorias.join(", ")}
  Texto: {texto}
  """
}
```

### `llm.ask<T>`

```
let r = await llm.ask<Resumen>(resumir(texto))?   // Result<Resumen, Error>
```

El compilador genera el JSON Schema de `T`, pide salida estructurada al modelo y
valida la respuesta. Si no cumple el tipo, el resultado es `err(...)`.

### `tool`

Una función que además se puede exponer como herramienta MCP. Los comentarios
`///` se convierten en la descripción y los tipos en el `inputSchema`.

```
/// Busca el clima actual de una ciudad.
tool clima(ciudad: String, unidades: Unidad = .celsius) -> Result<Clima, Error>
  uses net
{ … }
```

### `server` y `client`

```
server MiServidor {
  expose clima, resumir_url
}

let gh = await mcp.connect("https://ejemplo.com/mcp")?
let r = await gh.call("buscar_issues", { "repo": "x/y" })?
```

### `agent` (reservado, v0.5)

Bucle de modelo + herramientas con límites declarados (pasos, costo, capacidades).

## 9. Pruebas

```
test "suma básica" {
  assert sumar(2, 3) == 5
}

test "resumen con modelo simulado" {
  llm.mock(Resumen { titulo: "X", puntos: [], sentimiento: .neutral })
  assert (await resumir_url("https://e.com")).is_ok()
}
```

Toda llamada a un modelo o herramienta queda registrada en una traza que
`opens test --replay` puede reproducir sin red.

## 10. Módulos

```
import http
import mi_proyecto.utilidades as util
pub fn visible() { … }     // exportado
fn interna() { … }          // privado al módulo
```

## 11. Interoperabilidad y rendimiento

```
extern "c" fn sqrt(x: Float) -> Float          // librería C/C++
extern "js" fn fetch_crudo(url: String) -> …   // solo destino web
```

Las llamadas `extern` requieren la capacidad `ffi`. `unsafe { }` habilita
operaciones de bajo nivel en el backend nativo.

## 12. Diagnósticos

Formato JSON (`opens check archivo.ops --json`), esquema `opens.diagnostics/v1`:

```json
{
  "schema": "opens.diagnostics/v1",
  "file": "main.ops",
  "ok": false,
  "diagnostics": [{
    "code": "E0106",
    "severity": "error",
    "message": "`&&` no existe en OpenS",
    "span": {"start": 12, "end": 14, "line": 3, "col": 8, "end_line": 3, "end_col": 10},
    "help": "usa `and`",
    "fix": {"start": 12, "end": 14, "replacement": " and "}
  }]
}
```

Rangos de códigos: `E01xx` léxico, `E02xx` sintaxis, `E03xx` tipos,
`E04xx` capacidades, `E05xx` IA/MCP.

## Pendiente de decidir

- Genéricos en funciones de usuario (`fn f<T>(x: T)`) y traits/interfaces.
- Modelo de memoria del backend nativo (conteo de referencias vs. regiones).
- Sintaxis de closures (`fn(x) => x * 2` es la candidata).
