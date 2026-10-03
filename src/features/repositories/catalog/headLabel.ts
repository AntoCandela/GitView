/** Formats verified HEAD facts and explicitly labels contexts still awaiting native verification. */

import type { RepositoryEntry } from "../../../contracts/repositories";

export function headLabel(entry: RepositoryEntry): string {
  switch (entry.head.kind) {
    case "unknown":
      return entry.availability === "checking" ? "Checking HEAD…" : "HEAD unknown";
    case "branch":
      return entry.head.name;
    case "detached":
      return `Detached · ${entry.head.shortOid}`;
    case "unborn":
      return `Unborn · ${entry.head.name}`;
  }
}
