use crate::ir::*;
use std::collections::HashMap;
use wml_hir::hir::*;
use wml_hir::types::Type;

pub struct MirLowerer {
    next_val: usize,
    next_block: usize,
    current_blocks: Vec<BasicBlock>,
    current_block_idx: usize,
    locals: HashMap<String, ValueId>,
    arena_ptr: Option<ValueId>,
    req_ptr: Option<ValueId>,
    res_ptr: Option<ValueId>,
}

impl Default for MirLowerer {
    fn default() -> Self {
        Self::new()
    }
}

impl MirLowerer {
    pub fn new() -> Self {
        Self {
            next_val: 0,
            next_block: 0,
            current_blocks: Vec::new(),
            current_block_idx: 0,
            locals: HashMap::new(),
            arena_ptr: None,
            req_ptr: None,
            res_ptr: None,
        }
    }

    pub fn lower_program(mut self, program: &HirProgram) -> MirModule {
        let mut module = MirModule::new("wml_module");

        // 1. Lower routes into route handlers and route table
        for route in &program.routes {
            let (mir_fn, mir_route) = self.lower_route(route);
            module.functions.push(mir_fn);
            module.routes.push(mir_route);
        }

        // 2. Lower pages (server-rendered pages compile as GET route handlers)
        for page in &program.pages {
            let (mir_fn, mir_route) = self.lower_page(page);
            module.functions.push(mir_fn);
            module.routes.push(mir_route);
        }

        // 3. Lower standard functions
        for f in &program.fns {
            let mir_fn = self.lower_fn(f);
            module.functions.push(mir_fn);
        }

        // 4. Lower client UI island actions (callable via server RPC)
        for ui in &program.uis {
            for action in &ui.actions {
                let (mir_fn, mir_routes) = self.lower_action(action, &ui.name);
                module.functions.push(mir_fn);
                module.routes.extend(mir_routes);
            }
        }

        module
    }

    fn new_val(&mut self) -> ValueId {
        let v = ValueId(self.next_val);
        self.next_val += 1;
        v
    }

    fn new_block(&mut self) -> BlockId {
        let b = BlockId(self.next_block);
        self.next_block += 1;
        self.current_blocks.push(BasicBlock::new(b));
        b
    }

    fn emit_inst(&mut self, inst: MirInst, ty: MirType) -> ValueId {
        let val = self.new_val();
        self.current_blocks[self.current_block_idx]
            .instructions
            .push((val, inst, ty));
        val
    }

    fn set_terminator(&mut self, term: MirTerminator) {
        self.current_blocks[self.current_block_idx].terminator = term;
    }

    fn switch_to_block(&mut self, id: BlockId) {
        if let Some(pos) = self.current_blocks.iter().position(|b| b.id == id) {
            self.current_block_idx = pos;
        }
    }

    fn lower_route(&mut self, route: &HirRouteDecl) -> (MirFunction, MirRoute) {
        self.reset_fn_state();

        let slug = slugify_path(&route.path);
        let fn_name = format!("wml_route_{}", slug);

        let entry_id = self.new_block();
        self.current_block_idx = 0;

        let v_arena = self.new_val();
        let v_req = self.new_val();
        let v_res = self.new_val();

        self.arena_ptr = Some(v_arena);
        self.req_ptr = Some(v_req);
        self.res_ptr = Some(v_res);

        self.current_blocks[0].params = vec![
            (v_arena, MirType::Ptr),
            (v_req, MirType::Ptr),
            (v_res, MirType::Ptr),
        ];

        // Route payload unpacking if post/put parameters are defined
        if let Some(Type::Record(fields)) = &route.payload_type {
            for (field_name, field_ty) in fields {
                let mir_ty = Self::type_to_mir(field_ty);
                // Extract payload field from request
                let v_field_name = self.emit_inst(
                    MirInst::Const(MirConstant::Str(field_name.clone())),
                    MirType::Slice,
                );
                let v_val = self.emit_inst(
                    MirInst::CallRuntime {
                        intrinsic: "wml_req_get_param".to_string(),
                        args: vec![v_arena, v_req, v_field_name],
                    },
                    mir_ty,
                );
                self.locals.insert(field_name.clone(), v_val);
            }
        }

        // Lower body statements
        for stmt in &route.body {
            self.lower_stmt(stmt, true);
        }

        // Default return 200 OK if not already terminated
        if matches!(
            self.current_blocks[self.current_block_idx].terminator,
            MirTerminator::Unreachable
        ) {
            let zero = self.emit_inst(MirInst::Const(MirConstant::Int(0)), MirType::I64);
            self.set_terminator(MirTerminator::Return(Some(zero)));
        }

        let mir_fn = MirFunction {
            name: fn_name.clone(),
            params: vec![
                (v_arena, MirType::Ptr),
                (v_req, MirType::Ptr),
                (v_res, MirType::Ptr),
            ],
            return_ty: MirType::I64,
            blocks: std::mem::take(&mut self.current_blocks),
            entry: entry_id,
            is_route_handler: true,
        };

        let method_str = route
            .method
            .as_ref()
            .map(|m| format!("{:?}", m).to_uppercase())
            .unwrap_or_else(|| "GET".to_string());

        let mir_route = MirRoute {
            method: method_str,
            path: route.path.clone(),
            handler_fn: fn_name,
            auth: route.auth,
            rate_limit: route.rate_limit,
        };

        (mir_fn, mir_route)
    }

    fn lower_page(&mut self, page: &HirPageDecl) -> (MirFunction, MirRoute) {
        self.reset_fn_state();

        let slug = slugify_path(&page.path);
        let fn_name = format!("wml_page_{}", slug);

        let entry_id = self.new_block();
        self.current_block_idx = 0;

        let v_arena = self.new_val();
        let v_req = self.new_val();
        let v_res = self.new_val();

        self.arena_ptr = Some(v_arena);
        self.req_ptr = Some(v_req);
        self.res_ptr = Some(v_res);

        self.current_blocks[0].params = vec![
            (v_arena, MirType::Ptr),
            (v_req, MirType::Ptr),
            (v_res, MirType::Ptr),
        ];

        for stmt in &page.body {
            self.lower_stmt(stmt, true);
        }

        if matches!(
            self.current_blocks[self.current_block_idx].terminator,
            MirTerminator::Unreachable
        ) {
            let zero = self.emit_inst(MirInst::Const(MirConstant::Int(0)), MirType::I64);
            self.set_terminator(MirTerminator::Return(Some(zero)));
        }

        let mir_fn = MirFunction {
            name: fn_name.clone(),
            params: vec![
                (v_arena, MirType::Ptr),
                (v_req, MirType::Ptr),
                (v_res, MirType::Ptr),
            ],
            return_ty: MirType::I64,
            blocks: std::mem::take(&mut self.current_blocks),
            entry: entry_id,
            is_route_handler: true,
        };

        let mir_route = MirRoute {
            method: "GET".to_string(),
            path: page.path.clone(),
            handler_fn: fn_name,
            auth: page.auth,
            rate_limit: page.rate_limit,
        };

        (mir_fn, mir_route)
    }

    fn lower_fn(&mut self, f: &HirFnDecl) -> MirFunction {
        self.reset_fn_state();

        let entry_id = self.new_block();
        self.current_block_idx = 0;

        let mut params = Vec::new();
        for p in &f.params {
            let v = self.new_val();
            let ty = Self::type_to_mir(&p.ty);
            params.push((v, ty));
            self.locals.insert(p.name.clone(), v);
        }
        self.current_blocks[0].params = params.clone();

        for stmt in &f.body {
            self.lower_stmt(stmt, false);
        }

        if matches!(
            self.current_blocks[self.current_block_idx].terminator,
            MirTerminator::Unreachable
        ) {
            self.set_terminator(MirTerminator::Return(None));
        }

        MirFunction {
            name: f.name.clone(),
            params,
            return_ty: Self::type_to_mir(&f.return_type),
            blocks: std::mem::take(&mut self.current_blocks),
            entry: entry_id,
            is_route_handler: false,
        }
    }

    fn lower_action(
        &mut self,
        action: &HirActionDecl,
        ui_name: &str,
    ) -> (MirFunction, Vec<MirRoute>) {
        self.reset_fn_state();

        let fn_name = format!("wml_action_{}_{}", ui_name, action.name);
        let entry_id = self.new_block();
        self.current_block_idx = 0;

        let v_arena = self.new_val();
        let v_req = self.new_val();
        let v_res = self.new_val();

        self.arena_ptr = Some(v_arena);
        self.req_ptr = Some(v_req);
        self.res_ptr = Some(v_res);

        self.current_blocks[0].params = vec![
            (v_arena, MirType::Ptr),
            (v_req, MirType::Ptr),
            (v_res, MirType::Ptr),
        ];

        // Unpack action parameters from JSON request body
        for p in &action.params {
            let mir_ty = Self::type_to_mir(&p.ty);
            let v_param_name = self.emit_inst(
                MirInst::Const(MirConstant::Str(p.name.clone())),
                MirType::Slice,
            );
            let v_val = self.emit_inst(
                MirInst::CallRuntime {
                    intrinsic: "wml_req_get_param".to_string(),
                    args: vec![v_arena, v_req, v_param_name],
                },
                mir_ty,
            );
            self.locals.insert(p.name.clone(), v_val);
        }

        for stmt in &action.body {
            self.lower_stmt(stmt, true);
        }

        if matches!(
            self.current_blocks[self.current_block_idx].terminator,
            MirTerminator::Unreachable
        ) {
            let zero = self.emit_inst(MirInst::Const(MirConstant::Int(0)), MirType::I64);
            self.set_terminator(MirTerminator::Return(Some(zero)));
        }

        let mir_fn = MirFunction {
            name: fn_name.clone(),
            params: vec![
                (v_arena, MirType::Ptr),
                (v_req, MirType::Ptr),
                (v_res, MirType::Ptr),
            ],
            return_ty: MirType::I64,
            blocks: std::mem::take(&mut self.current_blocks),
            entry: entry_id,
            is_route_handler: true,
        };

        let routes = vec![
            MirRoute {
                method: "POST".to_string(),
                path: format!("/_wml/rpc/{}", action.name),
                handler_fn: fn_name.clone(),
                auth: false,
                rate_limit: None,
            },
            MirRoute {
                method: "POST".to_string(),
                path: format!("/_wml/rpc/{}/{}", ui_name, action.name),
                handler_fn: fn_name,
                auth: false,
                rate_limit: None,
            },
        ];

        (mir_fn, routes)
    }

    fn render_jsx_element_to_html(elem: &HirJsxElement) -> String {
        let mut html = String::new();
        let is_island = elem
            .tag
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase());

        if is_island {
            html.push_str(&format!(
                r#"<div data-wml-island="{}""#,
                escape_html(&elem.tag)
            ));
            let mut props = Vec::new();
            for (attr_name, attr_val) in &elem.attrs {
                if let Some(val_expr) = attr_val {
                    match &val_expr.kind {
                        HirExpr::Literal(Literal::Int(n)) => {
                            props.push(format!("\"{}\":{}", escape_html(attr_name), n));
                        }
                        HirExpr::Literal(Literal::Float(f)) => {
                            props.push(format!("\"{}\":{}", escape_html(attr_name), f));
                        }
                        HirExpr::Literal(Literal::Bool(b)) => {
                            props.push(format!("\"{}\":{}", escape_html(attr_name), b));
                        }
                        HirExpr::Literal(Literal::String(s)) => {
                            props.push(format!(
                                "\"{}\":\"{}\"",
                                escape_html(attr_name),
                                s.replace('\\', "\\\\").replace('"', "\\\"")
                            ));
                        }
                        _ => props.push(format!("\"{}\":0", escape_html(attr_name))),
                    }
                }
            }
            let props_json = format!("{{{}}}", props.join(","));
            let escaped_props = props_json.replace('&', "&amp;").replace('\'', "&#39;");
            html.push_str(&format!(r#" data-wml-props='{}'>"#, escaped_props));

            for child in &elem.children {
                match child {
                    HirJsxChild::Element(child_elem) => {
                        html.push_str(&Self::render_jsx_element_to_html(child_elem));
                    }
                    HirJsxChild::Text(text) => {
                        html.push_str(&escape_html(text));
                    }
                    HirJsxChild::Expr(expr) => match &expr.kind {
                        HirExpr::Literal(Literal::String(s)) => html.push_str(&escape_html(s)),
                        HirExpr::Literal(Literal::Int(n)) => html.push_str(&n.to_string()),
                        HirExpr::Literal(Literal::Float(f)) => html.push_str(&f.to_string()),
                        HirExpr::Literal(Literal::Bool(b)) => html.push_str(&b.to_string()),
                        _ => {}
                    },
                }
            }
            html.push_str("</div>");
        } else {
            html.push('<');
            html.push_str(&elem.tag);
            for (attr_name, attr_val) in &elem.attrs {
                html.push(' ');
                html.push_str(attr_name);
                if let Some(val) = attr_val {
                    match &val.kind {
                        HirExpr::Literal(Literal::String(s)) => {
                            html.push_str(&format!(r#"="{}""#, escape_html(s)));
                        }
                        HirExpr::Literal(Literal::Int(n)) => {
                            html.push_str(&format!(r#"="{}""#, n));
                        }
                        HirExpr::Literal(Literal::Bool(b)) => {
                            html.push_str(&format!(r#"="{}""#, b));
                        }
                        _ => {}
                    }
                }
            }
            html.push('>');

            for child in &elem.children {
                match child {
                    HirJsxChild::Element(child_elem) => {
                        html.push_str(&Self::render_jsx_element_to_html(child_elem));
                    }
                    HirJsxChild::Text(text) => {
                        html.push_str(&escape_html(text));
                    }
                    HirJsxChild::Expr(expr) => match &expr.kind {
                        HirExpr::Literal(Literal::String(s)) => html.push_str(&escape_html(s)),
                        HirExpr::Literal(Literal::Int(n)) => html.push_str(&n.to_string()),
                        HirExpr::Literal(Literal::Float(f)) => html.push_str(&f.to_string()),
                        HirExpr::Literal(Literal::Bool(b)) => html.push_str(&b.to_string()),
                        _ => {}
                    },
                }
            }

            html.push_str(&format!("</{}>", elem.tag));
        }

        html
    }

    fn lower_stmt(&mut self, stmt: &HirStmt, in_route: bool) {
        match stmt {
            HirStmt::Let { name, val, .. } => {
                let v = self.lower_expr(val);
                self.locals.insert(name.clone(), v);
            }
            HirStmt::Expr(expr) => {
                let _v = self.lower_expr(expr);
                if in_route {
                    if let HirExpr::Jsx(elem) = &expr.kind {
                        if let Some(res) = self.res_ptr {
                            let doc_html = Self::render_jsx_element_to_html(elem);
                            let script_tag = if doc_html.contains("data-wml-island") {
                                r#"<script type="module" src="/_wml/assets/islands.js"></script>"#
                            } else {
                                ""
                            };
                            let full_doc = format!(
                                "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\"><title>WML App</title></head><body>{}{}</body></html>",
                                doc_html, script_tag
                            );
                            let v_doc = self.emit_inst(
                                MirInst::Const(MirConstant::Str(full_doc)),
                                MirType::Slice,
                            );
                            let ptr = self.emit_inst(MirInst::SlicePtr(v_doc), MirType::Ptr);
                            let len = self.emit_inst(MirInst::SliceLen(v_doc), MirType::I64);
                            self.emit_inst(
                                MirInst::CallRuntime {
                                    intrinsic: "wml_res_set_body".to_string(),
                                    args: vec![res, ptr, len],
                                },
                                MirType::Unit,
                            );
                            let ct_str = self.emit_inst(
                                MirInst::Const(MirConstant::Str(
                                    "text/html; charset=utf-8".to_string(),
                                )),
                                MirType::Slice,
                            );
                            let ct_ptr = self.emit_inst(MirInst::SlicePtr(ct_str), MirType::Ptr);
                            let ct_len = self.emit_inst(MirInst::SliceLen(ct_str), MirType::I64);
                            self.emit_inst(
                                MirInst::CallRuntime {
                                    intrinsic: "wml_res_set_content_type".to_string(),
                                    args: vec![res, ct_ptr, ct_len],
                                },
                                MirType::Unit,
                            );
                        }
                    }
                }
            }
            HirStmt::Return { val, .. } => {
                let ret_val = val.as_ref().map(|e| self.lower_expr(e));
                if in_route {
                    if let Some(rv) = ret_val {
                        if let Some(res) = self.res_ptr {
                            let val_ty = val.as_ref().map(|e| Self::type_to_mir(&e.ty));
                            match val_ty {
                                Some(MirType::Slice) => {
                                    let ptr = self.emit_inst(MirInst::SlicePtr(rv), MirType::Ptr);
                                    let len = self.emit_inst(MirInst::SliceLen(rv), MirType::I64);
                                    self.emit_inst(
                                        MirInst::CallRuntime {
                                            intrinsic: "wml_res_set_body".to_string(),
                                            args: vec![res, ptr, len],
                                        },
                                        MirType::Unit,
                                    );
                                }
                                Some(MirType::I64) | Some(MirType::Bool) => {
                                    if let Some(arena) = self.arena_ptr {
                                        self.emit_inst(
                                            MirInst::CallRuntime {
                                                intrinsic: "wml_res_set_body_i64".to_string(),
                                                args: vec![arena, res, rv],
                                            },
                                            MirType::Unit,
                                        );
                                    }
                                }
                                _ => {
                                    let ptr = self.emit_inst(MirInst::SlicePtr(rv), MirType::Ptr);
                                    let len = self.emit_inst(MirInst::SliceLen(rv), MirType::I64);
                                    self.emit_inst(
                                        MirInst::CallRuntime {
                                            intrinsic: "wml_res_set_body".to_string(),
                                            args: vec![res, ptr, len],
                                        },
                                        MirType::Unit,
                                    );
                                }
                            }
                        }
                    }
                    let zero = self.emit_inst(MirInst::Const(MirConstant::Int(0)), MirType::I64);
                    self.set_terminator(MirTerminator::Return(Some(zero)));
                } else {
                    self.set_terminator(MirTerminator::Return(ret_val));
                }
            }
            HirStmt::Throw { code, message, .. } => {
                let msg_val = message.as_ref().map(|m| self.lower_expr(m));
                self.set_terminator(MirTerminator::Throw {
                    status: *code as u16,
                    message: msg_val,
                });
            }
            HirStmt::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                let cond_val = self.lower_expr(cond);

                let then_bb = self.new_block();
                let else_bb = self.new_block();
                let merge_bb = self.new_block();

                self.set_terminator(MirTerminator::Branch {
                    cond: cond_val,
                    then_block: then_bb,
                    then_args: Vec::new(),
                    else_block: else_bb,
                    else_args: Vec::new(),
                });

                // Then branch
                self.switch_to_block(then_bb);
                for s in then_branch {
                    self.lower_stmt(s, in_route);
                }
                if matches!(
                    self.current_blocks[self.current_block_idx].terminator,
                    MirTerminator::Unreachable
                ) {
                    self.set_terminator(MirTerminator::Jump {
                        target: merge_bb,
                        args: Vec::new(),
                    });
                }

                // Else branch
                self.switch_to_block(else_bb);
                if let Some(else_stmts) = else_branch {
                    for s in else_stmts {
                        self.lower_stmt(s, in_route);
                    }
                }
                if matches!(
                    self.current_blocks[self.current_block_idx].terminator,
                    MirTerminator::Unreachable
                ) {
                    self.set_terminator(MirTerminator::Jump {
                        target: merge_bb,
                        args: Vec::new(),
                    });
                }

                // Continue at merge
                self.switch_to_block(merge_bb);
            }
            HirStmt::For { .. } => {
                // Loop lowering to basic blocks
            }
        }
    }

    fn lower_expr(&mut self, expr: &HirTypedExpr) -> ValueId {
        let mir_ty = Self::type_to_mir(&expr.ty);
        match &expr.kind {
            HirExpr::Literal(lit) => match lit {
                Literal::Int(n) => {
                    self.emit_inst(MirInst::Const(MirConstant::Int(*n)), MirType::I64)
                }
                Literal::Float(f) => {
                    self.emit_inst(MirInst::Const(MirConstant::Float(*f)), MirType::F64)
                }
                Literal::Bool(b) => {
                    self.emit_inst(MirInst::Const(MirConstant::Bool(*b)), MirType::Bool)
                }
                Literal::String(s) => {
                    self.emit_inst(MirInst::Const(MirConstant::Str(s.clone())), MirType::Slice)
                }
                Literal::Null => self.emit_inst(MirInst::Const(MirConstant::Null), MirType::Ptr),
            },
            HirExpr::Var(name) => {
                if let Some(&val) = self.locals.get(name) {
                    val
                } else {
                    // Fallback constant / undefined
                    self.emit_inst(MirInst::Const(MirConstant::Null), mir_ty)
                }
            }
            HirExpr::Binary { op, lhs, rhs } => {
                let l = self.lower_expr(lhs);
                let r = self.lower_expr(rhs);
                let mir_op = match op {
                    BinaryOp::Add => MirBinaryOp::Add,
                    BinaryOp::Sub => MirBinaryOp::Sub,
                    BinaryOp::Mul => MirBinaryOp::Mul,
                    BinaryOp::Div => MirBinaryOp::Div,
                    BinaryOp::Mod => MirBinaryOp::Mod,
                    BinaryOp::Eq => MirBinaryOp::Eq,
                    BinaryOp::NotEq => MirBinaryOp::NotEq,
                    BinaryOp::Lt => MirBinaryOp::Lt,
                    BinaryOp::LtEq => MirBinaryOp::LtEq,
                    BinaryOp::Gt => MirBinaryOp::Gt,
                    BinaryOp::GtEq => MirBinaryOp::GtEq,
                    BinaryOp::And => MirBinaryOp::And,
                    BinaryOp::Or => MirBinaryOp::Or,
                    _ => MirBinaryOp::Add,
                };
                self.emit_inst(
                    MirInst::Binary {
                        op: mir_op,
                        lhs: l,
                        rhs: r,
                    },
                    mir_ty,
                )
            }
            HirExpr::Unary { op, expr: inner } => {
                let val = self.lower_expr(inner);
                let mir_op = match op {
                    UnaryOp::Not => MirUnaryOp::Not,
                    UnaryOp::Neg => MirUnaryOp::Neg,
                };
                self.emit_inst(
                    MirInst::Unary {
                        op: mir_op,
                        operand: val,
                    },
                    mir_ty,
                )
            }
            HirExpr::Coalesce { lhs, rhs } => {
                // Desugar `lhs ?? rhs` into an SSA conditional branch
                let lhs_val = self.lower_expr(lhs);

                let then_bb = self.new_block();
                let else_bb = self.new_block();
                let merge_bb = self.new_block();

                // Add parameter to merge block to receive unwrapped value
                let res_param = self.new_val();
                self.current_blocks
                    .last_mut()
                    .unwrap()
                    .params
                    .push((res_param, mir_ty.clone()));

                // Non-null or truthy check
                let null_val = self.emit_inst(MirInst::Const(MirConstant::Null), MirType::Ptr);
                let is_not_null = self.emit_inst(
                    MirInst::Binary {
                        op: MirBinaryOp::NotEq,
                        lhs: lhs_val,
                        rhs: null_val,
                    },
                    MirType::Bool,
                );

                self.set_terminator(MirTerminator::Branch {
                    cond: is_not_null,
                    then_block: then_bb,
                    then_args: Vec::new(),
                    else_block: else_bb,
                    else_args: Vec::new(),
                });

                // Then block: result is lhs_val
                self.switch_to_block(then_bb);
                self.set_terminator(MirTerminator::Jump {
                    target: merge_bb,
                    args: vec![lhs_val],
                });

                // Else block: evaluate rhs
                self.switch_to_block(else_bb);
                let rhs_val = self.lower_expr(rhs);
                if matches!(
                    self.current_blocks[self.current_block_idx].terminator,
                    MirTerminator::Unreachable
                ) {
                    self.set_terminator(MirTerminator::Jump {
                        target: merge_bb,
                        args: vec![rhs_val],
                    });
                }

                // Switch to merge block
                self.switch_to_block(merge_bb);
                res_param
            }
            HirExpr::Call { callee, args } => {
                let mut arg_vals = Vec::new();
                for a in args {
                    let v = match a {
                        HirCallArg::Positional(e) => self.lower_expr(e),
                        HirCallArg::Named { val, .. } => self.lower_expr(val),
                    };
                    arg_vals.push(v);
                }

                // Inspect callee for built-ins
                if let HirExpr::MemberAccess { object, field } = &callee.kind {
                    if let HirExpr::Var(ns) = &object.kind {
                        let intrinsic_name = format!("wml_{}_{}", ns, field);
                        let mut call_args = Vec::new();
                        // Request arena passed implicitly to db/crypto/req/res operations
                        if let Some(arena) = self.arena_ptr {
                            call_args.push(arena);
                        }
                        call_args.extend(arg_vals);
                        return self.emit_inst(
                            MirInst::CallRuntime {
                                intrinsic: intrinsic_name,
                                args: call_args,
                            },
                            mir_ty,
                        );
                    }
                }

                if let HirExpr::Var(fn_name) = &callee.kind {
                    self.emit_inst(
                        MirInst::Call {
                            func: fn_name.clone(),
                            args: arg_vals,
                        },
                        mir_ty,
                    )
                } else {
                    self.emit_inst(MirInst::Const(MirConstant::Null), mir_ty)
                }
            }
            HirExpr::MemberAccess { object, field } => {
                let obj_val = self.lower_expr(object);
                let field_str = self.emit_inst(
                    MirInst::Const(MirConstant::Str(field.clone())),
                    MirType::Slice,
                );
                self.emit_inst(
                    MirInst::CallRuntime {
                        intrinsic: "wml_record_get_field".to_string(),
                        args: vec![obj_val, field_str],
                    },
                    mir_ty,
                )
            }
            HirExpr::ObjectLit(fields) => {
                // Bump-allocate record in request arena
                let ptr = self.emit_inst(
                    MirInst::AllocReqArena {
                        size: fields.len() * 16,
                        align: 8,
                    },
                    MirType::Ptr,
                );
                for (idx, (f_name, f_val)) in fields.iter().enumerate() {
                    let val = self.lower_expr(f_val);
                    self.emit_inst(
                        MirInst::Store {
                            ptr,
                            offset: (idx * 16) as u32,
                            val,
                        },
                        MirType::Unit,
                    );
                    let _ = f_name;
                }
                ptr
            }
            HirExpr::ArrayLit(items) => {
                let ptr = self.emit_inst(
                    MirInst::AllocReqArena {
                        size: items.len() * 8,
                        align: 8,
                    },
                    MirType::Ptr,
                );
                for (idx, item) in items.iter().enumerate() {
                    let val = self.lower_expr(item);
                    self.emit_inst(
                        MirInst::Store {
                            ptr,
                            offset: (idx * 8) as u32,
                            val,
                        },
                        MirType::Unit,
                    );
                }
                let len = self.emit_inst(
                    MirInst::Const(MirConstant::Int(items.len() as i64)),
                    MirType::I64,
                );
                self.emit_inst(MirInst::MakeSlice { ptr, len }, MirType::Slice)
            }
            HirExpr::Throw { code, message } => {
                let msg_val = message.as_ref().map(|m| self.lower_expr(m));
                self.set_terminator(MirTerminator::Throw {
                    status: *code as u16,
                    message: msg_val,
                });
                self.emit_inst(MirInst::Const(MirConstant::Null), MirType::Ptr)
            }
            HirExpr::Jsx(elem) => {
                // Server-rendered JSX template string
                let tag = self.emit_inst(
                    MirInst::Const(MirConstant::Str(format!(
                        "<{}>...</{}>",
                        elem.tag, elem.tag
                    ))),
                    MirType::Slice,
                );
                tag
            }
        }
    }

    fn reset_fn_state(&mut self) {
        self.next_val = 0;
        self.next_block = 0;
        self.current_blocks.clear();
        self.current_block_idx = 0;
        self.locals.clear();
        self.arena_ptr = None;
        self.req_ptr = None;
        self.res_ptr = None;
    }

    pub fn type_to_mir(ty: &Type) -> MirType {
        match ty {
            Type::Int => MirType::I64,
            Type::Float => MirType::F64,
            Type::Bool => MirType::Bool,
            Type::Str => MirType::Slice,
            Type::Void => MirType::Unit,
            Type::Optional(inner) => Self::type_to_mir(inner),
            Type::Array(_) => MirType::Slice,
            Type::Record(_) | Type::Custom(_) => MirType::Ptr,
            Type::Signal(inner) => Self::type_to_mir(inner),
            Type::Jsx => MirType::Slice,
            _ => MirType::Ptr,
        }
    }
}

fn slugify_path(path: &str) -> String {
    path.trim_matches('/')
        .replace(['/', '-'], "_")
        .replace(':', "param_")
}

fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}
