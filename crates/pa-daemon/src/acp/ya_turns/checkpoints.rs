//! yusAgent (YA-18, ticket 43, PR 3a): workspace snapshots in a shadow git repository, one per
//! workspace under the agent directory, taken before a prompt of this client is sent to the worker
//! and at the end of every turn; their preview and their restore
//! (`docs/tickets/43-retour-arriere.md`, §P, §P.12).
//!
//! Push A (tests first): the skeleton only. `_yus/session/restore` and
//! `_yus/session/restore_preview` reach this module through the bridge's `rewind` arm and are
//! answered as not served (`-32601`, the bridge's answer to an unknown method), the behavior of
//! `main`. Listed in `docs/fork-changes.md`.

use serde_json::{json, Value};

use super::super::jsonrpc;
use super::super::producer::FrameSink;

/// Restore the workspace files, the conversation, or both, to before a prompt.
pub(super) const RESTORE: &str = "_yus/session/restore";
/// What a restore would change, or one file as it was.
pub(super) const RESTORE_PREVIEW: &str = "_yus/session/restore_preview";

/// Whether [`serve`] answers `method`.
pub(super) fn serves(method: &str) -> bool {
    matches!(method, RESTORE | RESTORE_PREVIEW)
}

/// Push A: not served yet, answered as the bridge answers an unknown method.
pub(super) fn serve(method: &str, id: &Value, tx: &FrameSink) {
    let _ = tx.send(jsonrpc::error_response(
        id,
        jsonrpc::METHOD_NOT_FOUND,
        &format!("\"Method not found\": {method}"),
        Some(&json!({ "method": method })),
    ));
}
