/**
 * Whether a module is the script Node was asked to run.
 *
 * The tools here guard their command line with this so their tests can import
 * them without running them. The check every one of them used to inline,
 * `resolve(process.argv[1]) === fileURLToPath(import.meta.url)`, compares two
 * different spellings of the same file whenever the path runs through a
 * symbolic link or a junction: Node only `path.resolve`s `argv[1]`, but
 * resolves the main module to its real path before it sets
 * `import.meta.url`. The script then did nothing and exited 0, which for a
 * CI gate or the weekly pin check is a pass that checked nothing.
 *
 * So `argv[1]` goes through the same `realpathSync` the loader uses. The
 * unresolved spelling is accepted too, for `--preserve-symlinks-main`, where
 * the loader keeps the link.
 */

import { realpathSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

/**
 * @param {string} metaUrl the module's own `import.meta.url`
 * @param {string | undefined} argv1 the script Node was asked to run
 */
export function isEntryPoint(metaUrl, argv1 = process.argv[1]) {
  if (!argv1) return false;
  const self = fileURLToPath(metaUrl);
  const asked = resolve(argv1);
  if (asked === self) return true;
  try {
    return realpathSync(asked) === self;
  } catch {
    return false;
  }
}
