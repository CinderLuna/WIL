use crate::builtins::{BuiltinError, Builtins, ModelDef, ModelField};
use crate::hir::*;
use crate::scope::{Scope, ScopeKind};
use crate::types::Type;
use std::collections::HashMap;
use std::ops::Range;
use wml_diagnostics::{
    byte_to_line_col, compute_context_slice, generate_diagnostic_id, generate_unified_diff,
    ByteRange, Diagnostic, DiagnosticCategory, DiagnosticSeverity, Location, PatchType,
    RelatedLocation, Replacement, Span, SuggestedPatch, TypeInfo,
};
use wml_syntax::ast::{
    AstArena, Block, CallArg, ElseBranch, Expr, ExprId, Item, JsxAttrValue, JsxChild, Literal,
    ObjectField, Program, Stmt,
};

#[derive(Default)]
pub struct DiagMeta {
    pub related_locations: Vec<RelatedLocation>,
    pub type_info: Option<TypeInfo>,
    pub patch: Option<SuggestedPatch>,
}

pub struct TypeChecker<'a> {
    source: &'a str,
    file: &'a str,
    arena: &'a AstArena,
    diagnostics: Vec<Diagnostic>,
    models: HashMap<String, ModelDef>,
}

impl<'a> TypeChecker<'a> {
    pub fn new(source: &'a str, file: &'a str, arena: &'a AstArena) -> Self {
        Self {
            source,
            file,
            arena,
            diagnostics: Vec::new(),
            models: HashMap::new(),
        }
    }

    fn emit_diag(
        &mut self,
        code: &str,
        category: DiagnosticCategory,
        message: &str,
        span: Range<usize>,
        patch: Option<SuggestedPatch>,
    ) {
        self.emit_diagnostic(
            code,
            category,
            DiagnosticSeverity::Error,
            message,
            span,
            DiagMeta {
                related_locations: Vec::new(),
                type_info: None,
                patch,
            },
        );
    }

    fn emit_diagnostic(
        &mut self,
        code: &str,
        category: DiagnosticCategory,
        severity: DiagnosticSeverity,
        message: &str,
        span: Range<usize>,
        mut meta: DiagMeta,
    ) {
        let (start_line, start_col) = byte_to_line_col(self.source, span.start);
        let (end_line, end_col) = byte_to_line_col(self.source, span.end);

        let byte_range = ByteRange {
            start: span.start,
            end: span.end,
        };
        let id = generate_diagnostic_id(self.file, &byte_range, code);
        let context_slice = compute_context_slice(self.source, start_line, 1);

        if let Some(p) = &mut meta.patch {
            if p.patch_unified.is_none() {
                p.patch_unified = Some(generate_unified_diff(
                    self.file,
                    self.source,
                    &p.replacement.byte_range,
                    &p.replacement.new_text,
                ));
            }
        }

        self.diagnostics.push(Diagnostic {
            id,
            code: code.to_string(),
            category,
            severity,
            message: message.to_string(),
            primary_location: Location {
                file: self.file.to_string(),
                span: Span {
                    start_line,
                    start_col,
                    end_line,
                    end_col,
                },
                byte_range,
            },
            related_locations: meta.related_locations,
            type_info: meta.type_info,
            context_slice: Some(context_slice),
            suggested_patch: meta.patch,
        });
    }

    pub fn check_program(mut self, program: &Program) -> (HirProgram, Vec<Diagnostic>) {
        let mut hir_types = HashMap::new();
        let mut hir_schemas = HashMap::new();
        let mut hir_routes = Vec::new();
        let mut hir_pages = Vec::new();
        let mut hir_uis = Vec::new();
        let mut hir_fns = Vec::new();

        let mut root_scope = Scope::new(ScopeKind::ServerGlobal);

        // Pass 1: Collect types and schemas
        for item in &program.items {
            match item {
                Item::Type(t) => {
                    let mut fields = Vec::new();
                    let mut model_fields = HashMap::new();
                    for f in &t.fields {
                        let ty = Type::from_type_ref(&f.ty);
                        fields.push(HirFieldDef {
                            name: f.name.clone(),
                            ty: ty.clone(),
                        });
                        model_fields.insert(
                            f.name.clone(),
                            ModelField {
                                name: f.name.clone(),
                                ty,
                                is_auto: false,
                                has_default: false,
                            },
                        );
                    }
                    self.models.insert(
                        t.name.clone(),
                        ModelDef {
                            name: t.name.clone(),
                            fields: model_fields,
                        },
                    );
                    root_scope.define(t.name.clone(), Type::Custom(t.name.clone()), false);
                    hir_types.insert(
                        t.name.clone(),
                        HirTypeDecl {
                            name: t.name.clone(),
                            fields,
                            span: 0..0,
                        },
                    );
                }
                Item::Schema(s) => {
                    let mut fields = Vec::new();
                    let mut model_fields = HashMap::new();
                    for f in &s.fields {
                        let ty = Type::from_type_ref(&f.ty);
                        let is_pk = f
                            .modifiers
                            .iter()
                            .any(|m| matches!(m, wml_syntax::ast::FieldModifier::Pk));
                        let is_unique = f
                            .modifiers
                            .iter()
                            .any(|m| matches!(m, wml_syntax::ast::FieldModifier::Unique));
                        let is_indexed = f
                            .modifiers
                            .iter()
                            .any(|m| matches!(m, wml_syntax::ast::FieldModifier::Indexed));
                        let is_auto = f
                            .modifiers
                            .iter()
                            .any(|m| matches!(m, wml_syntax::ast::FieldModifier::Auto));
                        let has_default = f
                            .modifiers
                            .iter()
                            .any(|m| matches!(m, wml_syntax::ast::FieldModifier::Default(_)));

                        fields.push(HirSchemaField {
                            name: f.name.clone(),
                            ty: ty.clone(),
                            is_pk,
                            is_unique,
                            is_indexed,
                            is_auto,
                            has_default,
                        });
                        model_fields.insert(
                            f.name.clone(),
                            ModelField {
                                name: f.name.clone(),
                                ty,
                                is_auto,
                                has_default,
                            },
                        );
                    }
                    self.models.insert(
                        s.name.clone(),
                        ModelDef {
                            name: s.name.clone(),
                            fields: model_fields,
                        },
                    );
                    root_scope.define(s.name.clone(), Type::Custom(s.name.clone()), false);
                    hir_schemas.insert(
                        s.name.clone(),
                        HirSchemaDecl {
                            name: s.name.clone(),
                            fields,
                            span: 0..0,
                        },
                    );
                }
                Item::Fn(f) => {
                    let params: Vec<Type> = f
                        .params
                        .iter()
                        .map(|p| Type::from_type_ref(&p.ty))
                        .collect();
                    let ret = f
                        .return_type
                        .as_ref()
                        .map(Type::from_type_ref)
                        .unwrap_or(Type::Void);
                    root_scope.define(
                        f.name.clone(),
                        Type::Function {
                            params,
                            ret: Box::new(ret),
                        },
                        false,
                    );
                }
                _ => {}
            }
        }

        // Pass 2: Check routes, pages, uis, and functions
        for item in &program.items {
            match item {
                Item::Route(r) => {
                    let hir_route = self.check_route(r, &root_scope);
                    hir_routes.push(hir_route);
                }
                Item::Page(p) => {
                    let hir_page = self.check_page(p, &root_scope);
                    hir_pages.push(hir_page);
                }
                Item::Ui(u) => {
                    let hir_ui = self.check_ui(u, &root_scope);
                    hir_uis.push(hir_ui);
                }
                Item::Fn(f) => {
                    let hir_fn = self.check_fn(f, &root_scope);
                    hir_fns.push(hir_fn);
                }
                _ => {}
            }
        }

        (
            HirProgram {
                types: hir_types,
                schemas: hir_schemas,
                routes: hir_routes,
                pages: hir_pages,
                uis: hir_uis,
                fns: hir_fns,
            },
            self.diagnostics,
        )
    }

    fn check_route(
        &mut self,
        r: &wml_syntax::ast::RouteDecl,
        parent_scope: &Scope,
    ) -> HirRouteDecl {
        let mut scope = parent_scope.enter_child(ScopeKind::ServerRoute);
        let ret_ty = r
            .return_type
            .as_ref()
            .map(Type::from_type_ref)
            .unwrap_or(Type::Void);

        let payload_ty = r.config.payload_type.as_ref().map(Type::from_type_ref);

        // Inject payload fields into route scope
        if let Some(Type::Record(fields)) = &payload_ty {
            for (f_name, f_ty) in fields {
                scope.define(f_name.clone(), f_ty.clone(), false);
            }
        }

        let is_auth = r.config.auth.unwrap_or(false);

        let body = self.check_block(&r.body, &mut scope, &ret_ty, is_auth);

        HirRouteDecl {
            path: r.path.clone(),
            method: r.config.method,
            payload_type: payload_ty,
            auth: is_auth,
            rate_limit: r.config.rate_limit,
            return_type: ret_ty,
            body,
            span: 0..0,
        }
    }

    fn check_page(&mut self, p: &wml_syntax::ast::PageDecl, parent_scope: &Scope) -> HirPageDecl {
        let mut scope = parent_scope.enter_child(ScopeKind::ServerPage);
        let is_auth = p.config.auth.unwrap_or(false);
        let body = self.check_block(&p.body, &mut scope, &Type::Jsx, is_auth);

        HirPageDecl {
            path: p.path.clone(),
            auth: is_auth,
            rate_limit: p.config.rate_limit,
            body,
            span: 0..0,
        }
    }

    fn check_ui(&mut self, u: &wml_syntax::ast::UiDecl, parent_scope: &Scope) -> HirUiDecl {
        let mut scope = parent_scope.enter_child(ScopeKind::ClientUi);
        let mut params = Vec::new();

        for p in &u.params {
            let ty = Type::from_type_ref(&p.ty);
            scope.define(p.name.clone(), ty.clone(), false);
            params.push(HirParam {
                name: p.name.clone(),
                ty,
            });
        }

        let mut actions = Vec::new();
        let mut body_stmts = Vec::new();

        // Pass 1: Pre-register action functions in UI scope so event handlers can invoke them
        for stmt in &u.body.stmts {
            if let Stmt::Action {
                name,
                params: a_params,
                return_type,
                ..
            } = stmt
            {
                let a_ret = return_type
                    .as_ref()
                    .map(Type::from_type_ref)
                    .unwrap_or(Type::Void);
                let param_types: Vec<Type> = a_params
                    .iter()
                    .map(|p| Type::from_type_ref(&p.ty))
                    .collect();
                scope.define(
                    name.clone(),
                    Type::Function {
                        params: param_types,
                        ret: Box::new(a_ret),
                    },
                    false,
                );
            }
        }

        // Pass 2: Typecheck action bodies (server scope) and UI statements (client scope)
        for stmt in &u.body.stmts {
            if let Stmt::Action {
                name,
                params: a_params,
                return_type,
                body: a_body,
            } = stmt
            {
                let a_ret = return_type
                    .as_ref()
                    .map(Type::from_type_ref)
                    .unwrap_or(Type::Void);
                let mut a_scope = parent_scope.enter_child(ScopeKind::ClientAction);
                let mut act_params = Vec::new();
                for ap in a_params {
                    let a_ty = Type::from_type_ref(&ap.ty);
                    a_scope.define(ap.name.clone(), a_ty.clone(), false);
                    act_params.push(HirParam {
                        name: ap.name.clone(),
                        ty: a_ty,
                    });
                }
                let a_body_hir = self.check_block(a_body, &mut a_scope, &a_ret, false);
                actions.push(HirActionDecl {
                    name: name.clone(),
                    params: act_params,
                    return_type: a_ret,
                    body: a_body_hir,
                    span: 0..0,
                });
            } else {
                if let Some(s) = self.check_stmt(stmt, &mut scope, &Type::Jsx, false) {
                    body_stmts.push(s);
                }
            }
        }

        HirUiDecl {
            name: u.name.clone(),
            params,
            body: body_stmts,
            actions,
            span: 0..0,
        }
    }

    fn check_fn(&mut self, f: &wml_syntax::ast::FnDecl, parent_scope: &Scope) -> HirFnDecl {
        let mut scope = parent_scope.enter_child(ScopeKind::ServerFn);
        let mut params = Vec::new();
        for p in &f.params {
            let ty = Type::from_type_ref(&p.ty);
            scope.define(p.name.clone(), ty.clone(), false);
            params.push(HirParam {
                name: p.name.clone(),
                ty,
            });
        }
        let ret_ty = f
            .return_type
            .as_ref()
            .map(Type::from_type_ref)
            .unwrap_or(Type::Void);
        let body = self.check_block(&f.body, &mut scope, &ret_ty, false);

        HirFnDecl {
            name: f.name.clone(),
            params,
            return_type: ret_ty,
            body,
            span: 0..0,
        }
    }

    fn check_block(
        &mut self,
        block: &Block,
        scope: &mut Scope,
        expected_ret: &Type,
        route_auth: bool,
    ) -> Vec<HirStmt> {
        let mut stmts = Vec::new();
        for s in &block.stmts {
            if let Some(hir_stmt) = self.check_stmt(s, scope, expected_ret, route_auth) {
                stmts.push(hir_stmt);
            }
        }
        stmts
    }

    fn check_stmt(
        &mut self,
        stmt: &Stmt,
        scope: &mut Scope,
        expected_ret: &Type,
        route_auth: bool,
    ) -> Option<HirStmt> {
        match stmt {
            Stmt::Let { name, ty, val } => {
                let val_span = self.arena.get_span(*val);
                let val_expr = self.check_expr(*val, scope, route_auth);
                let declared_ty = ty.as_ref().map(Type::from_type_ref);

                let final_ty = if let Some(d_ty) = declared_ty {
                    if !val_expr.ty.is_assignable_to(&d_ty) {
                        self.emit_diag(
                            "WML-TYP-2041",
                            DiagnosticCategory::TypeMismatch,
                            &format!(
                                "Mismatched types: expected `{}`, found `{}` in variable `{}`",
                                d_ty, val_expr.ty, name
                            ),
                            val_span.clone(),
                            None,
                        );
                    }
                    d_ty
                } else {
                    val_expr.ty.clone()
                };

                let is_sig = matches!(val_expr.ty, Type::Signal(_));
                scope.define(name.clone(), final_ty.clone(), is_sig);

                Some(HirStmt::Let {
                    name: name.clone(),
                    ty: final_ty,
                    val: val_expr,
                    span: val_span,
                })
            }
            Stmt::Return(ret_expr) => {
                if let Some(e) = ret_expr {
                    let span = self.arena.get_span(*e);
                    let typed_expr = self.check_expr(*e, scope, route_auth);

                    if !typed_expr.ty.is_assignable_to(expected_ret) {
                        self.emit_diag(
                            "WML-TYP-2042",
                            DiagnosticCategory::TypeMismatch,
                            &format!(
                                "Mismatched return type: expected `{}`, found `{}`",
                                expected_ret, typed_expr.ty
                            ),
                            span.clone(),
                            None,
                        );
                    }
                    Some(HirStmt::Return {
                        val: Some(typed_expr),
                        span,
                    })
                } else {
                    if *expected_ret != Type::Void && *expected_ret != Type::Any {
                        self.emit_diag(
                            "WML-TYP-2042",
                            DiagnosticCategory::TypeMismatch,
                            &format!(
                                "Mismatched return type: expected `{}`, found `void`",
                                expected_ret
                            ),
                            0..0,
                            None,
                        );
                    }
                    Some(HirStmt::Return {
                        val: None,
                        span: 0..0,
                    })
                }
            }
            Stmt::Throw { code, message } => {
                let msg_hir = message.map(|m| self.check_expr(m, scope, route_auth));
                Some(HirStmt::Throw {
                    code: *code,
                    message: msg_hir,
                    span: 0..0,
                })
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_span = self.arena.get_span(*cond);
                let cond_hir = self.check_expr(*cond, scope, route_auth);
                if !cond_hir.ty.is_assignable_to(&Type::Bool) {
                    self.emit_diag(
                        "WML-TYP-2041",
                        DiagnosticCategory::TypeMismatch,
                        &format!(
                            "If condition must be of type `bool`, found `{}`",
                            cond_hir.ty
                        ),
                        cond_span,
                        None,
                    );
                }

                let mut then_scope = scope.enter_child(ScopeKind::LocalBlock);
                let then_stmts =
                    self.check_block(then_branch, &mut then_scope, expected_ret, route_auth);

                let else_stmts = if let Some(eb) = else_branch {
                    match eb {
                        ElseBranch::Block(b) => {
                            let mut else_scope = scope.enter_child(ScopeKind::LocalBlock);
                            Some(self.check_block(b, &mut else_scope, expected_ret, route_auth))
                        }
                        ElseBranch::If(if_stmt) => {
                            let mut else_scope = scope.enter_child(ScopeKind::LocalBlock);
                            self.check_stmt(if_stmt, &mut else_scope, expected_ret, route_auth)
                                .map(|s| vec![s])
                        }
                    }
                } else {
                    None
                };

                Some(HirStmt::If {
                    cond: cond_hir,
                    then_branch: then_stmts,
                    else_branch: else_stmts,
                    span: 0..0,
                })
            }
            Stmt::For { var, iter, body } => {
                let iter_span = self.arena.get_span(*iter);
                let iter_hir = self.check_expr(*iter, scope, route_auth);
                let elem_ty = match &iter_hir.ty {
                    Type::Array(inner) => (**inner).clone(),
                    Type::Any => Type::Any,
                    other => {
                        self.emit_diag(
                            "WML-TYP-2041",
                            DiagnosticCategory::TypeMismatch,
                            &format!("Cannot iterate over non-array type `{}`", other),
                            iter_span,
                            None,
                        );
                        Type::Any
                    }
                };

                let mut for_scope = scope.enter_child(ScopeKind::LocalBlock);
                for_scope.define(var.clone(), elem_ty, false);
                let body_stmts = self.check_block(body, &mut for_scope, expected_ret, route_auth);

                Some(HirStmt::For {
                    var: var.clone(),
                    iter: iter_hir,
                    body: body_stmts,
                    span: 0..0,
                })
            }
            Stmt::Expr(e) => {
                let typed_expr = self.check_expr(*e, scope, route_auth);
                Some(HirStmt::Expr(typed_expr))
            }
            Stmt::Action { .. } => None, // Handled in check_ui
        }
    }

    fn check_expr(&mut self, expr_id: ExprId, scope: &Scope, route_auth: bool) -> HirTypedExpr {
        let span = self.arena.get_span(expr_id);
        let expr = self.arena.get_expr(expr_id).clone();

        match expr {
            Expr::Literal(lit) => {
                let ty = match &lit {
                    Literal::Int(_) => Type::Int,
                    Literal::Float(_) => Type::Float,
                    Literal::String(_) => Type::Str,
                    Literal::Bool(_) => Type::Bool,
                    Literal::Null => Type::Any,
                };
                HirTypedExpr {
                    kind: HirExpr::Literal(lit),
                    ty,
                    span,
                }
            }
            Expr::Ident(name) => {
                if scope.is_client_island()
                    && matches!(
                        name.as_str(),
                        "db" | "crypto" | "jwt" | "req" | "res" | "env"
                    )
                {
                    self.emit_diag(
                        "WML-SEC-5001",
                        DiagnosticCategory::SecurityViolation,
                        &format!(
                            "Server module `{}` cannot be accessed in client UI islands. Use a server action instead.",
                            name
                        ),
                        span.clone(),
                        None,
                    );
                }

                if let Some(sym) = scope.lookup(&name) {
                    HirTypedExpr {
                        kind: HirExpr::Var(name),
                        ty: sym.ty.clone(),
                        span,
                    }
                } else {
                    let closest = scope.find_closest_match(&name);
                    let patch = closest.map(|sug| SuggestedPatch {
                        patch_type: PatchType::AutoApply,
                        confidence: 0.95,
                        explanation: format!(
                            "Replace undefined symbol `{}` with `{}` in scope",
                            name, sug
                        ),
                        replacement: Replacement {
                            file: self.file.to_string(),
                            byte_range: ByteRange {
                                start: span.start,
                                end: span.end,
                            },
                            new_text: sug,
                        },
                        patch_unified: None,
                    });

                    self.emit_diag(
                        "WML-TYP-2001",
                        DiagnosticCategory::UndefinedIdentifier,
                        &format!("Undefined symbol `{}` in scope", name),
                        span.clone(),
                        patch,
                    );

                    HirTypedExpr {
                        kind: HirExpr::Var(name),
                        ty: Type::Any,
                        span,
                    }
                }
            }
            Expr::MemberAccess { object, field } => {
                let obj_hir = self.check_expr(object, scope, route_auth);
                let effective_obj_ty = obj_hir.ty.unwrap_signal();
                let ty = match effective_obj_ty {
                    Type::Namespace(ns) => {
                        if ns == "req" && field == "user" {
                            if !route_auth {
                                self.emit_diag(
                                    "WML-SEC-5002",
                                    DiagnosticCategory::SecurityViolation,
                                    "Accessing `req.user` requires `(auth: true)` on the route",
                                    span.clone(),
                                    None,
                                );
                            }
                            Type::Record(vec![
                                ("id".to_string(), Type::Int),
                                ("email".to_string(), Type::Str),
                            ])
                        } else {
                            Type::Namespace(format!("{}.{}", ns, field))
                        }
                    }
                    Type::Record(fields) => {
                        let found = fields.iter().find(|(n, _)| n == &field);
                        match found {
                            Some((_, ty)) => ty.clone(),
                            None => {
                                self.emit_diag(
                                    "WML-TYP-2044",
                                    DiagnosticCategory::TypeMismatch,
                                    &format!(
                                        "Field `{}` does not exist on record `{}`",
                                        field, obj_hir.ty
                                    ),
                                    span.clone(),
                                    None,
                                );
                                Type::Any
                            }
                        }
                    }
                    Type::Custom(model_name) => {
                        if let Some(model) = self.models.get(model_name) {
                            if let Some(f_def) = model.fields.get(&field) {
                                f_def.ty.clone()
                            } else {
                                self.emit_diag(
                                    "WML-TYP-2044",
                                    DiagnosticCategory::TypeMismatch,
                                    &format!(
                                        "Field `{}` does not exist on model `{}`",
                                        field, model_name
                                    ),
                                    span.clone(),
                                    None,
                                );
                                Type::Any
                            }
                        } else {
                            Type::Any
                        }
                    }
                    Type::Any => Type::Any,
                    other => {
                        self.emit_diag(
                            "WML-TYP-2041",
                            DiagnosticCategory::TypeMismatch,
                            &format!(
                                "Cannot access property `{}` on non-object type `{}`",
                                field, other
                            ),
                            span.clone(),
                            None,
                        );
                        Type::Any
                    }
                };

                HirTypedExpr {
                    kind: HirExpr::MemberAccess {
                        object: Box::new(obj_hir),
                        field,
                    },
                    ty,
                    span,
                }
            }
            Expr::Call { callee, args } => {
                let callee_span = self.arena.get_span(callee);
                let callee_hir = self.check_expr(callee, scope, route_auth);
                let mut checked_args = Vec::new();

                let is_db_update = if let HirExpr::MemberAccess {
                    object,
                    field: method,
                } = &callee_hir.kind
                {
                    if let Type::Namespace(ns) = &object.ty {
                        ns == "db" && method == "update"
                    } else {
                        false
                    }
                } else {
                    false
                };

                let mut update_scope = scope.clone();
                if is_db_update {
                    if let Some(CallArg::Positional(first_id)) = args.first() {
                        if let Expr::Ident(model_name) = self.arena.get_expr(*first_id) {
                            if let Some(model) = self.models.get(model_name) {
                                let mut child_scope = scope.enter_child(ScopeKind::LocalBlock);
                                for (f_name, f_def) in &model.fields {
                                    // Only define if not already present in caller scope to prevent shadowing outer variables
                                    if scope.lookup(f_name).is_none() {
                                        child_scope.define(f_name.clone(), f_def.ty.clone(), false);
                                    }
                                }
                                update_scope = child_scope;
                            }
                        }
                    }
                }

                let num_args = args.len();
                for (idx, arg) in args.iter().enumerate() {
                    // For db.update, only the last argument (the update payload) uses update_scope with model fields
                    let arg_scope = if is_db_update && idx == num_args - 1 && num_args > 1 {
                        &update_scope
                    } else {
                        scope
                    };
                    match arg {
                        CallArg::Positional(e) => {
                            let typed = self.check_expr(*e, arg_scope, route_auth);
                            checked_args.push(HirCallArg::Positional(typed));
                        }
                        CallArg::Named { name, val } => {
                            let typed = self.check_expr(*val, arg_scope, route_auth);
                            checked_args.push(HirCallArg::Named {
                                name: name.clone(),
                                val: typed,
                            });
                        }
                    }
                }

                // Check built-in namespace call (e.g. crypto.hash, db.insert, jwt.sign)
                if let HirExpr::MemberAccess { object, field } = &callee_hir.kind {
                    if let Type::Namespace(ns) = &object.ty {
                        match Builtins::check_call(
                            ns,
                            field,
                            &checked_args,
                            &self.models,
                            route_auth,
                        ) {
                            Ok(ret_ty) => {
                                return HirTypedExpr {
                                    kind: HirExpr::Call {
                                        callee: Box::new(callee_hir),
                                        args: checked_args,
                                    },
                                    ty: ret_ty,
                                    span,
                                };
                            }
                            Err(builtin_err) => {
                                match builtin_err {
                                    BuiltinError::ArgCountMismatch { expected, found } => {
                                        self.emit_diag(
                                            "WML-TYP-2050",
                                            DiagnosticCategory::TypeMismatch,
                                            &format!(
                                                "`{}.{}` expects {} arguments, found {}",
                                                ns, field, expected, found
                                            ),
                                            span.clone(),
                                            None,
                                        );
                                    }
                                    BuiltinError::TypeMismatch {
                                        expected,
                                        found,
                                        param_name,
                                    } => {
                                        self.emit_diag(
                                            "WML-TYP-2041",
                                            DiagnosticCategory::TypeMismatch,
                                            &format!(
                                                "Mismatched argument type for parameter `{}` in `{}.{}`: expected `{}`, found `{}`",
                                                param_name, ns, field, expected, found
                                            ),
                                            span.clone(),
                                            None,
                                        );
                                    }
                                    BuiltinError::ExpectedModelIdentifier => {
                                        self.emit_diag(
                                            "WML-TYP-2002",
                                            DiagnosticCategory::UndefinedIdentifier,
                                            &format!(
                                                "First argument to `{}.{}` must be a Model identifier (Type or Schema)",
                                                ns, field
                                            ),
                                            span.clone(),
                                            None,
                                        );
                                    }
                                    BuiltinError::UnknownModel(m) => {
                                        self.emit_diag(
                                            "WML-TYP-2002",
                                            DiagnosticCategory::UndefinedIdentifier,
                                            &format!(
                                                "Unknown model `{}` referenced in `{}.{}`",
                                                m, ns, field
                                            ),
                                            span.clone(),
                                            None,
                                        );
                                    }
                                    BuiltinError::MissingRequiredField {
                                        model,
                                        field: f_name,
                                        expected_type,
                                    } => {
                                        self.emit_diag(
                                            "WML-TYP-2043",
                                            DiagnosticCategory::DatabaseConstraint,
                                            &format!(
                                                "Missing required field `{}: {}` in `{}` insert",
                                                f_name, expected_type, model
                                            ),
                                            span.clone(),
                                            None,
                                        );
                                    }
                                    BuiltinError::UnknownField {
                                        model,
                                        field: f_name,
                                    } => {
                                        self.emit_diag(
                                            "WML-TYP-2044",
                                            DiagnosticCategory::TypeMismatch,
                                            &format!(
                                                "Unknown field `{}` specified for model `{}`",
                                                f_name, model
                                            ),
                                            span.clone(),
                                            None,
                                        );
                                    }
                                    BuiltinError::FieldTypeMismatch {
                                        model,
                                        field: f_name,
                                        expected,
                                        found,
                                    } => {
                                        let mut field_span = span.clone();
                                        for arg in &args {
                                            let expr_id = match arg {
                                                CallArg::Positional(id) => *id,
                                                CallArg::Named { val, .. } => *val,
                                            };
                                            if let Expr::ObjectLit(fields) =
                                                self.arena.get_expr(expr_id)
                                            {
                                                for f in fields {
                                                    if f.name == f_name {
                                                        if let Some(val_id) = f.val {
                                                            field_span =
                                                                self.arena.get_span(val_id);
                                                        }
                                                        break;
                                                    }
                                                }
                                            }
                                        }

                                        let mut patch = None;
                                        if let Some(sym) = scope.lookup(&f_name) {
                                            if sym.ty == expected {
                                                patch = Some(SuggestedPatch {
                                                    patch_type: PatchType::AutoApply,
                                                    confidence: 0.98,
                                                    explanation: format!(
                                                        "Replace literal with variable `{}` passed in scope.",
                                                        f_name
                                                    ),
                                                    replacement: Replacement {
                                                        file: self.file.to_string(),
                                                        byte_range: ByteRange {
                                                            start: field_span.start,
                                                            end: field_span.end,
                                                        },
                                                        new_text: f_name.clone(),
                                                    },
                                                    patch_unified: None,
                                                });
                                            }
                                        }

                                        self.emit_diagnostic(
                                            "WML-TYP-2041",
                                            DiagnosticCategory::TypeMismatch,
                                            DiagnosticSeverity::Error,
                                            &format!(
                                                "Mismatched type for field `{}` on `{}`: expected `{}`, found `{}`",
                                                f_name, model, expected, found
                                            ),
                                            field_span,
                                            DiagMeta {
                                                related_locations: Vec::new(),
                                                type_info: Some(TypeInfo {
                                                    expected: expected.to_string(),
                                                    actual: found.to_string(),
                                                    coercible: Some(true),
                                                }),
                                                patch,
                                            },
                                        );
                                    }
                                    BuiltinError::AuthRequiredForReqUser => {
                                        self.emit_diag(
                                            "WML-SEC-5002",
                                            DiagnosticCategory::SecurityViolation,
                                            "Route requires `(auth: true)` config to access `req.user`",
                                            span.clone(),
                                            None,
                                        );
                                    }
                                    BuiltinError::UnknownMethod { namespace, method } => {
                                        self.emit_diag(
                                            "WML-TYP-2050",
                                            DiagnosticCategory::UndefinedIdentifier,
                                            &format!(
                                                "Method `{}` does not exist on `{}`",
                                                method, namespace
                                            ),
                                            span.clone(),
                                            None,
                                        );
                                    }
                                    BuiltinError::UnknownNamespace(n) => {
                                        self.emit_diag(
                                            "WML-TYP-2001",
                                            DiagnosticCategory::UndefinedIdentifier,
                                            &format!("Unknown namespace `{}`", n),
                                            span.clone(),
                                            None,
                                        );
                                    }
                                }
                                return HirTypedExpr {
                                    kind: HirExpr::Call {
                                        callee: Box::new(callee_hir),
                                        args: checked_args,
                                    },
                                    ty: Type::Any,
                                    span,
                                };
                            }
                        }
                    }
                }

                // Check function call
                let ret_ty = match &callee_hir.ty {
                    Type::Function { params, ret } => {
                        if params.len() != checked_args.len() {
                            self.emit_diag(
                                "WML-TYP-2050",
                                DiagnosticCategory::TypeMismatch,
                                &format!(
                                    "Function expects {} arguments, found {}",
                                    params.len(),
                                    checked_args.len()
                                ),
                                span.clone(),
                                None,
                            );
                        } else {
                            for (p_ty, arg) in params.iter().zip(&checked_args) {
                                let arg_ty = match arg {
                                    HirCallArg::Positional(e) => &e.ty,
                                    HirCallArg::Named { val, .. } => &val.ty,
                                };
                                if !arg_ty.is_assignable_to(p_ty) {
                                    self.emit_diag(
                                        "WML-TYP-2041",
                                        DiagnosticCategory::TypeMismatch,
                                        &format!(
                                            "Argument type `{}` is not assignable to parameter type `{}`",
                                            arg_ty, p_ty
                                        ),
                                        span.clone(),
                                        None,
                                    );
                                }
                            }
                        }
                        (**ret).clone()
                    }
                    Type::Any => Type::Any,
                    other => {
                        self.emit_diag(
                            "WML-TYP-2041",
                            DiagnosticCategory::TypeMismatch,
                            &format!("Cannot call non-function type `{}`", other),
                            callee_span,
                            None,
                        );
                        Type::Any
                    }
                };

                HirTypedExpr {
                    kind: HirExpr::Call {
                        callee: Box::new(callee_hir),
                        args: checked_args,
                    },
                    ty: ret_ty,
                    span,
                }
            }
            Expr::Binary { op, lhs, rhs } => {
                let _lhs_span = self.arena.get_span(lhs);
                let rhs_span = self.arena.get_span(rhs);
                let lhs_hir = self.check_expr(lhs, scope, route_auth);
                let rhs_hir = self.check_expr(rhs, scope, route_auth);

                let lhs_unwrapped = lhs_hir.ty.unwrap_signal();
                let rhs_unwrapped = rhs_hir.ty.unwrap_signal();

                let res_ty = match op {
                    wml_syntax::ast::BinaryOp::Coalesce => {
                        // The universal `??` operator
                        // lhs is Optional<T> or Bool, unwraps to T or evaluates rhs
                        if let Some(unwrapped) = lhs_unwrapped.unwrap_optional() {
                            if !rhs_unwrapped.is_assignable_to(&unwrapped)
                                && rhs_hir.ty != Type::Void
                            {
                                self.emit_diag(
                                    "WML-TYP-2041",
                                    DiagnosticCategory::TypeMismatch,
                                    &format!(
                                        "Right side of `??` type `{}` must match unwrapped left side type `{}`",
                                        rhs_hir.ty, unwrapped
                                    ),
                                    rhs_span,
                                    None,
                                );
                            }
                            unwrapped
                        } else if *lhs_unwrapped == Type::Bool {
                            // crypto.verify(...) ?? throw 401
                            Type::Bool
                        } else {
                            lhs_unwrapped.clone()
                        }
                    }
                    wml_syntax::ast::BinaryOp::Assign => {
                        if !rhs_hir.ty.is_assignable_to(&lhs_hir.ty) {
                            self.emit_diag(
                                "WML-TYP-2041",
                                DiagnosticCategory::TypeMismatch,
                                &format!(
                                    "Cannot assign type `{}` to variable of type `{}`",
                                    rhs_hir.ty, lhs_hir.ty
                                ),
                                rhs_span,
                                None,
                            );
                        }
                        lhs_hir.ty.clone()
                    }
                    wml_syntax::ast::BinaryOp::Add
                    | wml_syntax::ast::BinaryOp::Sub
                    | wml_syntax::ast::BinaryOp::Mul
                    | wml_syntax::ast::BinaryOp::Div
                    | wml_syntax::ast::BinaryOp::Mod => {
                        if *lhs_unwrapped == Type::Int && *rhs_unwrapped == Type::Int {
                            Type::Int
                        } else if (*lhs_unwrapped == Type::Float || *lhs_unwrapped == Type::Int)
                            && (*rhs_unwrapped == Type::Float || *rhs_unwrapped == Type::Int)
                        {
                            Type::Float
                        } else if op == wml_syntax::ast::BinaryOp::Add
                            && (*lhs_unwrapped == Type::Str || *rhs_unwrapped == Type::Str)
                        {
                            Type::Str
                        } else if *lhs_unwrapped == Type::Any || *rhs_unwrapped == Type::Any {
                            Type::Any
                        } else {
                            self.emit_diag(
                                "WML-TYP-2041",
                                DiagnosticCategory::TypeMismatch,
                                &format!(
                                    "Operator `{:?}` not supported between `{}` and `{}`",
                                    op, lhs_hir.ty, rhs_hir.ty
                                ),
                                span.clone(),
                                None,
                            );
                            Type::Any
                        }
                    }
                    wml_syntax::ast::BinaryOp::AddAssign | wml_syntax::ast::BinaryOp::SubAssign => {
                        if !rhs_unwrapped.is_assignable_to(lhs_unwrapped)
                            && *lhs_unwrapped != Type::Any
                            && *rhs_unwrapped != Type::Any
                        {
                            self.emit_diag(
                                "WML-TYP-2041",
                                DiagnosticCategory::TypeMismatch,
                                &format!(
                                    "Cannot apply compound assignment `{:?}` with RHS `{}` to `{}`",
                                    op, rhs_hir.ty, lhs_hir.ty
                                ),
                                rhs_span,
                                None,
                            );
                        }
                        lhs_hir.ty.clone()
                    }
                    wml_syntax::ast::BinaryOp::Eq
                    | wml_syntax::ast::BinaryOp::NotEq
                    | wml_syntax::ast::BinaryOp::Lt
                    | wml_syntax::ast::BinaryOp::LtEq
                    | wml_syntax::ast::BinaryOp::Gt
                    | wml_syntax::ast::BinaryOp::GtEq => Type::Bool,
                    wml_syntax::ast::BinaryOp::And | wml_syntax::ast::BinaryOp::Or => Type::Bool,
                };

                HirTypedExpr {
                    kind: HirExpr::Binary {
                        op,
                        lhs: Box::new(lhs_hir),
                        rhs: Box::new(rhs_hir),
                    },
                    ty: res_ty,
                    span,
                }
            }
            Expr::Unary { op, expr } => {
                let inner = self.check_expr(expr, scope, route_auth);
                let ty = match op {
                    wml_syntax::ast::UnaryOp::Not => Type::Bool,
                    wml_syntax::ast::UnaryOp::Neg => inner.ty.clone(),
                };
                HirTypedExpr {
                    kind: HirExpr::Unary {
                        op,
                        expr: Box::new(inner),
                    },
                    ty,
                    span,
                }
            }
            Expr::ObjectLit(fields) => {
                let mut record_fields = Vec::new();
                let mut hir_fields = Vec::new();

                for ObjectField { name, val } in fields {
                    let field_expr = if let Some(v) = val {
                        self.check_expr(v, scope, route_auth)
                    } else {
                        // Shorthand { email }
                        if let Some(sym) = scope.lookup(&name) {
                            HirTypedExpr {
                                kind: HirExpr::Var(name.clone()),
                                ty: sym.ty.clone(),
                                span: span.clone(),
                            }
                        } else {
                            self.emit_diag(
                                "WML-TYP-2001",
                                DiagnosticCategory::UndefinedIdentifier,
                                &format!("Undefined symbol `{}` in object shorthand", name),
                                span.clone(),
                                None,
                            );
                            HirTypedExpr {
                                kind: HirExpr::Var(name.clone()),
                                ty: Type::Any,
                                span: span.clone(),
                            }
                        }
                    };

                    record_fields.push((name.clone(), field_expr.ty.clone()));
                    hir_fields.push((name, field_expr));
                }

                HirTypedExpr {
                    kind: HirExpr::ObjectLit(hir_fields),
                    ty: Type::Record(record_fields),
                    span,
                }
            }
            Expr::ArrayLit(items) => {
                let mut elem_ty = Type::Any;
                let mut hir_items = Vec::new();

                for item in items {
                    let typed = self.check_expr(item, scope, route_auth);
                    if elem_ty == Type::Any {
                        elem_ty = typed.ty.clone();
                    }
                    hir_items.push(typed);
                }

                HirTypedExpr {
                    kind: HirExpr::ArrayLit(hir_items),
                    ty: Type::Array(Box::new(elem_ty)),
                    span,
                }
            }
            Expr::Throw { code, message } => {
                let msg_hir = message.map(|m| Box::new(self.check_expr(m, scope, route_auth)));
                HirTypedExpr {
                    kind: HirExpr::Throw {
                        code,
                        message: msg_hir,
                    },
                    ty: Type::Void,
                    span,
                }
            }
            Expr::Jsx(jsx) => {
                let hir_jsx = self.check_jsx_element(&jsx, scope, route_auth);
                HirTypedExpr {
                    kind: HirExpr::Jsx(hir_jsx),
                    ty: Type::Jsx,
                    span,
                }
            }
        }
    }

    fn check_jsx_element(
        &mut self,
        jsx: &wml_syntax::ast::JsxElement,
        scope: &Scope,
        route_auth: bool,
    ) -> HirJsxElement {
        let mut attrs = Vec::new();
        for attr in &jsx.attrs {
            let val = match &attr.value {
                Some(JsxAttrValue::String(s)) => Some(HirTypedExpr {
                    kind: HirExpr::Literal(Literal::String(s.clone())),
                    ty: Type::Str,
                    span: 0..0,
                }),
                Some(JsxAttrValue::Expr(e)) => Some(self.check_expr(*e, scope, route_auth)),
                Some(JsxAttrValue::Block(b)) => {
                    let mut block_scope = scope.enter_child(ScopeKind::LocalBlock);
                    self.check_block(b, &mut block_scope, &Type::Void, route_auth);
                    None
                }
                None => None,
            };
            attrs.push((attr.name.clone(), val));
        }

        let mut children = Vec::new();
        for child in &jsx.children {
            match child {
                JsxChild::Element(el) => {
                    children.push(HirJsxChild::Element(
                        self.check_jsx_element(el, scope, route_auth),
                    ));
                }
                JsxChild::Expr(e) => {
                    children.push(HirJsxChild::Expr(self.check_expr(*e, scope, route_auth)));
                }
                JsxChild::Text(t) => {
                    children.push(HirJsxChild::Text(t.clone()));
                }
            }
        }

        HirJsxElement {
            tag: jsx.tag.clone(),
            attrs,
            children,
            span: jsx.span.clone(),
        }
    }
}

pub fn check(
    program: &Program,
    arena: &AstArena,
    source: &str,
    file: &str,
) -> (HirProgram, Vec<Diagnostic>) {
    let checker = TypeChecker::new(source, file, arena);
    checker.check_program(program)
}
