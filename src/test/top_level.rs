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

//! Traversal of transparent blocks at module-item scope.

use crate::ast::Node;

/// Return module items in source order, recursively flattening only blocks
/// encountered at module-item scope. Function bodies are opaque because a
/// `FnDef` is emitted directly and is never traversed.
pub(super) fn refs(items: &[Node]) -> Vec<&Node> {
    fn append<'a>(items: &'a [Node], out: &mut Vec<&'a Node>) {
        for item in items {
            match item {
                Node::Block { stmts, .. } => append(stmts, out),
                other => out.push(other),
            }
        }
    }
    let mut out = Vec::new();
    append(items, &mut out);
    out
}

pub(super) fn cloned(items: &[Node]) -> Vec<Node> {
    refs(items).into_iter().cloned().collect()
}
