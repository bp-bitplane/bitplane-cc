// PROTOTYPE smoke test: every variant renders in every scenario.
import { act, render, screen } from "@testing-library/react";
import { SCENARIOS, setScenario } from "./model";
import { VariantA, VariantB, VariantC } from "./variants";

describe.each([["A", VariantA], ["B", VariantB], ["C", VariantC]] as const)("variant %s", (_, V) => {
  it.each(SCENARIOS)("%s", (scenario) => {
    act(() => setScenario(scenario));
    render(<V />);
    expect(screen.getAllByText(/MacBook|Settings/).length).toBeGreaterThan(0);
    if (scenario === "pending" && _ !== "A") expect(screen.getAllByText(/restart the daemon/).length).toBeGreaterThan(0);
  });
});
