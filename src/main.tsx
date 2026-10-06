/** Boots the desktop workspace with shared fonts, styles and React lifecycle checks. */

import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Workspace } from "./app/Workspace";
import { CompanionPanel } from "./app/companion/CompanionPanel";
import { repositoryClient, reviewSurfaceClient } from "./platform/RepositoryClient";
import { initializeLocale } from "./i18n";
import "./style.scss";

void reviewSurfaceClient.bootstrap().then(async (surface) => {
  // Native identity, not query strings or routes, selects the least-authority composition.
  if (surface === "main") await initializeLocale(() => repositoryClient.preferredLanguages().then((result) => result.languages));
  createRoot(document.getElementById("root")!).render(
    <StrictMode>{surface === "companion" ? <CompanionPanel /> : <Workspace />}</StrictMode>,
  );
}).catch(() => {
  // Without an approved identity there is no authorized composition or guessed locale.
  const root = document.getElementById("root");
  if (root) { root.setAttribute("role", "status"); root.setAttribute("aria-busy", "true"); }
});
