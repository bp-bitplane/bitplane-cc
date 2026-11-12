import { Bot, SquareTerminal } from "lucide-react";
import styles from "./KindIcon.module.css";

/** What a session runs: a terminal (`null`), Claude, or another agent kind. */
export function KindIcon({ kind, size = 14 }: { kind: string | null; size?: number }) {
  if (kind === null) {
    return <SquareTerminal size={size} className={styles.icon} data-kind="terminal" aria-hidden />;
  }
  if (kind === "claude") return <ClaudeIcon size={size} />;
  return <Bot size={size} className={styles.icon} data-kind="agent" aria-hidden />;
}

function ClaudeIcon({ size }: { size: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2.4}
      strokeLinecap="round"
      className={`${styles.icon} ${styles.claude}`}
      data-kind="claude"
      aria-hidden
    >
      <path d="M12 9.5V2M13.8 10.2l3.9-3.9M14.5 12H22M13.8 13.8l4.2 4.2M12 14.5V22M10.2 13.8l-3.9 3.9M9.5 12H2M10.2 10.2L6 6" />
    </svg>
  );
}
