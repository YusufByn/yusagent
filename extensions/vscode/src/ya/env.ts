// yusAgent (YA-10, ticket 24, PR 3) : l'environnement passé au binaire. L'extension lui donne celui
// de VS Code, sauf `PRIME_AGENT_KERNEL_PYTHON` : avec elle, le binaire prend ce Python pour celui de
// l'appelant et désactive les skills Python qui y manquent, dont `edit`
// (`crates/pa-core/src/kernel/bootstrap/mod.rs:184-214`). Exportée là où VS Code a été lancé, elle
// passerait sans qu'on le voie. Unharnessed ne la filtre pas (`vscode/src/agent.ts:96` à
// `157dcdc1d`, `spawn` sans `env`) ; décision de Yusuf du 21/11/2026 : la retirer, sans réglage.
// `PRIME_AGENT_KERNEL_VENV` (dossier du venv, skills installés par le binaire) passe.

/** Les variables jamais passées au binaire. */
export const STRIPPED_ENV = ["PRIME_AGENT_KERNEL_PYTHON"] as const;

export function agentEnv(env: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  const out = { ...env };
  for (const key of STRIPPED_ENV) delete out[key];
  return out;
}
