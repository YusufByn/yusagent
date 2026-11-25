#!/usr/bin/env bash
# Ticket 40 (docs/tickets/40-login-extension.md, §11) : simulation de P-A et P-B sur 1ece0ff04, puis
# `git cherry-pick --no-commit` de a05bcb695 et dad2232cc (upstream). Application textuelle seulement,
# rien compilé. Servira au ticket E-C.
# Usage : git worktree add --detach <wt> 1ece0ff04 && bash bench/tools/sim40.sh <wt> ; puis
#         git worktree remove --force <wt>. Le worktree est remis à zéro à chaque cas.
set -u
W=$1; cd "$W"
g(){ git -c user.name=t -c user.email=t@t -c core.hooksPath=/dev/null "$@"; }
resolve_a05(){ python3 - <<'PY'
import re
p='crates/pa-daemon/src/agent_engine/tests.rs'
s=open(p).read()
s=s.replace("<<<<<<< HEAD\n=======\nmod incomplete_reply;\nmod live_auth;\n>>>>>>> a05bcb695 (live auth refresh (#4019))\n","mod live_auth;\n")
s=re.sub(r"<<<<<<< HEAD\n(.*?)fn write_compaction_settings\(dir: &std::path::Path, reserve_tokens: u64\) \{\n=======\n.*?>>>>>>> a05bcb695 \(live auth refresh \(#4019\)\)\n", lambda m: m.group(1)+"pub(crate) fn write_compaction_settings(dir: &std::path::Path, reserve_tokens: u64) {\n", s, flags=re.S)
assert "<<<<<<<" not in s
open(p,'w').write(s)
PY
}
run(){
  label=$1; shift
  g reset -q --hard 1ece0ff04; g clean -qfd
  "$@"
  g add -A; g commit -q --allow-empty -m "sim $label"
  g cherry-pick --no-commit a05bcb695 >/dev/null 2>&1; r1=$?
  u1=$(git diff --name-only --diff-filter=U | tr '\n' ' ')
  [ -n "$u1" ] && resolve_a05
  g add -A; g commit -q -m a05
  g cherry-pick --no-commit dad2232cc >/dev/null 2>&1; r2=$?
  u2=$(git diff --name-only --diff-filter=U | tr '\n' ' ')
  echo "$label | a05 exit=$r1 conflits: ${u1:-aucun} | dad après a05 exit=$r2 conflits: ${u2:-aucun}"
  g reset -q --hard HEAD; g clean -qfd
}
noop(){ :; }
pa(){ git mv crates/pa-tui/src/auth_panel.rs crates/pa-daemon/src/auth_panel.rs
      git mv crates/pa-tui/src/provider_auth.rs crates/pa-daemon/src/provider_auth.rs
      echo 'pub use pa_daemon::{auth_panel, provider_auth};' >> crates/pa-tui/src/lib.rs
      echo 'pa-daemon = { path = "../pa-daemon" }' >> crates/pa-tui/Cargo.toml
      printf 'pub mod auth_panel;\npub mod provider_auth;\n' >> crates/pa-daemon/src/lib.rs; }
pb(){ echo '// ya_providers' > crates/pa-daemon/src/acp/ya_providers.rs
      echo '// ya_acp_providers' > crates/pa-cli/src/ya_acp_providers.rs
      python3 -c "
p='crates/pa-cli/src/print_runtime.rs'; s=open(p).read()
a='fn run_acp_mode(options: &RunOptions) -> Result<i32, String> {\n'
assert s.count(a)==1; open(p,'w').write(s.replace(a,a+'    // ya: injection des fournisseurs (simulation)\n'))"; }
run main noop
run P-A pa
run P-B pb
