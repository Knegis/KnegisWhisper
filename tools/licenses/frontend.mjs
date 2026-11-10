import { readFileSync, readdirSync, existsSync } from "node:fs";
import { dirname, join, parse } from "node:path";

// Inventory modules in emitted chunks, rather than every installed package.
export function dependencyNotices() {
  return {
    name: "dependency-notices",
    generateBundle(_options, bundle) {
      const packages = new Map();
      for (const chunk of Object.values(bundle)) {
        if (chunk.type !== "chunk") continue;
        for (const id of Object.keys(chunk.modules)) {
          if (!id.includes("node_modules")) continue;
          let dir = dirname(id.replace(/^\0/, "").split("?")[0]);
          let pkg;
          while (dir !== parse(dir).root) {
            if (existsSync(join(dir, "package.json"))) {
              pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
              if (pkg.name) break;
            }
            dir = dirname(dir);
          }
          if (!pkg?.name) throw new Error("No package owner for " + id);
          const key = pkg.name + "@" + pkg.version;
          if (packages.has(key)) continue;
          const names = readdirSync(dir).filter(n => /^(licen[cs]e|copying|notice)(\.|$|-|_)/i.test(n)).sort();
          if (!names.length) throw new Error("Missing bundled license: " + key);
          const texts = names.map(n => readFileSync(join(dir, n), "utf8").replace(/\r\n/g, "\n"));
          packages.set(key, { name: pkg.name, version: pkg.version, license: pkg.license,
            source: typeof pkg.repository === "string" ? pkg.repository : pkg.repository?.url, texts });
        }
      }
      this.emitFile({ type: "asset", fileName: "dependency-notices.json",
        source: JSON.stringify([...packages.values()].sort((a,b) => a.name.localeCompare(b.name, "en")), null, 2) + "\n" });
    },
  };
}
