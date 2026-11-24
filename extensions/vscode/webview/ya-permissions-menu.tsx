// yusAgent (YA-10, ticket 31, X3, décisions C5 et C6 ; C6 du ticket 30) : l'étiquette des permissions
// du compositeur, et son menu à deux choix. Les libellés sont ceux de l'option `permissions` du pont
// (`crates/pa-daemon/src/acp/ya_permissions.rs`) ; un choix part par `session/set_config_option`
// (`configId: "permissions"`), seulement s'il change la valeur. Portée : la conversation, comme
// `/permissions` du TUI ; le réglage global reste hors de l'extension (D1).
// - « auto » : `auto · no sandbox`, en couleur d'avertissement, comme le TUI (ticket 30, C1 option B) ;
// - « manual » : `manual`, discret, pour pouvoir repasser en « auto ».
import type { SessionConfig } from "../src/protocol";
import { IconCheck } from "./icons";
import { Popover } from "./Popover";
import { post } from "./vscode";

const TIPS: Record<string, string> = {
  auto: "Permissions: auto. Tool calls run without asking, with your user's rights (no sandbox).",
  manual: "Permissions: manual. Each tool call asks first.",
};

const LABELS: Record<string, string> = { auto: "auto · no sandbox", manual: "manual" };

export function PermissionsMenu({
  permissions,
  open,
  onToggle,
  onClose,
}: {
  permissions: NonNullable<SessionConfig["permissions"]>;
  open: boolean;
  onToggle: () => void;
  onClose: () => void;
}) {
  const { current, choices } = permissions;
  return (
    <div className="picker ya-permissions-picker">
      {open && (
        <Popover className="menu ya-permissions-menu" trigger="permissions" onClose={onClose}>
          <ul className="menu-list" aria-label="Permissions">
            {choices.map((choice) => (
              <li key={choice.value}>
                <button
                  className="menu-row"
                  type="button"
                  role="menuitemradio"
                  aria-checked={choice.value === current}
                  onClick={() => {
                    if (choice.value !== current) post({ type: "setConfig", configId: "permissions", value: choice.value });
                    onClose();
                  }}
                >
                  <span className="menu-row-name">{choice.name}</span>
                  {choice.value === current && <IconCheck className="menu-check" />}
                </button>
              </li>
            ))}
          </ul>
        </Popover>
      )}
      <button
        className={current === "auto" ? "ya-auto-badge" : "ya-manual-badge"}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        data-menu-trigger="permissions"
        data-tip={open ? undefined : (TIPS[current] ?? `Permissions: ${current}.`)}
        data-tip-align="start"
        onClick={onToggle}
      >
        {LABELS[current] ?? current}
      </button>
    </div>
  );
}
