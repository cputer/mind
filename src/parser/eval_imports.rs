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

//! Evaluator-only provenance for namespace references.
//!
//! The compiler AST intentionally normalises `module.symbol` to a bare call or
//! identifier. The test evaluator requests this companion data so it can keep
//! lexical module ownership without changing the ordinary parser result.

use crate::ast::{Module, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvalImportRefKind {
    Call,
    Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EvalImportRef {
    pub(crate) span: Span,
    pub(crate) qualifier: Vec<String>,
    pub(crate) symbol: String,
    pub(crate) kind: EvalImportRefKind,
}

pub(crate) struct EvalParsedModule {
    pub(crate) module: Module,
    #[allow(dead_code)]
    pub(crate) import_refs: Vec<EvalImportRef>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluator_capture_does_not_change_the_compiler_ast() {
        let source = "use crate.src.dep; fn f() -> i64 { return dep.call(dep.VALUE); }";
        let ordinary = crate::parser::parse(source).expect("ordinary parse");
        let captured = crate::parser::parse_for_eval(source).expect("evaluator parse");
        assert_eq!(captured.module, ordinary);
        assert_eq!(captured.import_refs.len(), 2);
        assert_eq!(captured.import_refs[0].qualifier, ["dep"]);
        assert_eq!(captured.import_refs[0].symbol, "VALUE");
        assert_eq!(captured.import_refs[0].kind, EvalImportRefKind::Value);
        assert_eq!(captured.import_refs[1].qualifier, ["dep"]);
        assert_eq!(captured.import_refs[1].symbol, "call");
        assert_eq!(captured.import_refs[1].kind, EvalImportRefKind::Call);
    }
}
