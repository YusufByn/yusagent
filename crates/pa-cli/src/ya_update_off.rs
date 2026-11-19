//! yusAgent (YA-16, ticket 19): self-update is disabled.
//!
//! The upstream update flow installs Prime Agent's own releases (installer from
//! `app.primeintellect.ai`), which would replace this build with upstream's. The `update`
//! command and the TUI's `/update` runner refuse with [`SELF_UPDATE_OFF`] before any download.
//! Unharnessed removed the command (`75cfc357fe501a46f4a01a345457c0897eb88d89`, "unharnessed
//! step 1a: drop the terminal client subcommands") and the update module
//! (`09f760dfa4b9fbb205443f6fcf7736256ccc226f`); here it is cut, not deleted, so upstream
//! cherry-picks stay cheap. Listed in `docs/fork-changes.md`.

/// Whether the binary may update itself. No setting or environment variable reads it.
pub(crate) const SELF_UPDATE: bool = false;

/// The refusal `update` and `/update` print.
pub(crate) const SELF_UPDATE_OFF: &str =
    "Self-update is disabled in yusAgent. Install a newer yusAgent build to update.";
