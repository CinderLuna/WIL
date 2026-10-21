use serde::{Deserialize, Serialize};
use std::fmt;
use wml_syntax::ast::{FieldDef, TypeRef};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Type {
    Int,
    Float,
    Str,
    Bool,
    Void,
    Any,
    Array(Box<Type>),
    Optional(Box<Type>),
    Record(Vec<(String, Type)>),
    Custom(String),
    Jsx,
    Signal(Box<Type>),
    Function { params: Vec<Type>, ret: Box<Type> },
    Namespace(String),
}

impl Type {
    pub fn from_type_ref(tr: &TypeRef) -> Self {
        match tr {
            TypeRef::Int => Type::Int,
            TypeRef::Float => Type::Float,
            TypeRef::Str => Type::Str,
            TypeRef::Bool => Type::Bool,
            TypeRef::Void => Type::Void,
            TypeRef::Any => Type::Any,
            TypeRef::Custom(name) => Type::Custom(name.clone()),
            TypeRef::Array(inner) => Type::Array(Box::new(Type::from_type_ref(inner))),
            TypeRef::Optional(inner) => Type::Optional(Box::new(Type::from_type_ref(inner))),
            TypeRef::Record(fields) => {
                let f = fields
                    .iter()
                    .map(|FieldDef { name, ty }| (name.clone(), Type::from_type_ref(ty)))
                    .collect();
                Type::Record(f)
            }
        }
    }

    pub fn unwrap_optional(&self) -> Option<Type> {
        match self {
            Type::Optional(inner) => Some((**inner).clone()),
            _ => None,
        }
    }

    pub fn is_optional(&self) -> bool {
        matches!(self, Type::Optional(_))
    }

    pub fn is_assignable_to(&self, target: &Type) -> bool {
        if self == target {
            return true;
        }

        if matches!(self, Type::Any) || matches!(target, Type::Any) {
            return true;
        }

        // Covariant Optional: Optional<T> assignable to Optional<U> if T assignable to U
        if let (Type::Optional(s_inner), Type::Optional(t_inner)) = (self, target) {
            return s_inner.is_assignable_to(t_inner);
        }

        // T is assignable to Optional<T>
        if let Type::Optional(target_inner) = target {
            if self.is_assignable_to(target_inner) {
                return true;
            }
        }

        // Array<T> assignable to Array<U> if T assignable to U
        if let (Type::Array(s_inner), Type::Array(t_inner)) = (self, target) {
            return s_inner.is_assignable_to(t_inner);
        }

        // Record subtyping / structural matching
        if let (Type::Record(s_fields), Type::Record(t_fields)) = (self, target) {
            for (t_name, t_ty) in t_fields {
                let found = s_fields.iter().find(|(s_name, _)| s_name == t_name);
                match found {
                    Some((_, s_ty)) if s_ty.is_assignable_to(t_ty) => continue,
                    _ => return false,
                }
            }
            return true;
        }

        // Signal<T> unwraps to T in expressions
        if let Type::Signal(inner) = self {
            if inner.is_assignable_to(target) {
                return true;
            }
        }

        false
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int => write!(f, "int"),
            Type::Float => write!(f, "float"),
            Type::Str => write!(f, "str"),
            Type::Bool => write!(f, "bool"),
            Type::Void => write!(f, "void"),
            Type::Any => write!(f, "any"),
            Type::Array(inner) => write!(f, "[{}]", inner),
            Type::Optional(inner) => write!(f, "{}?", inner),
            Type::Record(fields) => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(name, ty)| format!("{}: {}", name, ty))
                    .collect();
                write!(f, "{{{}}}", parts.join(", "))
            }
            Type::Custom(name) => write!(f, "{}", name),
            Type::Jsx => write!(f, "jsx"),
            Type::Signal(inner) => write!(f, "signal<{}>", inner),
            Type::Function { params, ret } => {
                let param_strs: Vec<String> = params.iter().map(|p| p.to_string()).collect();
                write!(f, "fn({}) -> {}", param_strs.join(", "), ret)
            }
            Type::Namespace(name) => write!(f, "module<{}>", name),
        }
    }
}
