# ya-apply-patch

Port of the `apply_patch` crate of OpenAI Codex, used by yusAgent to apply
`*** Begin Patch` edits.

## Origin

- Repository: <https://github.com/openai/codex>
- Path: `codex-rs/apply-patch`
- Commit: `69231e119bfca0b14453515e97d99688ce1c47c9`
- License: Apache-2.0 (`LICENSE` and `NOTICE` copied from the root of that
  repository at the same commit). Every modified file starts with a
  `// Modified by yusAgent: ...` comment.

The code that computes new file contents (patch parsing, hunk application,
`seek_sequence`, the tolerant matching, `streaming_parser`, `text_file`) is
Codex's code unchanged. Disk access goes through `std::fs` (synchronous)
instead of the async `codex_exec_server::ExecutorFileSystem`, and paths are
`Path`/`PathBuf` instead of `PathUri`.

## Files

Unchanged:

- `src/seek_sequence.rs`
- `src/streaming_parser.rs`
- `src/text_file.rs`
- `tests/all.rs`
- `tests/suite/mod.rs`
- `tests/fixtures/**` (scenarios, byte for byte, CRLF fixtures included)
- `rustfmt.toml` (copy of `codex-rs/rustfmt.toml`)

Modified:

- `src/lib.rs`: sync `std::fs` access, `Path` instead of `PathUri`, no
  `sandbox` parameter; `invocation` types removed (`MaybeApplyPatchVerified`,
  `ApplyPatchAction`, `ApplyPatchFileChange`, `ApplyPatchError::PathUri`,
  `ApplyPatchError::ImplicitInvocation`).
- `src/file_update.rs`: sync `std::fs` read; the content computation of
  `derive_new_contents_from_chunks` moves unchanged into
  `derive_new_contents_from_text`.
- `src/file_update_tests.rs`: sync calls, `Path` instead of `PathUri`.
- `src/parser.rs`: `Hunk::resolve_path` takes and returns `Path`/`PathBuf`.
- `src/main.rs`: crate renamed `ya_apply_patch`.
- `src/standalone_executable.rs`: no tokio runtime, cwd from
  `std::env::current_dir()`.
- `tests/suite/cli.rs`, `tests/suite/tool.rs`, `tests/suite/scenarios.rs`:
  binary and fixture paths from Cargo (`CARGO_BIN_EXE_apply_patch`,
  `CARGO_MANIFEST_DIR`) instead of `codex_utils_cargo_bin`.
- `tests/suite/no_follow.rs`: sync calls, `Path` cwd; the
  `verify_apply_patch_args` step is replaced by `parse_patch`.

Added (not from Codex):

- `src/local_fs.rs`: the `std::fs` operations, including a check-then-act
  emulation of Codex's no-follow mode (`follow_symlinks: false`), which Codex
  implements atomically with `openat(O_NOFOLLOW)`.
- `src/tool_spec.rs`: the name, description and Lark grammar of Codex's
  freeform tool (`codex-rs/core/src/tools/handlers/apply_patch_spec.rs`
  lines 5 and 20), exported by `lib.rs`; `assets/apply_patch.lark` is
  `codex-rs/core/assets/tools/apply_patch.lark` byte for byte.
- `Cargo.toml`, `clippy.toml` (subset of `codex-rs/clippy.toml`), this README.

Removed:

- `src/invocation.rs` (detection of `apply_patch` inside shell commands,
  tree-sitter / tree-sitter-bash) and its tests.
- `BUILD.bazel`.
