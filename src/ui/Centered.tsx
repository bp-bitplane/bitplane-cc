import type { ReactNode } from "react";
import styles from "./Centered.module.css";

export function Centered({
  title,
  children,
  actions,
}: {
  title: string;
  children?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <div className={styles.centered}>
      <h2>{title}</h2>
      {children}
      {actions && <div className={styles.actions}>{actions}</div>}
    </div>
  );
}
