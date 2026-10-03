/** Defines the six assignments of comparison, files and history to the workbench's three slots. */
export type WorkbenchPanel = "comparison" | "files" | "history";
export const workbenchLayouts = [
  { id: "comparison-files-history", top: "comparison", left: "files", right: "history", label: "Comparison above, files left" },
  { id: "comparison-history-files", top: "comparison", left: "history", right: "files", label: "Comparison above, history left" },
  { id: "files-comparison-history", top: "files", left: "comparison", right: "history", label: "Files above, comparison left" },
  { id: "files-history-comparison", top: "files", left: "history", right: "comparison", label: "Files above, history left" },
  { id: "history-files-comparison", top: "history", left: "files", right: "comparison", label: "History above, files left" },
  { id: "history-comparison-files", top: "history", left: "comparison", right: "files", label: "History above, comparison left" },
] as const satisfies readonly { id: string; top: WorkbenchPanel; left: WorkbenchPanel; right: WorkbenchPanel; label: string }[];
export type WorkbenchLayoutId = typeof workbenchLayouts[number]["id"];
export const defaultWorkbenchLayout: WorkbenchLayoutId = "comparison-files-history";
