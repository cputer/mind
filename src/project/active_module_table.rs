// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// Part of the MIND project (Machine Intelligence Native Design).

//! Scoped ownership of the active cross-module export table.

use std::cell::RefCell;

use super::module_table::ModuleTable;

thread_local! {
    static ACTIVE: RefCell<Option<ModuleTable>> = const { RefCell::new(None) };
}

/// Replace the active table. Retained for project-build compatibility.
pub(crate) fn set(table: Option<ModuleTable>) {
    ACTIVE.with(|cell| *cell.borrow_mut() = table);
}

/// Borrow the active table for one non-reentrant lookup.
pub(crate) fn with<R>(f: impl FnOnce(Option<&ModuleTable>) -> R) -> R {
    ACTIVE.with(|cell| f(cell.borrow().as_ref()))
}

/// Installs a table and restores the previous value on every exit path.
pub(crate) struct Guard {
    previous: Option<ModuleTable>,
}

impl Guard {
    pub(crate) fn install(table: ModuleTable) -> Self {
        let previous = ACTIVE.with(|cell| cell.borrow_mut().replace(table));
        Self { previous }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        ACTIVE.with(|cell| *cell.borrow_mut() = self.previous.take());
    }
}
