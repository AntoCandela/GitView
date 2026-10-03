/** Boots the desktop workspace with shared fonts, styles and React lifecycle checks. */

import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Workspace } from "./app/Workspace";
import "./style.scss";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Workspace />
  </StrictMode>,
);
