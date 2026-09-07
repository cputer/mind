// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Lexical module identity for the tree evaluator's linked test modules.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::ast::Span;

const OWNER_PREFIX: &str = "\0mind-eval-owner\0";

pub(crate) fn owned_symbol(owner: &str, name: &str) -> String {
    format!("{OWNER_PREFIX}{owner}\0{name}")
}

pub(super) fn symbol_owner(name: &str) -> Option<&str> {
    name.strip_prefix(OWNER_PREFIX)
        .and_then(|rest| rest.split_once('\0'))
        .map(|(owner, _)| owner)
}

pub(super) fn symbol_display_name(name: &str) -> &str {
    name.strip_prefix(OWNER_PREFIX)
        .and_then(|rest| rest.split_once('\0'))
        .map_or(name, |(_, symbol)| symbol)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingKind {
    Call,
    Value,
}

#[derive(Clone, Default)]
pub(crate) struct Bindings {
    calls: HashMap<(String, Span), String>,
    values: HashMap<(String, Span), String>,
}

impl Bindings {
    #[cfg(any(feature = "cross-module-imports", test))]
    pub(crate) fn insert(
        &mut self,
        owner: &str,
        span: Span,
        kind: BindingKind,
        target_owner: &str,
        symbol: &str,
    ) {
        let key = (owner.to_string(), span);
        let target = owned_symbol(target_owner, symbol);
        match kind {
            BindingKind::Call => self.calls.insert(key, target),
            BindingKind::Value => self.values.insert(key, target),
        };
    }
}

thread_local! {
    static BINDINGS: RefCell<Bindings> = RefCell::new(Bindings::default());
    static OWNER: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub(super) fn current_owner() -> Option<String> {
    OWNER.with(|owner| owner.borrow().clone())
}

pub(super) fn bound_symbol(span: Span, kind: BindingKind) -> Option<String> {
    let owner = current_owner()?;
    BINDINGS.with(|bindings| {
        let bindings = bindings.borrow();
        match kind {
            BindingKind::Call => bindings.calls.get(&(owner, span)).cloned(),
            BindingKind::Value => bindings.values.get(&(owner, span)).cloned(),
        }
    })
}

pub(crate) struct BindingsGuard {
    previous_bindings: Bindings,
    previous_owner: Option<String>,
}

impl BindingsGuard {
    pub(crate) fn install(bindings: Bindings, owner: Option<String>) -> Self {
        let previous_bindings =
            BINDINGS.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), bindings));
        let previous_owner = OWNER.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), owner));
        Self {
            previous_bindings,
            previous_owner,
        }
    }
}

impl Drop for BindingsGuard {
    fn drop(&mut self) {
        BINDINGS.with(|slot| *slot.borrow_mut() = std::mem::take(&mut self.previous_bindings));
        OWNER.with(|slot| *slot.borrow_mut() = self.previous_owner.take());
    }
}

pub(super) struct OwnerGuard(Option<String>);

impl OwnerGuard {
    pub(super) fn enter(owner: Option<&str>) -> Self {
        let previous = OWNER
            .with(|slot| std::mem::replace(&mut *slot.borrow_mut(), owner.map(str::to_string)));
        Self(previous)
    }
}

impl Drop for OwnerGuard {
    fn drop(&mut self) {
        OWNER.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_key_includes_owner_and_owner_guards_restore_on_unwind() {
        let span = Span::new(7, 11);
        let mut bindings = Bindings::default();
        bindings.insert("crate.a", span, BindingKind::Call, "crate.left", "value");
        bindings.insert("crate.b", span, BindingKind::Call, "crate.right", "value");
        let _bindings = BindingsGuard::install(bindings, Some("crate.a".into()));
        assert_eq!(
            bound_symbol(span, BindingKind::Call),
            Some(owned_symbol("crate.left", "value"))
        );
        let _ = std::panic::catch_unwind(|| {
            let _owner = OwnerGuard::enter(Some("crate.b"));
            assert_eq!(
                bound_symbol(span, BindingKind::Call),
                Some(owned_symbol("crate.right", "value"))
            );
            panic!("exercise unwind restoration");
        });
        assert_eq!(current_owner().as_deref(), Some("crate.a"));
    }
}
