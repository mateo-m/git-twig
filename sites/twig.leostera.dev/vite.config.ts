import react from "@vitejs/plugin-react";
import { cloudflare } from "@cloudflare/vite-plugin";
import { defineConfig, type Plugin } from "vite";
import { fileURLToPath } from "node:url";
import { benchmarkFile, loadBenchmarks, sources } from "./scripts/benchmarks.ts";

const virtualId = "virtual:twig-benchmarks";
function benchmarkData(): Plugin {
  return {
    name: "twig-benchmark-data",
    resolveId(id) {
      if (id === virtualId) return "\0" + virtualId;
    },
    load(id) {
      if (id !== "\0" + virtualId) return;
      for (const { file } of sources)
        this.addWatchFile(fileURLToPath(benchmarkFile(file)));
      return `export default ${JSON.stringify(loadBenchmarks())}`;
    },
  };
}

export default defineConfig({
  plugins: [benchmarkData(), react(), cloudflare()],
  server: { host: "127.0.0.1", port: 5178, fs: { allow: ["../.."] } },
});
