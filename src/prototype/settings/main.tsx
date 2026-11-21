// PROTOTYPE (bitplane-cc#70 cc Settings view): three variants of the Settings view next to cc's
// real sidebar, switchable via ?variant=A|B|C and ?scenario=…; fake data, no backend. Throwaway.
import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import "../../theme.css";
import appStyles from "../../App.module.css";
import { Sidebar } from "../../sidebar/Sidebar";
import { mockBackend, plane, planesModel, seed, session, sessionsById } from "../../test/fixtures";
import { SCENARIOS, type Scenario, setScenario, useSettings } from "./model";
import s from "./proto.module.css";
import { VariantA, VariantB, VariantC, VariantD, VariantE } from "./variants";

mockBackend();
seed({
  planes: planesModel([plane("alpha"), plane("beta"), plane("gamma")]),
  sessions: sessionsById(
    session(1, "alpha", { shellName: "zsh" }),
    session(2, "alpha", { name: "refactor auth" }),
    session(3, "beta", { name: "docs" }),
  ),
  selectedPlaneId: "alpha",
  daemon: { kind: "connected" } as never,
});

const VARIANTS = [
  { key: "A", name: "Modal · left nav · Save", C: VariantA },
  { key: "B", name: "Page tab · table · save per row", C: VariantB },
  { key: "C", name: "Side panel · TOML text", C: VariantC },
  { key: "D", name: "B with a plain table, click to edit", C: VariantD },
  { key: "E", name: "A with a plain table, click to edit, Save", C: VariantE },
];

function useParam(name: string, fallback: string): [string, (v: string) => void] {
  const [v, setV] = useState(() => new URLSearchParams(location.search).get(name) ?? fallback);
  const set = (nv: string) => {
    const u = new URL(location.href);
    u.searchParams.set(name, nv);
    history.replaceState(null, "", u);
    setV(nv);
  };
  return [v, set];
}

function Root() {
  const [variant, setVariant] = useParam("variant", "A");
  const [scenario, setScen] = useParam("scenario", "clean");
  const st = useSettings();
  useEffect(() => setScenario(scenario as Scenario), [scenario]);
  const i = Math.max(0, VARIANTS.findIndex((v) => v.key === variant));
  const cycle = (d: number) => setVariant(VARIANTS[(i + d + VARIANTS.length) % VARIANTS.length].key);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (t.closest("input, textarea, select, [contenteditable]")) return;
      if (e.key === "ArrowLeft") cycle(-1);
      if (e.key === "ArrowRight") cycle(1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
  const V = VARIANTS[i].C;
  return (
    <div className={appStyles.app}>
      <Sidebar />
      <V key={variant} />
      <div className={s.switcher}>
        <button type="button" onClick={() => cycle(-1)}>
          ←
        </button>
        <b>
          {VARIANTS[i].key} ({VARIANTS[i].name})
        </b>
        <button type="button" onClick={() => cycle(1)}>
          →
        </button>
        <select value={scenario} onChange={(e) => setScen(e.target.value)}>
          {SCENARIOS.map((x) => (
            <option key={x}>{x}</option>
          ))}
        </select>
      </div>
      {st.log.length > 0 && (
        <div className={s.log}>
          {st.log.slice(-4).map((l, n) => (
            <div key={`${n}-${l}`}>{l}</div>
          ))}
        </div>
      )}
    </div>
  );
}

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <Root />
  </StrictMode>,
);
