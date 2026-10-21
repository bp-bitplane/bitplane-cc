/**
 * PROTOTYPE (#37), throwaway: the floating bar that cycles the Markdown
 * preview variants. Dev builds only. ← / → also cycle while nothing
 * editable (inputs, the terminal) has focus.
 */
import { ChevronLeft, ChevronRight } from "lucide-react";
import { useEffect } from "react";
import styles from "./MdVariantSwitcher.module.css";
import {
  MD_VARIANT_KEYS,
  MD_VARIANTS,
  type MdVariant,
  setMdVariant,
  useMdVariant,
} from "./mdVariant";

export function MdVariantSwitcher() {
  const variant = useMdVariant();

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
      if (isEditable(document.activeElement)) return;
      setMdVariant(step(variant, event.key === "ArrowLeft" ? -1 : 1));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [variant]);

  if (!import.meta.env.DEV) return null;
  return (
    <div className={styles.bar} role="toolbar" aria-label="Markdown preview prototype">
      <button
        type="button"
        aria-label="Previous variant"
        onClick={() => setMdVariant(step(variant, -1))}
      >
        <ChevronLeft size={14} />
      </button>
      <span className={styles.label}>
        <b>MD prototype {variant}</b> {MD_VARIANTS[variant]}
      </span>
      <button
        type="button"
        aria-label="Next variant"
        onClick={() => setMdVariant(step(variant, 1))}
      >
        <ChevronRight size={14} />
      </button>
    </div>
  );
}

function step(variant: MdVariant, by: number): MdVariant {
  const count = MD_VARIANT_KEYS.length;
  const index = (MD_VARIANT_KEYS.indexOf(variant) + by + count) % count;
  return MD_VARIANT_KEYS[index] ?? "A";
}

function isEditable(element: Element | null): boolean {
  if (!element || element === document.body) return false;
  return (
    element instanceof HTMLInputElement ||
    element instanceof HTMLTextAreaElement ||
    element instanceof HTMLSelectElement ||
    (element instanceof HTMLElement && element.isContentEditable) ||
    element.closest("[contenteditable], .wterm, [data-terminal]") !== null
  );
}
