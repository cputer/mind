// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");

//! Per-evaluation snapshots of initialized module-level `let` bindings.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use super::Value;
use super::autodiff::TensorEnvEntry;
use super::module_bindings::owned_symbol;

#[derive(Clone, Default)]
struct Scope {
    values: HashMap<String, Value>,
    tensors: HashMap<String, TensorEnvEntry>,
}

thread_local! {
    static SCOPES: RefCell<HashMap<Option<String>, Scope>> = RefCell::new(HashMap::new());
    static MODULE_SCOPE: Cell<bool> = const { Cell::new(false) };
}

pub(super) struct Guard {
    scopes: HashMap<Option<String>, Scope>,
    module_scope: bool,
}

pub(super) fn install(
    owner: Option<&str>,
    values: &HashMap<String, Value>,
    tensors: &HashMap<String, TensorEnvEntry>,
) -> Guard {
    SCOPES.with(|slot| {
        let mut scopes = HashMap::new();
        scopes.insert(
            owner.map(str::to_string),
            Scope {
                values: values.clone(),
                tensors: tensors.clone(),
            },
        );
        Guard {
            scopes: std::mem::replace(&mut *slot.borrow_mut(), scopes),
            module_scope: MODULE_SCOPE.with(|active| active.replace(true)),
        }
    })
}

pub(super) struct FunctionGuard(bool);

pub(super) fn enter_function() -> FunctionGuard {
    FunctionGuard(MODULE_SCOPE.with(|active| active.replace(false)))
}

pub(super) fn environment(
    owner: Option<&str>,
) -> (HashMap<String, Value>, HashMap<String, TensorEnvEntry>) {
    SCOPES.with(|slot| {
        slot.borrow()
            .get(&owner.map(str::to_string))
            .cloned()
            .map(|scope| (scope.values, scope.tensors))
            .unwrap_or_default()
    })
}

pub(super) fn function_environment(
    owner: Option<&str>,
    caller_owner: Option<&str>,
    values: &HashMap<String, Value>,
    tensors: &HashMap<String, TensorEnvEntry>,
) -> (HashMap<String, Value>, HashMap<String, TensorEnvEntry>) {
    if MODULE_SCOPE.with(Cell::get) && owner == caller_owner {
        sync(owner, values, tensors);
    }
    environment(owner)
}

pub(super) fn set(owner: Option<&str>, name: &str, value: Value, tensor: Option<TensorEnvEntry>) {
    SCOPES.with(|slot| {
        let mut scopes = slot.borrow_mut();
        let scope = scopes.entry(owner.map(str::to_string)).or_default();
        scope.values.insert(name.to_string(), value);
        if let Some(tensor) = tensor {
            scope.tensors.insert(name.to_string(), tensor);
        } else {
            scope.tensors.remove(name);
        }
    });
}

pub(super) fn bind_parameter(
    values: &mut HashMap<String, Value>,
    tensors: &mut HashMap<String, TensorEnvEntry>,
    name: &str,
    value: Value,
) {
    if let Value::Tensor(tensor) = &value {
        tensors.insert(
            name.to_string(),
            TensorEnvEntry {
                value: tensor.clone(),
                expr: None,
            },
        );
    } else {
        tensors.remove(name);
    }
    values.insert(name.to_string(), value);
}

pub(super) fn sync(
    owner: Option<&str>,
    values: &HashMap<String, Value>,
    tensors: &HashMap<String, TensorEnvEntry>,
) {
    SCOPES.with(|slot| {
        let mut scopes = slot.borrow_mut();
        let Some(scope) = scopes.get_mut(&owner.map(str::to_string)) else {
            return;
        };
        let qualified = |name: &str| owner.map(|owner| owned_symbol(owner, name));
        let names = scope.values.keys().cloned().collect::<Vec<_>>();
        for name in names {
            let key = qualified(&name);
            if let Some(updated) = key
                .as_ref()
                .and_then(|key| values.get(key))
                .or_else(|| values.get(&name))
            {
                scope.values.insert(name.clone(), updated.clone());
            }
            let updated_tensor = key
                .as_ref()
                .and_then(|key| tensors.get(key))
                .or_else(|| tensors.get(&name))
                .cloned();
            if let Some(updated) = updated_tensor {
                scope.tensors.insert(name, updated);
            } else {
                scope.tensors.remove(&name);
            }
        }
    });
}

impl Drop for Guard {
    fn drop(&mut self) {
        SCOPES.with(|slot| *slot.borrow_mut() = std::mem::take(&mut self.scopes));
        MODULE_SCOPE.with(|active| active.set(self.module_scope));
    }
}

impl Drop for FunctionGuard {
    fn drop(&mut self) {
        MODULE_SCOPE.with(|active| active.set(self.0));
    }
}
