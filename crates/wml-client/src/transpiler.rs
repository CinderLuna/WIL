use wml_syntax::ast::*;

pub const EMBEDDED_SIGNALS_CORE: &str = r#"
// WML Embedded Micro-Signals Core (< 2.5KB gzipped)
let currentEffect = null;

export function signal(initial) {
    let value = initial;
    const subscribers = new Set();
    return {
        get val() {
            if (currentEffect) subscribers.add(currentEffect);
            return value;
        },
        set val(next) {
            if (value !== next) {
                value = next;
                for (const sub of subscribers) sub();
            }
        }
    };
}

export function effect(fn) {
    currentEffect = fn;
    try {
        fn();
    } finally {
        currentEffect = null;
    }
}
"#;

pub struct ClientTranspiler;

impl Default for ClientTranspiler {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientTranspiler {
    pub fn new() -> Self {
        Self
    }

    pub fn transpile_island(&self, ui: &UiDecl, arena: &AstArena) -> String {
        let mut js = String::new();
        js.push_str(EMBEDDED_SIGNALS_CORE);
        js.push_str("\n\n");

        // Component declaration
        js.push_str(&format!(
            "export function mount_{}(container, props = {{}}) {{\n",
            ui.name
        ));

        // Transpile statements (signals, actions, etc.)
        for stmt in &ui.body.stmts {
            match stmt {
                Stmt::Let { name, val, .. } => {
                    let expr = arena.get_expr(*val);
                    match expr {
                        Expr::Call { callee, args } => {
                            if let Expr::Ident(callee_name) = arena.get_expr(*callee) {
                                if callee_name == "signal" {
                                    let arg_code = if let Some(arg) = args.first() {
                                        match arg {
                                            CallArg::Positional(e) => self.expr_to_js(*e, arena),
                                            CallArg::Named { val, .. } => {
                                                self.expr_to_js(*val, arena)
                                            }
                                        }
                                    } else {
                                        "undefined".to_string()
                                    };
                                    js.push_str(&format!(
                                        "    const {} = signal(props.{} ?? {});\n",
                                        name, name, arg_code
                                    ));
                                } else {
                                    js.push_str(&format!(
                                        "    let {} = {};\n",
                                        name,
                                        self.expr_to_js(*val, arena)
                                    ));
                                }
                            }
                        }
                        _ => {
                            js.push_str(&format!(
                                "    let {} = {};\n",
                                name,
                                self.expr_to_js(*val, arena)
                            ));
                        }
                    }
                }
                Stmt::Action { name, params, .. } => {
                    // Generate typed RPC action wrapper
                    let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                    let params_str = param_names.join(", ");
                    let payload_props: Vec<String> = param_names
                        .iter()
                        .map(|p| format!("{}: {}", p, p))
                        .collect();
                    let payload_str = payload_props.join(", ");

                    js.push_str(&format!(
                        r#"    async function {name}({params}) {{
        const res = await fetch(`/_wml/rpc/{name}`, {{
            method: 'POST',
            headers: {{ 'Content-Type': 'application/json' }},
            body: JSON.stringify({{ {payload} }})
        }});
        return await res.json();
    }}
"#,
                        name = name,
                        params = params_str,
                        payload = payload_str
                    ));
                }
                Stmt::Expr(expr_id) => {
                    let expr = arena.get_expr(*expr_id);
                    if let Expr::Jsx(jsx) = expr {
                        js.push_str("    // Mounting JSX root\n");
                        js.push_str(&format!(
                            "    container.innerHTML = `<{tag} class=\"counter-card\"></{tag}>`;\n",
                            tag = jsx.tag
                        ));
                    }
                }
                _ => {}
            }
        }

        js.push_str("}\n");
        js
    }

    fn expr_to_js(&self, id: ExprId, arena: &AstArena) -> String {
        match arena.get_expr(id) {
            Expr::Literal(Literal::Int(n)) => n.to_string(),
            Expr::Literal(Literal::Float(f)) => f.to_string(),
            Expr::Literal(Literal::String(s)) => format!("\"{}\"", s),
            Expr::Literal(Literal::Bool(b)) => b.to_string(),
            Expr::Literal(Literal::Null) => "null".to_string(),
            Expr::Ident(s) => s.clone(),
            Expr::Binary { op, lhs, rhs } => {
                let op_str = match op {
                    BinaryOp::Add => "+",
                    BinaryOp::Sub => "-",
                    BinaryOp::Mul => "*",
                    BinaryOp::Div => "/",
                    BinaryOp::Mod => "%",
                    BinaryOp::Eq => "===",
                    BinaryOp::NotEq => "!==",
                    BinaryOp::Lt => "<",
                    BinaryOp::LtEq => "<=",
                    BinaryOp::Gt => ">",
                    BinaryOp::GtEq => ">=",
                    BinaryOp::And => "&&",
                    BinaryOp::Or => "||",
                    BinaryOp::Coalesce => "??",
                    BinaryOp::AddAssign => "+=",
                    BinaryOp::SubAssign => "-=",
                };
                format!(
                    "({} {} {})",
                    self.expr_to_js(*lhs, arena),
                    op_str,
                    self.expr_to_js(*rhs, arena)
                )
            }
            Expr::MemberAccess { object, field } => {
                format!("{}.{}", self.expr_to_js(*object, arena), field)
            }
            Expr::Call { callee, args } => {
                let callee_js = self.expr_to_js(*callee, arena);
                let args_js: Vec<String> = args
                    .iter()
                    .map(|a| match a {
                        CallArg::Positional(e) => self.expr_to_js(*e, arena),
                        CallArg::Named { val, .. } => self.expr_to_js(*val, arena),
                    })
                    .collect();
                format!("{}({})", callee_js, args_js.join(", "))
            }
            _ => "null".to_string(),
        }
    }
}
