/** Keeps Vite's process-exiting signal handlers outside the smoke runner's cleanup owner. */
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { preview } from "vite";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const server = await preview({ root, configFile: false, logLevel: "silent", build: { outDir: "dist" },
  preview: { host: "127.0.0.1", port: 0, strictPort: true, open: false } });
const address = server.httpServer.address();
if (!address || typeof address !== "object" || address.address !== "127.0.0.1" || !process.send) {
  await server.close();
  process.exitCode = 1;
} else {
  process.on("disconnect", () => { void server.close(); });
  process.send({ port: address.port });
}
