// yusAgent (YA-10, ticket 24, PR I) : bouchon visuel. Le fichier d'Unharnessed du même nom
// (157dcdc1d, vscode/webview/) dessine ce composant ; le visuel de yusAgent viendra plus tard.
// Mêmes exports, rien de dessiné.

function initial(name: string): string {
  return (name.trim()[0] ?? "?").toUpperCase();
}

export function ProviderLogo({ name }: { provider: string; name: string; local?: boolean }) {
  return (
    <span className="logo" aria-hidden="true">
      {initial(name)}
    </span>
  );
}

export function ProviderAvatar({
  provider,
  name,
  className,
  local,
}: {
  provider: string;
  name: string;
  className: string;
  local?: boolean;
}) {
  return (
    <span className={className}>
      <ProviderLogo provider={provider} name={name} local={local} />
    </span>
  );
}

export function ServiceAvatar({ name, className }: { service: string; name: string; className: string }) {
  return (
    <span className={className}>
      <span className="logo" aria-hidden="true">
        {initial(name)}
      </span>
    </span>
  );
}

export function McpLogo() {
  return (
    <span className="logo" aria-hidden="true">
      MCP
    </span>
  );
}
