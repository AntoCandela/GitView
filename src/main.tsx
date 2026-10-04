/** Boots the desktop workspace with shared fonts, styles and React lifecycle checks. */

import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Workspace } from "./app/Workspace";
import { repositoryClient } from "./platform/RepositoryClient";
import { initializeLocale } from "./i18n";
import "./style.scss";

void initializeLocale(() => repositoryClient.preferredLanguages().then((result) => result.languages)).then(() => {
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <Workspace />
    </StrictMode>,
  );
});
