import { StrictMode, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import type { Graph } from "../src/createGraphNodes";
import { HEIGHT, renderRails } from "../src/react";
import "../src/react/git-graph.css";
import { parseLog } from "../test/fixtures/repos/parse";
import { SHAPES, WIP } from "../test/fixtures/shapes";
import { layout } from "../test/layout";
import "./examples.css";

const repoLogs = import.meta.glob<string>("../test/fixtures/repos/*.log", {
  query: "?raw",
  import: "default",
  eager: true,
});

const EXAMPLES: Record<string, () => Graph> = {
  ...Object.fromEntries(
    Object.entries(SHAPES).map(([name, commits]) => [
      name,
      () => layout(commits, name === "wip" ? { index: WIP } : {}),
    ]),
  ),
  ...Object.fromEntries(
    Object.entries(repoLogs).map(([path, log]) => [
      `repo: ${path.split("/").at(-1)!.replace(".log", "")}`,
      () => layout(parseLog(log)),
    ]),
  ),
};

const App = () => {
  const [name, setName] = useState(Object.keys(EXAMPLES)[0]!);
  const graph = useMemo(() => EXAMPLES[name]!(), [name]);

  return (
    <main>
      <nav>
        {Object.keys(EXAMPLES).map((n) => (
          <button key={n} type="button" aria-pressed={n === name} onClick={() => setName(n)}>
            {n}
          </button>
        ))}
      </nav>
      <ol className="graph">
        {graph.nodes.map((node) => {
          const label = "hash" in node.commit ? node.commit.hash : "WIP";
          return (
            <li key={label} style={{ height: HEIGHT }}>
              {renderRails(node)}
              <code>{label.slice(0, 10)}</code>
              <span className="parents">
                {node.commit.parents.map((p) => p.slice(0, 7)).join(" ")}
              </span>
            </li>
          );
        })}
      </ol>
    </main>
  );
};

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
