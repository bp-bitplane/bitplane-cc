import { type FileTab, fileTabTitle } from "../tabs/fileTabs";
import { Centered } from "../ui/Centered";

const MODES: Record<FileTab["mode"], string> = {
  index: "Staged changes: HEAD ↔ index",
  worktree: "Unstaged changes: index ↔ working tree",
  file: "HEAD ↔ working tree",
};

/** Placeholder until DiffView renders the file. */
export function FileTabContent({ tab }: { tab: FileTab }) {
  return (
    <Centered title={fileTabTitle(tab)}>
      <p>{tab.path}</p>
      <p>{MODES[tab.mode]}</p>
    </Centered>
  );
}
