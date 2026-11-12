import { Bot, SquareTerminal } from "lucide-react";
import type { ReactNode } from "react";
import styles from "./KindIcon.module.css";

/** What a session runs: a terminal (`null`), Claude, or another agent kind. */
export function KindIcon({ kind }: { kind: string | null }) {
  if (kind === null) {
    return <SquareTerminal size={SIZE} className={styles.icon} data-kind="terminal" aria-hidden />;
  }
  return agentKind(kind).icon;
}

/** An agent kind as people call it. */
export function agentKindName(kind: string): string {
  return agentKind(kind).name;
}

const SIZE = 14;

/** The agent kinds cc knows by name and icon; others get their id and a generic icon. */
const AGENT_KINDS: Record<string, { name: string; icon: ReactNode }> = {
  claude: { name: "Claude", icon: <ClaudeIcon /> },
};

function agentKind(kind: string): { name: string; icon: ReactNode } {
  return (
    AGENT_KINDS[kind] ?? {
      name: kind,
      icon: <Bot size={SIZE} className={styles.icon} data-kind="agent" aria-hidden />,
    }
  );
}

function ClaudeIcon() {
  return (
    <svg
      width={SIZE}
      height={SIZE}
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
