import {
  createContext,
  memo,
  useContext,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentProps,
  type ReactNode,
} from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { IconCheck, IconCopy } from "./icons";
import { post } from "./vscode";
import { Wave } from "./wave";

const PLUGINS = [remarkGfm];
const STREAMING = [rehypeTokens];

const COMPONENTS: Components = {
  // Web pages open in the browser; nothing else opens from a message.
  a: ({ href, children }) => (
    <a
      href={href}
      title={href}
      onClick={(event) => {
        event.preventDefault();
        if (href) post({ type: "openLink", url: href });
      }}
    >
      {children}
    </a>
  ),
  pre: ({ children }) => <CodeBlock>{children}</CodeBlock>,
};

/** While the text streams: its words, which the wave brings in. */
const STREAMING_COMPONENTS: Components = {
  ...COMPONENTS,
  span: ({ node: _, ...props }) => (props.className === "md-token" ? <Token {...props} /> : <span {...props} />),
};

/**
 * The agent's text: Markdown with GitHub's tables, task lists and
 * strikethrough; raw HTML stays text. While it is `streaming`, its words
 * come in behind one front moving at the pace the text arrives (Wave).
 */
export const Markdown = memo(function Markdown({ text, streaming = false }: { text: string; streaming?: boolean }) {
  const wave = useRef<Wave>(undefined);
  if (streaming) wave.current ??= new Wave();
  // After the words of this update mounted (their effects run first).
  useLayoutEffect(() => {
    if (streaming) wave.current?.flush();
  });
  return (
    <div className="md">
      <WaveContext.Provider value={streaming ? wave.current : undefined}>
        <ReactMarkdown
          remarkPlugins={PLUGINS}
          rehypePlugins={streaming ? STREAMING : undefined}
          components={streaming ? STREAMING_COMPONENTS : COMPONENTS}
        >
          {text}
        </ReactMarkdown>
      </WaveContext.Provider>
    </div>
  );
});

const WaveContext = createContext<Wave | undefined>(undefined);

/** A streaming word: handed to the wave when it mounts, so only new words move. */
function Token(props: ComponentProps<"span">) {
  const wave = useContext(WaveContext);
  const ref = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    if (ref.current) wave?.add(ref.current);
  }, []);
  return <span ref={ref} {...props} />;
}

/** The HTML tree react-markdown renders, the parts read here. */
type HtmlNode = { type: string; value?: string; tagName?: string; properties?: object; children?: HtmlNode[] };

/**
 * Each word in its own `.md-token` span, code left whole. The spans are keyed
 * by their place (react-markdown's `span-0`, `span-1`…), so the words already
 * there stay put and only the new ones mount, and come in.
 */
function rehypeTokens() {
  return (tree: HtmlNode) => split(tree);
}

function split(node: HtmlNode): void {
  if (!node.children) return;
  const children: HtmlNode[] = [];
  for (const child of node.children) {
    if (child.type === "text" && child.value) {
      for (const part of child.value.split(/(\s+)/)) {
        if (!part) continue;
        if (/^\s/.test(part)) children.push({ type: "text", value: part });
        else
          children.push({
            type: "element",
            tagName: "span",
            properties: { className: ["md-token"] },
            children: [{ type: "text", value: part }],
          });
      }
      continue;
    }
    if (child.type === "element" && child.tagName !== "pre" && child.tagName !== "code") split(child);
    children.push(child);
  }
  node.children = children;
}

/** A fenced block, under a band with its language (when the model gave one) and a copy button. */
function CodeBlock({ children }: { children: ReactNode }) {
  const code = codeOf(children);
  const language = /language-(\S+)/.exec(code.className)?.[1];
  return (
    <div className="md-code">
      <div className="md-code-head">
        <span className="md-code-language">{language}</span>
        <CopyButton className="md-code-copy" text={code.text} />
      </div>
      <pre>{children}</pre>
    </div>
  );
}

/** Copies `text`, then says so a moment. */
function CopyButton({ className, text }: { className: string; text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      className={`icon-button ${className}`}
      type="button"
      aria-label="Copy"
      data-tip={copied ? "Copied" : "Copy"}
      data-tip-side="left"
      onClick={() => {
        post({ type: "copy", text });
        setCopied(true);
        setTimeout(() => setCopied(false), 1400);
      }}
    >
      {copied ? <IconCheck /> : <IconCopy />}
    </button>
  );
}

/** The `<code>` inside a `<pre>`: its class (the language) and its text. */
function codeOf(children: ReactNode): { className: string; text: string } {
  const child = Array.isArray(children) ? children[0] : children;
  if (child && typeof child === "object" && "props" in child) {
    const props = child.props as { className?: string; children?: ReactNode };
    return { className: props.className ?? "", text: String(props.children ?? "").replace(/\n$/, "") };
  }
  return { className: "", text: "" };
}
