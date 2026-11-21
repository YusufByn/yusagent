// The `modelInstructions` setting (MODEL_INSTRUCTIONS.md): groups of
// models, each with a text the agent's system prompt carries when it runs
// on one of them. The panel shows the groups through these, the extension
// writes them: both see the same models as hidden.
import type { ModelChoice, ModelInstructions, Provider, SettingsView } from "./protocol";

/** How a group names a model. */
export function modelKey(choice: Pick<ModelChoice, "provider" | "id">): string {
  return `${choice.provider}/${choice.id}`;
}

/** The groups in the global settings file. */
export function savedGroups(view: SettingsView | null): ModelInstructions[] {
  const groups = view?.values.modelInstructions;
  return Array.isArray(groups) ? (groups as ModelInstructions[]) : [];
}

/**
 * The models of the connected providers, by key: the only ones a group
 * shows. The others stay in the file and come back with their provider.
 * The session's list keeps the current model even when its provider is
 * gone, hence the providers.
 */
export function connectedModels(choices: ModelChoice[], providers: Provider[]): Map<string, ModelChoice> {
  const connected = new Set(providers.filter((provider) => provider.connected).map((provider) => provider.id));
  return new Map(
    choices.filter((choice) => connected.has(choice.provider)).map((choice) => [modelKey(choice), choice]),
  );
}

/**
 * The groups once the one at `index` (`null`: a new one) is saved with the
 * `checked` models and `text`. Its hidden models keep their place; the
 * newly checked ones follow.
 */
export function withGroup(
  groups: ModelInstructions[],
  index: number | null,
  checked: string[],
  text: string,
  connected: ReadonlySet<string>,
): ModelInstructions[] {
  const saved = index === null ? [] : groups[index].models;
  const models = [
    ...saved.filter((model) => !connected.has(model) || checked.includes(model)),
    ...checked.filter((model) => !saved.includes(model)),
  ];
  const group = { models, text };
  return index === null ? [...groups, group] : groups.map((other, at) => (at === index ? group : other));
}
