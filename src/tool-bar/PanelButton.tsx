import type { ReactNode } from "react";
import styles from "./PanelButton.module.css";

export function PanelActions({ children }: { children: ReactNode }) {
  return <span className={styles.actions}>{children}</span>;
}

export function PanelButton({
  title,
  onClick,
  children,
}: {
  title: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      className={styles.iconButton}
      title={title}
      aria-label={title}
      onClick={onClick}
    >
      {children}
    </button>
  );
}
