use crate::hir::HirCallArg;
use crate::types::Type;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ModelDef {
    pub name: String,
    pub fields: HashMap<String, ModelField>,
}

#[derive(Debug, Clone)]
pub struct ModelField {
    pub name: String,
    pub ty: Type,
    pub is_auto: bool,
    pub has_default: bool,
}

pub struct Builtins;

impl Builtins {
    pub fn check_call(
        namespace: &str,
        method: &str,
        args: &[HirCallArg],
        models: &HashMap<String, ModelDef>,
        route_auth: bool,
    ) -> Result<Type, BuiltinError> {
        match namespace {
            "crypto" => Self::check_crypto(method, args),
            "jwt" => Self::check_jwt(method, args),
            "db" => Self::check_db(method, args, models),
            "req" => Self::check_req(method, args, route_auth),
            "res" => Self::check_res(method, args),
            _ => Err(BuiltinError::UnknownNamespace(namespace.to_string())),
        }
    }

    fn check_crypto(method: &str, args: &[HirCallArg]) -> Result<Type, BuiltinError> {
        match method {
            "hash" => {
                if args.len() != 1 {
                    return Err(BuiltinError::ArgCountMismatch {
                        expected: 1,
                        found: args.len(),
                    });
                }
                let arg_ty = get_arg_type(&args[0]);
                if !arg_ty.is_assignable_to(&Type::Str) {
                    return Err(BuiltinError::TypeMismatch {
                        expected: Type::Str,
                        found: arg_ty.clone(),
                        param_name: "pass".to_string(),
                    });
                }
                Ok(Type::Str)
            }
            "verify" => {
                if args.len() != 2 {
                    return Err(BuiltinError::ArgCountMismatch {
                        expected: 2,
                        found: args.len(),
                    });
                }
                let plain_ty = get_arg_type(&args[0]);
                let hash_ty = get_arg_type(&args[1]);
                if !plain_ty.is_assignable_to(&Type::Str) {
                    return Err(BuiltinError::TypeMismatch {
                        expected: Type::Str,
                        found: plain_ty.clone(),
                        param_name: "plain".to_string(),
                    });
                }
                if !hash_ty.is_assignable_to(&Type::Str) {
                    return Err(BuiltinError::TypeMismatch {
                        expected: Type::Str,
                        found: hash_ty.clone(),
                        param_name: "hash".to_string(),
                    });
                }
                Ok(Type::Bool)
            }
            _ => Err(BuiltinError::UnknownMethod {
                namespace: "crypto".to_string(),
                method: method.to_string(),
            }),
        }
    }

    fn check_jwt(method: &str, args: &[HirCallArg]) -> Result<Type, BuiltinError> {
        match method {
            "sign" => {
                if args.len() != 1 {
                    return Err(BuiltinError::ArgCountMismatch {
                        expected: 1,
                        found: args.len(),
                    });
                }
                let payload_ty = get_arg_type(&args[0]);
                match payload_ty {
                    Type::Record(_) | Type::Custom(_) | Type::Any => Ok(Type::Str),
                    other => Err(BuiltinError::TypeMismatch {
                        expected: Type::Record(vec![]),
                        found: other.clone(),
                        param_name: "claims".to_string(),
                    }),
                }
            }
            "verify" => {
                if args.len() != 1 {
                    return Err(BuiltinError::ArgCountMismatch {
                        expected: 1,
                        found: args.len(),
                    });
                }
                let token_ty = get_arg_type(&args[0]);
                if !token_ty.is_assignable_to(&Type::Str) {
                    return Err(BuiltinError::TypeMismatch {
                        expected: Type::Str,
                        found: token_ty.clone(),
                        param_name: "token".to_string(),
                    });
                }
                Ok(Type::Any)
            }
            _ => Err(BuiltinError::UnknownMethod {
                namespace: "jwt".to_string(),
                method: method.to_string(),
            }),
        }
    }

    fn check_db(
        method: &str,
        args: &[HirCallArg],
        models: &HashMap<String, ModelDef>,
    ) -> Result<Type, BuiltinError> {
        if args.is_empty() {
            return Err(BuiltinError::ArgCountMismatch {
                expected: 1,
                found: 0,
            });
        }

        // First argument to db methods is the Model name (e.g. User, Stats)
        let model_name = match &args[0] {
            HirCallArg::Positional(expr) => match &expr.kind {
                crate::hir::HirExpr::Var(name) => name.clone(),
                _ => {
                    return Err(BuiltinError::ExpectedModelIdentifier);
                }
            },
            _ => return Err(BuiltinError::ExpectedModelIdentifier),
        };

        let model = models
            .get(&model_name)
            .ok_or_else(|| BuiltinError::UnknownModel(model_name.clone()))?;

        match method {
            "insert" => {
                if args.len() != 2 {
                    return Err(BuiltinError::ArgCountMismatch {
                        expected: 2,
                        found: args.len(),
                    });
                }
                let data_arg = &args[1];
                let data_ty = get_arg_type(data_arg);

                // Verify fields in data against model
                if let Type::Record(record_fields) = data_ty {
                    // Check for unknown fields
                    for (field_name, field_ty) in record_fields {
                        if let Some(expected_field) = model.fields.get(field_name) {
                            if !field_ty.is_assignable_to(&expected_field.ty) {
                                return Err(BuiltinError::FieldTypeMismatch {
                                    model: model_name.clone(),
                                    field: field_name.clone(),
                                    expected: expected_field.ty.clone(),
                                    found: field_ty.clone(),
                                });
                            }
                        } else {
                            return Err(BuiltinError::UnknownField {
                                model: model_name.clone(),
                                field: field_name.clone(),
                            });
                        }
                    }

                    // Check for missing required fields (not auto, not id, not default, and not optional)
                    for (f_name, f_def) in &model.fields {
                        if f_name != "id"
                            && !f_def.is_auto
                            && !f_def.has_default
                            && !f_def.ty.is_optional()
                            && !record_fields.iter().any(|(n, _)| n == f_name)
                        {
                            return Err(BuiltinError::MissingRequiredField {
                                model: model_name.clone(),
                                field: f_name.clone(),
                                expected_type: f_def.ty.clone(),
                            });
                        }
                    }
                }

                Ok(Type::Custom(model_name))
            }
            "get" => {
                if args.len() < 2 {
                    return Err(BuiltinError::ArgCountMismatch {
                        expected: 2,
                        found: args.len(),
                    });
                }

                // Check filter arguments
                for arg in &args[1..] {
                    match arg {
                        HirCallArg::Named { name, val } => {
                            if let Some(expected_field) = model.fields.get(name) {
                                if !val.ty.is_assignable_to(&expected_field.ty) {
                                    return Err(BuiltinError::FieldTypeMismatch {
                                        model: model_name.clone(),
                                        field: name.clone(),
                                        expected: expected_field.ty.clone(),
                                        found: val.ty.clone(),
                                    });
                                }
                            } else {
                                return Err(BuiltinError::UnknownField {
                                    model: model_name.clone(),
                                    field: name.clone(),
                                });
                            }
                        }
                        HirCallArg::Positional(val) => {
                            if let Type::Record(record_fields) = &val.ty {
                                for (k, v) in record_fields {
                                    if let Some(expected_field) = model.fields.get(k) {
                                        if !v.is_assignable_to(&expected_field.ty) {
                                            return Err(BuiltinError::FieldTypeMismatch {
                                                model: model_name.clone(),
                                                field: k.clone(),
                                                expected: expected_field.ty.clone(),
                                                found: v.clone(),
                                            });
                                        }
                                    } else {
                                        return Err(BuiltinError::UnknownField {
                                            model: model_name.clone(),
                                            field: k.clone(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }

                Ok(Type::Optional(Box::new(Type::Custom(model_name))))
            }
            "update" => Ok(Type::Custom(model_name)),
            "delete" => Ok(Type::Bool),
            _ => Err(BuiltinError::UnknownMethod {
                namespace: "db".to_string(),
                method: method.to_string(),
            }),
        }
    }

    fn check_req(
        method: &str,
        _args: &[HirCallArg],
        route_auth: bool,
    ) -> Result<Type, BuiltinError> {
        match method {
            "user" => {
                if !route_auth {
                    return Err(BuiltinError::AuthRequiredForReqUser);
                }
                Ok(Type::Record(vec![
                    ("id".to_string(), Type::Int),
                    ("email".to_string(), Type::Str),
                ]))
            }
            _ => Ok(Type::Any),
        }
    }

    fn check_res(method: &str, args: &[HirCallArg]) -> Result<Type, BuiltinError> {
        match method {
            "status" => {
                if !args.is_empty() {
                    let code_ty = get_arg_type(&args[0]);
                    if !code_ty.is_assignable_to(&Type::Int) {
                        return Err(BuiltinError::TypeMismatch {
                            expected: Type::Int,
                            found: code_ty.clone(),
                            param_name: "status_code".to_string(),
                        });
                    }
                }
                Ok(Type::Any)
            }
            "json" | "redirect" | "send" | "header" | "cookie" => Ok(Type::Any),
            _ => Ok(Type::Any),
        }
    }
}

fn get_arg_type(arg: &HirCallArg) -> &Type {
    match arg {
        HirCallArg::Positional(expr) => &expr.ty,
        HirCallArg::Named { val, .. } => &val.ty,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BuiltinError {
    UnknownNamespace(String),
    UnknownMethod {
        namespace: String,
        method: String,
    },
    ArgCountMismatch {
        expected: usize,
        found: usize,
    },
    TypeMismatch {
        expected: Type,
        found: Type,
        param_name: String,
    },
    ExpectedModelIdentifier,
    UnknownModel(String),
    MissingRequiredField {
        model: String,
        field: String,
        expected_type: Type,
    },
    UnknownField {
        model: String,
        field: String,
    },
    FieldTypeMismatch {
        model: String,
        field: String,
        expected: Type,
        found: Type,
    },
    AuthRequiredForReqUser,
}
