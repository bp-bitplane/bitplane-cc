/**
 * A Markdown file rendered as a document, lazy-loaded like `DiffView`.
 * react-markdown renders React elements only, so raw HTML stays escaped text.
 * Fenced code highlights through `DiffView`'s shared Shiki. No click ever
 * navigates the webview: `#anchor`s scroll the preview, plane links open a
 * File tab, and anything with a scheme only shows its URL.
 */
import { Image as ImageIcon } from "lucide-react";
import {
  isValidElement,
  type MouseEvent,
  type ReactNode,
  type RefObject,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import Markdown, { type Components } from "react-markdown";
import rehypeSlug from "rehype-slug";
import remarkGfm from "remark-gfm";
import { commands } from "../ipc";
import { openFileTab } from "../store/store";
import { folderOf, resolvePlaneLink } from "../tabs/fileTabs";
import { highlightCode } from "./DiffView";
import styles from "./MarkdownView.module.css";

export default function MarkdownView({
  planeId,
  path,
  source,
}: {
  planeId: string;
  /** The file's plane path; relative links resolve against its folder. */
  path: string;
  source: string;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const components = useMemo<Components>(
    () => ({
      pre: ({ children }) => <CodeBlock {...codeOf(children)} />,
      a: ({ node: _, href, children, ...props }) => (
        <a
          {...props}
          href={href}
          title={href !== undefined && hasScheme(href) ? href : undefined}
          onClick={(event) => followLink(event, href, { planeId, path, scrollRef })}
        >
          {children}
        </a>
      ),
      img: ({ src, alt }) => <Picture src={typeof src === "string" ? src : ""} alt={alt ?? ""} />,
    }),
    [planeId, path],
  );

  return (
    <div ref={scrollRef} className={styles.scroll}>
      <article className={styles.markdown}>
        <Markdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeSlug]} components={components}>
          {source}
        </Markdown>
      </article>
    </div>
  );
}

/** Remote images load; anything else would be a request the webview can't serve. */
function Picture({ src, alt }: { src: string; alt: string }) {
  if (/^https?:\/\//i.test(src)) return <img src={src} alt={alt} />;
  return (
    <span className={styles.chip} role="img" aria-label={alt || src} title={src}>
      <ImageIcon size={12} aria-hidden />
      {alt && <span>{alt}</span>}
      <span className={styles.chipPath}>{src}</span>
    </span>
  );
}

type Token = { content: string; color?: string; fontStyle?: number };

/** Plain text first, highlighted once the grammar loads. */
function CodeBlock({ code, lang }: { code: string; lang: string }) {
  const [tokens, setTokens] = useState<Token[][] | null>(null);

  useEffect(() => {
    let current = true;
    setTokens(null);
    void highlightCode(code, lang).then((result) => {
      if (current) setTokens(result);
    });
    return () => {
      current = false;
    };
  }, [code, lang]);

  return (
    <pre className={styles.pre} data-lang={lang || undefined}>
      <code>
        {tokens
          ? tokens.map((line, i) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: lines have no identity
              <span key={i}>
                {line.map((token, j) => (
                  // biome-ignore lint/suspicious/noArrayIndexKey: tokens have no identity
                  <span key={j} style={{ color: token.color, fontStyle: italic(token) }}>
                    {token.content}
                  </span>
                ))}
                {i < tokens.length - 1 && "\n"}
              </span>
            ))
          : code}
      </code>
    </pre>
  );
}

/** The `<code>` react-markdown puts inside `<pre>`: its text and `language-x` class. */
function codeOf(children: ReactNode): { code: string; lang: string } {
  if (!isValidElement<{ className?: string; children?: ReactNode }>(children)) {
    return { code: String(children ?? ""), lang: "" };
  }
  const lang = /language-(\S+)/.exec(children.props.className ?? "")?.[1] ?? "";
  const code = String(children.props.children ?? "").replace(/\n$/, "");
  return { code, lang };
}

/** Shiki's FontStyle bit flags: 1 is italic. */
function italic(token: Token): "italic" | undefined {
  return token.fontStyle && token.fontStyle & 1 ? "italic" : undefined;
}

function followLink(
  event: MouseEvent,
  href: string | undefined,
  from: { planeId: string; path: string; scrollRef: RefObject<HTMLDivElement | null> },
) {
  event.preventDefault();
  if (href === undefined) return;
  if (href.startsWith("#")) {
    scrollToAnchor(from.scrollRef.current, href.slice(1));
    return;
  }
  const target = resolvePlaneLink(from.path, href);
  if (target !== null) void openLinkedFile(from.planeId, target);
}

/** The id as written, then with react-markdown's `user-content-` clobber prefix. */
function scrollToAnchor(scroll: HTMLElement | null, fragment: string) {
  if (!scroll) return;
  const id = decoded(fragment);
  const target = byId(scroll, id) ?? byId(scroll, `user-content-${id}`);
  target?.scrollIntoView({ behavior: "smooth", block: "start" });
}

/** A missing file still opens, to its error state; a folder doesn't. */
async function openLinkedFile(planeId: string, path: string) {
  const folder = folderOf(path);
  const name = folder === "" ? path : path.slice(folder.length + 1);
  const entries = await commands.planeListDir(planeId, folder).catch(() => []);
  if (entries.some((entry) => entry.name === name && entry.kind === "directory")) return;
  openFileTab(planeId, path, "file");
}

/** Within this preview only: other File tabs' previews may share ids. */
function byId(scroll: HTMLElement, id: string): Element | undefined {
  return Array.from(scroll.querySelectorAll("[id]")).find((element) => element.id === id);
}

function hasScheme(href: string): boolean {
  return /^([a-z][a-z\d+.-]*:|\/\/)/i.test(href);
}

function decoded(fragment: string): string {
  try {
    return decodeURIComponent(fragment);
  } catch {
    return fragment;
  }
}
