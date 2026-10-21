/**
 * PROTOTYPE (#37): a Markdown file rendered as a document, lazy-loaded like
 * `DiffView`. react-markdown renders React elements only, so raw HTML stays
 * escaped text (#38). Fenced code highlights through `DiffView`'s shared Shiki.
 */
import { isValidElement, type ReactNode, useEffect, useState } from "react";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { highlightCode } from "./DiffView";
import styles from "./MarkdownView.module.css";

export default function MarkdownView({ source, note }: { source: string; note?: ReactNode }) {
  return (
    <div className={styles.scroll}>
      {note && <div className={styles.note}>{note}</div>}
      <article className={styles.markdown}>
        <Markdown remarkPlugins={[remarkGfm]} components={COMPONENTS}>
          {source}
        </Markdown>
      </article>
    </div>
  );
}

const COMPONENTS: Components = {
  pre: ({ children }) => <CodeBlock {...codeOf(children)} />,
  // A click would navigate the whole webview away from the app.
  a: ({ href, children }) => (
    <a
      href={href}
      title={href}
      onClick={(event) => {
        event.preventDefault();
        console.info("[prototype #37] link click ignored:", href);
      }}
    >
      {children}
    </a>
  ),
};

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
