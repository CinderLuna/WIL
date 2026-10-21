use std::collections::HashSet;
use wml_syntax::ast::*;

pub const EMBEDDED_SIGNALS_CORE: &str = r#"// WML Embedded Micro-Signals Core (< 2.5KB gzipped)
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
    const run = () => {
        const prev = currentEffect;
        currentEffect = run;
        try {
            fn();
        } finally {
            currentEffect = prev;
        }
    };
    run();
}
"#;

/// Escapes a string for safe inclusion in a JavaScript string literal or template.
pub fn escape_js_string(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\'' => escaped.push_str("\\'"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '<' => escaped.push_str("\\x3C"),
            '>' => escaped.push_str("\\x3E"),
            '\u{2028}' => escaped.push_str("\\u2028"),
            '\u{2029}' => escaped.push_str("\\u2029"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write;
                let _ = write!(escaped, "\\x{:02x}", c as u32);
            }
            c => escaped.push(c),
        }
    }
    escaped
}

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

    pub fn transpile_island(&self, ui: &UiDecl, arena: &AstArena) -> Result<String, String> {
        let mut js = String::new();
        js.push_str(EMBEDDED_SIGNALS_CORE);
        js.push_str("\n\n");
        js.push_str(&self.transpile_island_body(ui, arena)?);
        Ok(js)
    }

    pub fn bundle_islands(&self, uis: &[&UiDecl], arena: &AstArena) -> Result<String, String> {
        let mut bundle = String::new();
        bundle.push_str(EMBEDDED_SIGNALS_CORE);
        bundle.push_str("\n\n");

        for ui in uis {
            let island_code = self.transpile_island_body(ui, arena)?;
            bundle.push_str(&island_code);
            bundle.push_str("\n\n");
        }

        bundle.push_str(
            r#"// Auto-mount all declared client islands
export function auto_mount() {
    if (typeof document === 'undefined') return;
    document.querySelectorAll('[data-wml-island]').forEach(container => {
        const islandName = container.getAttribute('data-wml-island');
        let props = {};
        try {
            const rawProps = container.getAttribute('data-wml-props');
            if (rawProps) props = JSON.parse(rawProps);
        } catch (_) {}
"#,
        );

        for ui in uis {
            bundle.push_str(&format!(
                r#"        if (islandName === '{name}') {{
            container.innerHTML = '';
            mount_{name}(container, props);
        }}
"#,
                name = ui.name
            ));
        }

        bundle.push_str(
            r#"    });
}

if (typeof document !== 'undefined') {
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', auto_mount);
    } else {
        auto_mount();
    }
}
"#,
        );

        Ok(bundle)
    }

    pub fn transpile_island_body(&self, ui: &UiDecl, arena: &AstArena) -> Result<String, String> {
        let mut js = String::new();
        js.push_str(&format!(
            "export function mount_{}(container, props = {{}}) {{\n",
            ui.name
        ));

        // Unpack island parameters from props
        for param in &ui.params {
            js.push_str(&format!(
                "    const {name} = props.{name} !== undefined ? props.{name} : undefined;\n",
                name = param.name
            ));
        }

        let mut var_counter = 0;
        let mut signals = HashSet::new();

        // Pass 1: Collect signal declarations
        for stmt in &ui.body.stmts {
            if let Stmt::Let { name, val, .. } = stmt {
                let expr = arena.get_expr(*val);
                if let Expr::Call { callee, .. } = expr {
                    if let Expr::Ident(callee_name) = arena.get_expr(*callee) {
                        if callee_name == "signal" {
                            signals.insert(name.clone());
                        }
                    }
                }
            }
        }

        // Pass 2: Transpile statements
        for stmt in &ui.body.stmts {
            match stmt {
                Stmt::Let { name, val, .. } => {
                    let expr = arena.get_expr(*val);
                    if signals.contains(name) {
                        let arg_code = if let Expr::Call { args, .. } = expr {
                            if let Some(arg) = args.first() {
                                match arg {
                                    CallArg::Positional(e) => {
                                        self.expr_to_js(*e, arena, &signals)?
                                    }
                                    CallArg::Named { val, .. } => {
                                        self.expr_to_js(*val, arena, &signals)?
                                    }
                                }
                            } else {
                                "undefined".to_string()
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
                            self.expr_to_js(*val, arena, &signals)?
                        ));
                    }
                }
                Stmt::Action { name, params, .. } => {
                    let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                    let params_str = param_names.join(", ");
                    let payload_props: Vec<String> = param_names
                        .iter()
                        .map(|p| format!("{}: {}", p, p))
                        .collect();
                    let payload_str = payload_props.join(", ");

                    js.push_str(&format!(
                        r#"    async function {name}({params}) {{
        const res = await fetch(`/_wml/rpc/{ui_name}/{name}`, {{
            method: 'POST',
            headers: {{ 'Content-Type': 'application/json' }},
            body: JSON.stringify({{ {payload} }})
        }});
        return await res.json();
    }}
"#,
                        ui_name = ui.name,
                        name = name,
                        params = params_str,
                        payload = payload_str
                    ));
                }
                Stmt::Expr(expr_id) => {
                    let expr = arena.get_expr(*expr_id);
                    if let Expr::Jsx(jsx) = expr {
                        let (root_var, jsx_code) =
                            self.transpile_jsx(jsx, arena, &signals, &mut var_counter)?;
                        js.push_str(&jsx_code);
                        js.push_str(&format!("    container.appendChild({});\n", root_var));
                    } else {
                        let code = self.expr_to_js(*expr_id, arena, &signals)?;
                        js.push_str(&format!("    {};\n", code));
                    }
                }
                Stmt::Return(ret_expr) => {
                    if let Some(e) = ret_expr {
                        js.push_str(&format!(
                            "    return {};\n",
                            self.expr_to_js(*e, arena, &signals)?
                        ));
                    } else {
                        js.push_str("    return;\n");
                    }
                }
                _ => {}
            }
        }

        js.push_str("}\n");
        Ok(js)
    }

    fn transpile_jsx(
        &self,
        elem: &JsxElement,
        arena: &AstArena,
        signals: &HashSet<String>,
        var_gen: &mut usize,
    ) -> Result<(String, String), String> {
        let el_var = format!("_el{}", *var_gen);
        *var_gen += 1;

        let mut code = String::new();
        code.push_str(&format!(
            "    const {} = document.createElement(\"{}\");\n",
            el_var,
            escape_js_string(&elem.tag)
        ));

        // Attributes
        for attr in &elem.attrs {
            let (is_event, event_name) = if let Some(stripped) = attr.name.strip_prefix('@') {
                (true, stripped.to_ascii_lowercase())
            } else if attr.name.len() > 2 && attr.name.starts_with("on") {
                let rest = &attr.name[2..];
                let is_uppercase_jsx = rest.chars().next().is_some_and(|c| c.is_ascii_uppercase());
                let is_known_lowercase_dom_event = matches!(
                    rest,
                    "click"
                        | "change"
                        | "input"
                        | "submit"
                        | "keydown"
                        | "keyup"
                        | "keypress"
                        | "blur"
                        | "focus"
                        | "scroll"
                        | "reset"
                        | "load"
                        | "error"
                        | "mouseenter"
                        | "mouseleave"
                        | "mousedown"
                        | "mouseup"
                        | "mousemove"
                );
                if is_uppercase_jsx
                    || is_known_lowercase_dom_event
                    || matches!(attr.value, Some(JsxAttrValue::Block(_)))
                {
                    (true, rest.to_ascii_lowercase())
                } else {
                    (false, String::new())
                }
            } else {
                (false, String::new())
            };

            if is_event {
                match &attr.value {
                    Some(JsxAttrValue::Block(block)) => {
                        code.push_str(&format!(
                            "    {}.addEventListener(\"{}\", (event) => {{\n",
                            el_var,
                            escape_js_string(&event_name)
                        ));
                        for stmt in &block.stmts {
                            match stmt {
                                Stmt::Let { name, val, .. } => {
                                    let s = self.expr_to_js(*val, arena, signals)?;
                                    code.push_str(&format!("        let {} = {};\n", name, s));
                                }
                                Stmt::Expr(e) => {
                                    let s = self.expr_to_js(*e, arena, signals)?;
                                    code.push_str(&format!("        {};\n", s));
                                }
                                Stmt::Return(r) => {
                                    if let Some(e) = r {
                                        let s = self.expr_to_js(*e, arena, signals)?;
                                        code.push_str(&format!("        return {};\n", s));
                                    } else {
                                        code.push_str("        return;\n");
                                    }
                                }
                                unsupported => {
                                    return Err(format!(
                                        "Unsupported statement in event handler block: {:?}",
                                        unsupported
                                    ));
                                }
                            }
                        }
                        code.push_str("    });\n");
                    }
                    Some(JsxAttrValue::Expr(e)) => {
                        let handler = self.expr_to_js(*e, arena, signals)?;
                        code.push_str(&format!(
                            "    {}.addEventListener(\"{}\", {});\n",
                            el_var,
                            escape_js_string(&event_name),
                            handler
                        ));
                    }
                    Some(JsxAttrValue::String(_)) => {
                        return Err(format!(
                            "Event handler '{}' cannot have a string literal value. Use an expression or block closure.",
                            attr.name
                        ));
                    }
                    None => {
                        return Err(format!(
                            "Event handler '{}' must specify a callback expression or block closure.",
                            attr.name
                        ));
                    }
                }
            } else {
                match &attr.value {
                    Some(JsxAttrValue::String(s)) => {
                        code.push_str(&format!(
                            "    {}.setAttribute(\"{}\", \"{}\");\n",
                            el_var,
                            escape_js_string(&attr.name),
                            escape_js_string(s)
                        ));
                    }
                    Some(JsxAttrValue::Expr(e)) => {
                        let val_expr = self.expr_to_js(*e, arena, signals)?;
                        code.push_str(&format!(
                            "    effect(() => {{ {}.setAttribute(\"{}\", {}); }});\n",
                            el_var,
                            escape_js_string(&attr.name),
                            val_expr
                        ));
                    }
                    None => {
                        code.push_str(&format!(
                            "    {}.setAttribute(\"{}\", \"\");\n",
                            el_var,
                            escape_js_string(&attr.name)
                        ));
                    }
                    Some(JsxAttrValue::Block(_)) => {
                        return Err(format!(
                            "Attribute '{}' cannot have a block value. Only event handlers accept block closures.",
                            attr.name
                        ));
                    }
                }
            }
        }

        // Children
        for child in &elem.children {
            match child {
                JsxChild::Text(t) => {
                    code.push_str(&format!(
                        "    {}.appendChild(document.createTextNode(\"{}\"));\n",
                        el_var,
                        escape_js_string(t)
                    ));
                }
                JsxChild::Element(child_elem) => {
                    let (child_var, child_code) =
                        self.transpile_jsx(child_elem, arena, signals, var_gen)?;
                    code.push_str(&child_code);
                    code.push_str(&format!("    {}.appendChild({});\n", el_var, child_var));
                }
                JsxChild::Expr(expr_id) => {
                    let node_var = format!("_t{}", *var_gen);
                    *var_gen += 1;
                    let val_expr = self.expr_to_js(*expr_id, arena, signals)?;
                    code.push_str(&format!(
                        "    const {} = document.createTextNode(\"\");\n",
                        node_var
                    ));
                    code.push_str(&format!("    {}.appendChild({});\n", el_var, node_var));
                    code.push_str(&format!(
                        "    effect(() => {{ {}.textContent = {}; }});\n",
                        node_var, val_expr
                    ));
                }
            }
        }

        Ok((el_var, code))
    }

    pub fn expr_to_js(
        &self,
        id: ExprId,
        arena: &AstArena,
        signals: &HashSet<String>,
    ) -> Result<String, String> {
        let expr = arena.get_expr(id);
        match expr {
            Expr::Literal(Literal::Int(n)) => Ok(n.to_string()),
            Expr::Literal(Literal::Float(f)) => Ok(f.to_string()),
            Expr::Literal(Literal::String(s)) => Ok(format!("\"{}\"", escape_js_string(s))),
            Expr::Literal(Literal::Bool(b)) => Ok(b.to_string()),
            Expr::Literal(Literal::Null) => Ok("null".to_string()),
            Expr::Ident(s) => {
                if signals.contains(s) {
                    Ok(format!("{}.val", s))
                } else {
                    Ok(s.clone())
                }
            }
            Expr::Unary { op, expr } => {
                let op_str = match op {
                    UnaryOp::Not => "!",
                    UnaryOp::Neg => "-",
                };
                let inner = self.expr_to_js(*expr, arena, signals)?;
                Ok(format!("({}{})", op_str, inner))
            }
            Expr::Binary { op, lhs, rhs } => {
                let op_str = match op {
                    BinaryOp::Assign => "=",
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

                // When assigning to a signal (e.g. count += 1 or count = 5), target `.val`
                if matches!(
                    op,
                    BinaryOp::Assign | BinaryOp::AddAssign | BinaryOp::SubAssign
                ) {
                    if let Expr::Ident(name) = arena.get_expr(*lhs) {
                        if signals.contains(name) {
                            let r = self.expr_to_js(*rhs, arena, signals)?;
                            return Ok(format!("({}.val {} {})", name, op_str, r));
                        }
                    }
                }

                let l = self.expr_to_js(*lhs, arena, signals)?;
                let r = self.expr_to_js(*rhs, arena, signals)?;
                Ok(format!("({} {} {})", l, op_str, r))
            }
            Expr::MemberAccess { object, field } => {
                let obj = self.expr_to_js(*object, arena, signals)?;
                Ok(format!("{}.{}", obj, field))
            }
            Expr::Call { callee, args } => {
                let callee_js = self.expr_to_js(*callee, arena, signals)?;
                let args_js: Result<Vec<String>, String> = args
                    .iter()
                    .map(|a| match a {
                        CallArg::Positional(e) => self.expr_to_js(*e, arena, signals),
                        CallArg::Named { val, .. } => self.expr_to_js(*val, arena, signals),
                    })
                    .collect();
                Ok(format!("{}({})", callee_js, args_js?.join(", ")))
            }
            Expr::ArrayLit(items) => {
                let items_js: Result<Vec<String>, String> = items
                    .iter()
                    .map(|e| self.expr_to_js(*e, arena, signals))
                    .collect();
                Ok(format!("[{}]", items_js?.join(", ")))
            }
            Expr::ObjectLit(fields) => {
                let fields_js: Result<Vec<String>, String> = fields
                    .iter()
                    .map(|f| match &f.val {
                        Some(v) => {
                            let val_str = self.expr_to_js(*v, arena, signals)?;
                            Ok(format!("{}: {}", f.name, val_str))
                        }
                        None => Ok(f.name.clone()),
                    })
                    .collect();
                Ok(format!("{{ {} }}", fields_js?.join(", ")))
            }
            unsupported => Err(format!(
                "Unsupported client expression variant: {:?}",
                unsupported
            )),
        }
    }
}
