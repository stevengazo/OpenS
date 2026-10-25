//! Representación de tipos del chequeo (distinta del `Type` sintáctico).

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Int,
    Float,
    Bool,
    String,
    Bytes,
    Url,
    Duration,
    Unit,
    /// Tipos nominales y genéricos: `List<T>`, `Map<K, V>`, `Set<T>`,
    /// `Option<T>`, `Result<T, E>`, `Error`, o un tipo/enum de usuario.
    Named(String, Vec<Ty>),
    Ref {
        mutable: bool,
        inner: Box<Ty>,
    },
    Ptr(Box<Ty>),
    Boxed(Box<Ty>),
    Func(Vec<Ty>, Box<Ty>),
    /// No se pudo determinar. **Nunca** produce errores: es compatible con
    /// cualquier tipo, para que la inferencia parcial no dé falsos positivos.
    Unknown,
}

impl Ty {
    pub fn is_unknown(&self) -> bool {
        matches!(self, Ty::Unknown)
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Ty::Int | Ty::Float)
    }

    /// ¿Son compatibles? `Unknown` lo es con todo; los nominales comparan
    /// nombre y argumentos recursivamente.
    pub fn compatible(&self, other: &Ty) -> bool {
        use Ty::*;
        match (self, other) {
            (Unknown, _) | (_, Unknown) => true,
            (Named(a, xs), Named(b, ys)) => {
                a == b
                    && xs.len() == ys.len()
                    && xs.iter().zip(ys).all(|(x, y)| x.compatible(y))
            }
            (Ref { inner: a, .. }, Ref { inner: b, .. }) => a.compatible(b),
            (Ptr(a), Ptr(b)) => a.compatible(b),
            (Boxed(a), Boxed(b)) => a.compatible(b),
            (Func(pa, ra), Func(pb, rb)) => {
                pa.len() == pb.len()
                    && pa.iter().zip(pb).all(|(x, y)| x.compatible(y))
                    && ra.compatible(rb)
            }
            // Un `Url` se escribe como literal de string, así que son compatibles.
            (Url, String) | (String, Url) => true,
            (a, b) => a == b,
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Int => write!(f, "Int"),
            Ty::Float => write!(f, "Float"),
            Ty::Bool => write!(f, "Bool"),
            Ty::String => write!(f, "String"),
            Ty::Bytes => write!(f, "Bytes"),
            Ty::Url => write!(f, "Url"),
            Ty::Duration => write!(f, "Duration"),
            Ty::Unit => write!(f, "()"),
            Ty::Named(n, args) => {
                write!(f, "{n}")?;
                if !args.is_empty() {
                    write!(f, "<")?;
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{a}")?;
                    }
                    write!(f, ">")?;
                }
                Ok(())
            }
            Ty::Ref { mutable, inner } => {
                write!(f, "&{}{}", if *mutable { "var " } else { "" }, inner)
            }
            Ty::Ptr(inner) => write!(f, "*{inner}"),
            Ty::Boxed(inner) => write!(f, "box {inner}"),
            Ty::Func(params, ret) => {
                write!(f, "fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{p}")?;
                }
                write!(f, ") -> {ret}")
            }
            Ty::Unknown => write!(f, "?"),
        }
    }
}
