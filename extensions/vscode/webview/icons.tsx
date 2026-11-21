// Our icons, drawn by hand, never taken from an icon pack: 24 grid, stroke
// 1.25, round caps and joins, no fill, `currentColor`, 16 px, as few nodes as
// possible. No AI clichés: no sparkles, magic wands, robot heads or brains.
import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement>;

function Icon({ children, ...props }: IconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={16}
      height={16}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.25}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...props}
    >
      {children}
    </svg>
  );
}

/** A circle closing clockwise from 3 o'clock, arrowhead at the gap. */
export function IconRestart(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M18.5 12A6.5 6.5 0 1 1 16.6 7.4" />
      <path d="M16.08 4.45L16.6 7.4L13.65 6.88" />
    </Icon>
  );
}

/** A folder: its tab on the top left. */
export function IconFolder(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4.5 7.5A1.5 1.5 0 0 1 6 6h3.4a1 1 0 0 1 .7.3l1.6 1.6a1 1 0 0 0 .7.3H18a1.5 1.5 0 0 1 1.5 1.5v7.8A1.5 1.5 0 0 1 18 19H6a1.5 1.5 0 0 1-1.5-1.5z" />
    </Icon>
  );
}

/** Two corners closing in on the middle: the context compacted. */
export function IconCompact(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M5 10h5V5M19 14h-5v5" />
    </Icon>
  );
}

/** An open book: a skill, what the agent reads to know how. */
export function IconBook(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M12 7.5c-1.75-1.4-4.25-1.9-7-1.6v11.6c2.75-.3 5.25.2 7 1.6 1.75-1.4 4.25-1.9 7-1.6V5.9c-2.75-.3-5.25.2-7 1.6zM12 7.5v11.6" />
    </Icon>
  );
}

/** A screen on its foot: this machine. */
export function IconComputer(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="4" y="5" width="16" height="11" rx="2.5" />
      <path d="M12 16v3.5M8.5 19.5h7" />
    </Icon>
  );
}

export function IconPlus(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M12 6.5v11M6.5 12h11" />
    </Icon>
  );
}

export function IconCross(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M8.5 8.5l7 7M15.5 8.5l-7 7" />
    </Icon>
  );
}

export function IconBack(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M14.5 6.5L9 12l5.5 5.5" />
    </Icon>
  );
}

export function IconChevron(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M9.5 6.5L15 12l-5.5 5.5" />
    </Icon>
  );
}

/** A frame left open at the top right, an arrow leaving through the gap. */
export function IconExternal(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M13.5 5.5h5v5M18.5 5.5l-7 7" />
      <path d="M16.5 13.5v4a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1v-9a1 1 0 0 1 1-1h4" />
    </Icon>
  );
}

/** Two sheets, the back one showing only its corner. */
export function IconCopy(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="9" y="9" width="9.5" height="9.5" rx="1.5" />
      <path d="M15 9V6.5a1 1 0 0 0-1-1H6.5a1 1 0 0 0-1 1V14a1 1 0 0 0 1 1H9" />
    </Icon>
  );
}

export function IconCheck(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M6.5 12.5l3.5 3.5 7.5-8" />
    </Icon>
  );
}

/** A chain link with its middle bar gone, two ticks marking the break. */
export function IconUnlink(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10 8H8a4 4 0 0 0 0 8h2M14 8h2a4 4 0 0 1 0 8h-2" />
      <path d="M12 4.5v1.5M12 18v1.5" />
    </Icon>
  );
}

export function IconSearch(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="11" cy="11" r="5.5" />
      <path d="M15.5 15.5L19 19" />
    </Icon>
  );
}

/** A plug: two prongs, a rounded body, the cord. */
export function IconPlug(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M9.5 4.5v4M14.5 4.5v4" />
      <path d="M7 8.5h10V11a5 5 0 0 1-10 0z" />
      <path d="M12 16v3.5" />
    </Icon>
  );
}

export function IconChevronUp(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M7.5 14l4.5-4.5 4.5 4.5" />
    </Icon>
  );
}

/** A window with a prompt: the chevron, then the cursor. For the background commands. */
export function IconTerminal(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="3.5" y="5.5" width="17" height="13" rx="3" />
      <path d="M7.5 10l2.5 2.5-2.5 2.5M12.5 15h4" />
    </Icon>
  );
}

/**
 * Python's two snakes, head to tail: one S down the middle, a wing out of
 * each head, an eye in each. For the REPL's variables and the cells the agent runs.
 */
export function IconPython(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M8.5 8.5V6a2.5 2.5 0 0 1 2.5-2.5h2A2.5 2.5 0 0 1 15.5 6v4a2 2 0 0 1-2 2h-3a2 2 0 0 0-2 2v4a2.5 2.5 0 0 0 2.5 2.5h2a2.5 2.5 0 0 0 2.5-2.5v-2.5" />
      <path d="M12 8.5H6A2.5 2.5 0 0 0 3.5 11v2A2.5 2.5 0 0 0 6 15.5h2.5M12 15.5h6a2.5 2.5 0 0 0 2.5-2.5v-2A2.5 2.5 0 0 0 18 8.5h-2.5" />
      <path d="M11 6.2h.01M13 17.8h.01" strokeWidth={2} />
    </Icon>
  );
}

export function IconChevronDown(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M7.5 10l4.5 4.5 4.5-4.5" />
    </Icon>
  );
}

/** An arrow pointing up, for send. */
export function IconSend(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M12 18.5V6M6.5 11.5L12 6l5.5 5.5" />
    </Icon>
  );
}

/** A pencil, its tip down on the left: change what was written. */
export function IconPencil(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M5.5 18.5l.9-3.6L15.6 5.7a1.7 1.7 0 0 1 2.4 2.4L8.8 17.3z" />
      <path d="M14 7.3l2.4 2.4" />
    </Icon>
  );
}

/** An eye, open: read what is there. */
export function IconEye(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M3.5 12c2-3.7 4.9-5.5 8.5-5.5s6.5 1.8 8.5 5.5c-2 3.7-4.9 5.5-8.5 5.5S5.5 15.7 3.5 12z" />
      <circle cx="12" cy="12" r="2.5" />
    </Icon>
  );
}

/**
 * Braces around a dot: the kernel's variables. Its braces (`.draw`, each
 * one path long) and its dot (`.spark`) apart, for a light to draw the one
 * and light the other (styles.css, .kernel-line).
 */
export function IconVariables(props: IconProps) {
  return (
    <Icon {...props}>
      <path className="draw" pathLength={1} d="M8.5 4.5H8a2 2 0 0 0-2 2v3.5l-1.5 1.5 1.5 1.5v3.5a2 2 0 0 0 2 2h.5" />
      <path className="draw" pathLength={1} d="M15.5 4.5h.5a2 2 0 0 1 2 2v3.5l1.5 1.5-1.5 1.5v3.5a2 2 0 0 1-2 2h-.5" />
      <path className="spark" d="M12 12h.01" strokeWidth={2.5} />
    </Icon>
  );
}

/** A page, its corner folded, a plus on it: write a file. */
export function IconFilePlus(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M13.5 3.5H8A2.5 2.5 0 0 0 5.5 6v12A2.5 2.5 0 0 0 8 20.5h8a2.5 2.5 0 0 0 2.5-2.5V8.5z" />
      <path d="M13.5 3.5v5h5M12 11.5v5M9.5 14h5" />
    </Icon>
  );
}

/** A capital T: the model reads text. */
export function IconText(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M6.5 6.5h11M12 6.5v11" />
    </Icon>
  );
}

/** A frame, a hill and a sun: the model reads images. */
export function IconImage(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="4.5" y="5.5" width="15" height="13" rx="1.5" />
      <path d="M4.5 16l4.5-4.5 4 4 2-2 4.5 4.5" />
      <circle cx="15.25" cy="9.75" r="1.25" />
    </Icon>
  );
}

/** A lightning bolt: the fast mode. */
export function IconBolt(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M13.5 4L7 13h5l-1.5 7L17 11h-5z" />
    </Icon>
  );
}

/** Two sliders, their knobs set apart: the settings. */
export function IconSliders(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M5 8.5h5M14 8.5h5M5 15.5h9M18 15.5h1" />
      <circle cx="12" cy="8.5" r="2" />
      <circle cx="16" cy="15.5" r="2" />
    </Icon>
  );
}

/** A soft square, for stop. */
export function IconStop(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="7.5" y="7.5" width="9" height="9" rx="2" />
    </Icon>
  );
}

/** A speech bubble with a plus: a new conversation. */
export function IconNewChat(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M6 5.5h12a1.5 1.5 0 0 1 1.5 1.5v8a1.5 1.5 0 0 1-1.5 1.5h-7.5L7 19.5v-3H6A1.5 1.5 0 0 1 4.5 15V7A1.5 1.5 0 0 1 6 5.5z" />
      <path d="M12 8.5v5M9.5 11h5" />
    </Icon>
  );
}

/** An arrow pointing down, to the latest message. */
export function IconArrowDown(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M12 6v12.5M6.5 13L12 18.5l5.5-5.5" />
    </Icon>
  );
}

/** A clock whose rim turns back, arrowhead at the gap: the conversations before. */
export function IconHistory(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M5.5 12A6.5 6.5 0 1 0 7.4 7.4" />
      <path d="M7.92 4.45L7.4 7.4L10.35 6.88" />
      <path d="M12 9v3l2 1.5" />
    </Icon>
  );
}

/** A bin under its lid: delete. */
export function IconTrash(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M5.5 7.5h13M10 7.5V6h4v1.5" />
      <path d="M7.5 7.5l.75 10.5h7.5l.75-10.5" />
    </Icon>
  );
}

const CLOTH_UP = "M6 4.5C10 2.5 13 6.5 18.5 4.5V12C13 14 10 10 6 12Z";
const CLOTH_DOWN = "M6 4.5C10 6.5 13 2.5 18.5 4.5V12C13 10 10 14 6 12Z";

/** A flag on its pole, its cloth waving slowly: a goal under way. The `hoist` group goes up the pole. */
export function IconFlag(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M6 21V3.5" />
      <g className="hoist">
        <path className="cloth" d={CLOTH_UP}>
          <animate
            attributeName="d"
            values={`${CLOTH_UP};${CLOTH_DOWN};${CLOTH_UP}`}
            calcMode="spline"
            keyTimes="0;0.5;1"
            keySplines="0.45 0 0.55 1;0.45 0 0.55 1"
            dur="2.6s"
            repeatCount="indefinite"
          />
        </path>
      </g>
    </Icon>
  );
}

/** Two columns of three dots: a handle to drag by. */
export function IconGrip(props: IconProps) {
  return (
    <Icon strokeWidth={2.2} {...props}>
      <path d="M9.5 6.5h0M14.5 6.5h0M9.5 12h0M14.5 12h0M9.5 17.5h0M14.5 17.5h0" />
    </Icon>
  );
}

/** A soft triangle pointing right: play again. */
export function IconPlay(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M8.5 6.8v10.4a.6.6 0 0 0 .9.5l8-5.2a.6.6 0 0 0 0-1l-8-5.2a.6.6 0 0 0-.9.5z" />
    </Icon>
  );
}
