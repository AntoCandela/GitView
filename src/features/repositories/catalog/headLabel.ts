/** Formats verified HEAD facts and explicitly labels contexts still awaiting native verification. */

import type { RepositoryEntry } from "../../../contracts/repositories";
import { translate, type Locale } from "../../../i18n";

export function headLabel(entry: RepositoryEntry, locale: Locale): string {
  switch (entry.head.kind) {
    case "unknown":
      return translate(locale, entry.availability === "checking" ? "repo.head.checking" : "repo.head.unknown");
    case "branch":
      return entry.head.name;
    case "detached":
      return translate(locale, "repo.head.detached", { oid: entry.head.shortOid });
    case "unborn":
      return translate(locale, "repo.head.unborn", { name: entry.head.name });
  }
}
