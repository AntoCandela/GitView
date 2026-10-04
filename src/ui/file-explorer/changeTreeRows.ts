/** Builds stable logical rows once per data/expansion change, independent of scrolling. */

export interface ChangeTreeDirectory {
  id: string;
  displayPath: string;
  segments: string[];
}

/** Locale-independent status facts; source categories and marker identity never depend on prose. */
export type ChangeStatusFact =
  | { readonly kind: "untracked" | "conflict" | "unchanged" | "unavailable" }
  | { readonly kind: "staged" | "unstaged"; readonly change: "added" | "modified" | "deleted" }
  | { readonly kind: "unsupported"; readonly change: "rename_or_copy" | "submodule" | "type_change" }
  | { readonly kind: "committed"; readonly change: "added" | "modified" | "deleted" | "type_change" };

export interface ChangeStatusCount {
  readonly fact: ChangeStatusFact;
  readonly count: number;
}

export interface ChangeTreeFile extends ChangeTreeDirectory {
  readonly statuses: readonly ChangeStatusFact[];
  marker: string;
  unsupported?: boolean;
}

/** Omit to summarize all files; an explicit array is complete status authority, null is unavailable. */
export interface ChangeTreeSummaryProps {
  summaryFiles?: readonly ChangeTreeFile[] | null;
}

interface Directory {
  id: string;
  displayPath: string;
  segments: string[];
  directories: Map<string, Directory>;
  files: ChangeTreeFile[];
}

export interface ChangeTreeRow {
  key: string;
  depth: number;
  parent: number | null;
  /** Exclusive logical end of this directory, including only expanded descendants. */
  end: number;
  item: ChangeTreeDirectory;
  file?: ChangeTreeFile;
  expanded: boolean;
}

/** Presentation identity uses native segments, never a split display path or file authority. */
export function changeDirectoryId(segments: readonly string[]): string {
  return JSON.stringify(segments);
}

/** Counts every descendant independently of expansion and progressive directory listing. */
export function summarizeDirectoryChanges(files: readonly ChangeTreeFile[]): ReadonlyMap<string, readonly ChangeStatusCount[]> {
  const counts = new Map<string, Map<string, ChangeStatusCount>>();
  for (const file of files) {
    const facts: readonly ChangeStatusFact[] = file.statuses.length ? file.statuses : [{ kind: "unavailable" }];
    for (const fact of facts) {
      if (fact.kind === "unchanged") continue;
      const key = "change" in fact ? `${fact.kind}:${fact.change}` : fact.kind;
      for (let depth = 1; depth < file.segments.length; depth++) {
        const id = changeDirectoryId(file.segments.slice(0, depth));
        let directory = counts.get(id);
        if (!directory) { directory = new Map(); counts.set(id, directory); }
        directory.set(key, { fact, count: (directory.get(key)?.count ?? 0) + 1 });
      }
    }
  }
  return new Map([...counts].map(([id, statuses]) => [
    id, [...statuses].sort(([left], [right]) => left < right ? -1 : left > right ? 1 : 0).map(([, value]) => value),
  ]));
}

export function buildChangeHierarchy(files: ChangeTreeFile[], directories: ChangeTreeDirectory[]) {
  const root: Directory = { id: "", displayPath: "", segments: [], directories: new Map(), files: [] };
  function ensureDirectory(segments: string[]) {
    let directory = root;
    for (let depth = 0; depth < segments.length; depth++) {
      const segment = segments[depth];
      let child = directory.directories.get(segment);
      if (!child) {
        const ancestry = segments.slice(0, depth + 1);
        child = { id: changeDirectoryId(ancestry), displayPath: ancestry.join("/"), segments: ancestry, directories: new Map(), files: [] };
        directory.directories.set(segment, child);
      }
      directory = child;
    }
    return directory;
  }
  for (const item of directories) ensureDirectory(item.segments).displayPath = item.displayPath;
  for (const file of files) ensureDirectory(file.segments.slice(0, -1)).files.push(file);
  return root;
}

export function flattenChangeRows(hierarchy: Directory, files: ChangeTreeFile[], view: "tree" | "list", collapsed: ReadonlySet<string>) {
  const rows: ChangeTreeRow[] = [];
  function addFile(file: ChangeTreeFile, parent: number | null, depth: number) {
    // A renewed native capability changes activation authority, not the presentation identity.
    rows.push({ key: `file:${changeDirectoryId(file.segments)}`, item: file, file, depth, parent, end: rows.length + 1, expanded: false });
  }
  function visit(directory: Directory, parent: number | null, depth: number) {
    const children = [...directory.directories.values()].sort((left, right) => {
      const a = left.segments[left.segments.length - 1];
      const b = right.segments[right.segments.length - 1];
      return a < b ? -1 : a > b ? 1 : 0;
    });
    for (const child of children) {
      const index = rows.length;
      const expanded = !collapsed.has(child.id);
      const row: ChangeTreeRow = { key: `directory:${child.id}`, item: child, depth, parent, end: index + 1, expanded };
      rows.push(row);
      if (expanded) visit(child, index, depth + 1);
      row.end = rows.length;
    }
    for (const file of directory.files) addFile(file, parent, depth);
  }
  if (view === "list") for (const file of files) addFile(file, null, 0);
  else visit(hierarchy, null, 0);
  return rows;
}
