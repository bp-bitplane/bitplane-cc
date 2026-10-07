import { TriangleAlert } from "lucide-react";
import type { PlaneRow } from "../ipc";
import styles from "./Badges.module.css";

export function PlaneBadges({ plane, sessionCount }: { plane: PlaneRow; sessionCount?: number }) {
  return (
    <span className={styles.badges}>
      {sessionCount ? (
        <span className={styles.count} title={`${sessionCount} open sessions`}>
          {sessionCount}
        </span>
      ) : null}
      {plane.findings.length > 0 && (
        <span
          className={styles.warn}
          title={`bp list reported: ${plane.findings.join(", ")}`}
          role="img"
          aria-label="findings"
        >
          <TriangleAlert size={13} />
        </span>
      )}
      {plane.kind === "gone" && <span className={styles.gone}>gone</span>}
      {plane.kind === "draft" && (
        <span className={styles.draft} title="No plane.toml yet: run bp create --here inside it">
          draft
        </span>
      )}
      {plane.kind === "creating" && <span className={styles.creating}>creating…</span>}
    </span>
  );
}
