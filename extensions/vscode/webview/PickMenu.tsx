import { Fragment, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { Popover } from "./Popover";

/**
 * A row of the menu, under its group's title: its icon, its name, then what
 * is said of it, in a column. `detail` fills a band at the bottom while it is
 * highlighted.
 */
export type Pick = { key: string; group?: string; icon: ReactNode; name: ReactNode; about?: ReactNode; detail?: ReactNode };

/**
 * The composer's `/` and `@` menus: a row per entry, the highlighted one
 * under a pill that slides from row to row, its icon lighting up as it
 * lands. The keys stay in the message box: it moves the highlight and picks.
 */
export function PickMenu({
  className,
  trigger,
  label,
  picks,
  empty,
  highlight,
  onHighlight,
  onPick,
  onClose,
}: {
  className: string;
  trigger: string;
  label: string;
  picks: Pick[];
  empty: string;
  highlight: number;
  onHighlight: (at: number) => void;
  onPick: (at: number) => void;
  onClose: () => void;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  /** Where the pill sits; `lands` counts its moves, each lights the icon again. */
  const [cursor, setCursor] = useState<{ top: number; height: number; lands: number } | null>(null);

  const row = () => scroller.current?.querySelector<HTMLElement>(`[data-at="${highlight}"]`);
  // Braces: VS Code's Chromium returns a promise from scrollIntoView, which React would call as the cleanup.
  useLayoutEffect(() => {
    row()?.scrollIntoView({ block: "nearest" });
  }, [highlight]);
  // After each render: the rows under the pill may have changed.
  useLayoutEffect(() => {
    const at = row();
    if (!at) return;
    const { offsetTop: top, offsetHeight: height } = at;
    setCursor((was) =>
      was && was.top === top && was.height === height ? was : { top, height, lands: (was?.lands ?? 0) + 1 },
    );
  });

  if (picks.length === 0) {
    return (
      <Popover className={`menu pick-menu ${className}`} trigger={trigger} onClose={onClose}>
        <p className="menu-empty">{empty}</p>
      </Popover>
    );
  }
  const detail = picks[Math.min(highlight, picks.length - 1)].detail;
  return (
    <Popover className={`menu pick-menu ${className}`} trigger={trigger} onClose={onClose}>
      <div className="pick-scroll" ref={scroller}>
        {cursor && (
          <div
            className={cursor.lands === 1 ? "pick-cursor is-placed" : "pick-cursor"}
            style={{ translate: `0 ${cursor.top}px`, height: cursor.height }}
            aria-hidden="true"
          />
        )}
        <ul className="pick-list" role="listbox" aria-label={label}>
          {picks.map((pick, at) => (
            <Fragment key={pick.key}>
              {pick.group && pick.group !== picks[at - 1]?.group && (
                <li className="menu-title" role="presentation">
                  {pick.group}
                </li>
              )}
              {/* Not a button: a button lays out no subgrid, and the keys stay in the message box anyway. */}
              <li
                className={at === highlight ? "pick-row is-active" : "pick-row"}
                role="option"
                aria-selected={at === highlight}
                data-at={at}
                // The message box keeps the focus, and its caret.
                onMouseDown={(event) => event.preventDefault()}
                onMouseEnter={() => onHighlight(at)}
                onClick={() => onPick(at)}
              >
                {/* A new key as the pill lands: the icon lights up again. */}
                <span className="pick-icon" key={at === highlight ? cursor?.lands : "rest"}>
                  {pick.icon}
                </span>
                <span className="pick-name">{pick.name}</span>
                {pick.about !== undefined && <span className="pick-about">{pick.about}</span>}
              </li>
            </Fragment>
          ))}
        </ul>
      </div>
      {detail && <div className="pick-detail">{detail}</div>}
    </Popover>
  );
}
