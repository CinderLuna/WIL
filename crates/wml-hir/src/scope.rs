use crate::types::Type;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub ty: Type,
    pub is_signal: bool,
    pub is_mut: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    ServerGlobal,
    ServerRoute,
    ServerPage,
    ServerFn,
    ClientUi,
    ClientAction,
    LocalBlock,
}

#[derive(Debug, Clone)]
pub struct Scope {
    pub kind: ScopeKind,
    pub symbols: HashMap<String, Symbol>,
    pub parent: Option<Box<Scope>>,
}

impl Scope {
    pub fn new(kind: ScopeKind) -> Self {
        let mut scope = Self {
            kind,
            symbols: HashMap::new(),
            parent: None,
        };

        match kind {
            ScopeKind::ServerGlobal | ScopeKind::ClientAction => {
                scope.define_builtin("db", Type::Namespace("db".to_string()));
                scope.define_builtin("crypto", Type::Namespace("crypto".to_string()));
                scope.define_builtin("jwt", Type::Namespace("jwt".to_string()));
                scope.define_builtin("req", Type::Namespace("req".to_string()));
                scope.define_builtin("res", Type::Namespace("res".to_string()));
            }
            ScopeKind::ClientUi => {
                scope.define_builtin(
                    "signal",
                    Type::Function {
                        params: vec![Type::Any],
                        ret: Box::new(Type::Signal(Box::new(Type::Any))),
                    },
                );
            }
            _ => {}
        }

        scope
    }

    pub fn enter_child(&self, kind: ScopeKind) -> Self {
        let mut child = Self::new(kind);
        child.parent = Some(Box::new(self.clone()));
        child
    }

    pub fn define(&mut self, name: String, ty: Type, is_signal: bool) {
        self.symbols.insert(
            name.clone(),
            Symbol {
                name,
                ty,
                is_signal,
                is_mut: false,
            },
        );
    }

    pub fn define_builtin(&mut self, name: &str, ty: Type) {
        self.symbols.insert(
            name.to_string(),
            Symbol {
                name: name.to_string(),
                ty,
                is_signal: false,
                is_mut: false,
            },
        );
    }

    pub fn lookup(&self, name: &str) -> Option<&Symbol> {
        if let Some(sym) = self.symbols.get(name) {
            return Some(sym);
        }
        if let Some(parent) = &self.parent {
            return parent.lookup(name);
        }
        None
    }

    pub fn is_client_island(&self) -> bool {
        if self.kind == ScopeKind::ClientAction {
            return false; // Action executes on server
        }
        if self.kind == ScopeKind::ClientUi {
            return true;
        }
        if let Some(parent) = &self.parent {
            parent.is_client_island()
        } else {
            false
        }
    }

    pub fn all_accessible_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.symbols.keys().cloned().collect();
        if let Some(parent) = &self.parent {
            names.extend(parent.all_accessible_names());
        }
        if self.is_client_island() {
            names
                .retain(|n| !matches!(n.as_str(), "db" | "crypto" | "jwt" | "req" | "res" | "env"));
        }
        names.sort();
        names.dedup();
        names
    }

    pub fn find_closest_match(&self, name: &str) -> Option<String> {
        let mut best: Option<(usize, String)> = None;
        for candidate in self.all_accessible_names() {
            let dist = levenshtein(name, &candidate);
            if dist <= 2 {
                match best {
                    Some((b_dist, _)) if dist < b_dist => best = Some((dist, candidate)),
                    None => best = Some((dist, candidate)),
                    _ => {}
                }
            }
        }
        best.map(|(_, name)| name)
    }
}

pub fn levenshtein(a: &str, b: &str) -> usize {
    let a_len = a.chars().count();
    let b_len = b.chars().count();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut row: Vec<usize> = (0..=b_len).collect();

    for (i, ca) in a.chars().enumerate() {
        let mut prev = i;
        row[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cur = row[j + 1];
            let cost = if ca == cb { 0 } else { 1 };
            row[j + 1] = (prev + cost).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }

    row[b_len]
}
