//! yusAgent (YA-11, ticket 20): the wire names of the permission requests of a daemon session,
//! shared by the worker (`pa_daemon::ya_permissions`), the ACP bridge and the TUI (which depends
//! on `pa-types` only). Listed in `docs/fork-changes.md`.

/// The `create` and `attach` field by which a client declares that it answers the requests.
pub const ASKER_FIELD: &str = "yaPermissionAsker";
/// The session event of a request (worker to clients): `requestId`, `toolCallId`, `toolName`,
/// `args`.
pub const REQUEST_EVENT: &str = "ya_permission_request";
/// The session event that closes a request (answered, or withdrawn): `requestId`, `toolCallId`,
/// `outcome` (`allow`, `deny` or `cancelled`).
pub const RESOLVED_EVENT: &str = "ya_permission_resolved";
/// The session event of a change of mode: `mode` (`manual` or `auto`).
pub const MODE_EVENT: &str = "ya_permission_mode";
/// The command of the clients (`action`: `respond`, `set_mode`, `get`).
pub const COMMAND: &str = "ya_permissions";
/// The field of `get_connection_state` that carries the mode of a session that can ask.
pub const STATE_FIELD: &str = "yaPermissions";
