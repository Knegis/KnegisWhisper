/** Classifies CDP pages by their stable application URL fragment. */
export function classifyTarget(target) {
  let hash = "";
  try {
    hash = new URL(target.url).hash;
  } catch {
    return "other";
  }
  if (hash === "") return "main";
  if (hash === "#overlay") return "overlay";
  if (hash === "#meeting") return "meeting";
  if (hash === "#update") return "update";
  return "other";
}

export function listClassifiedTargets(targets) {
  return targets.map((target) => ({
    ...target,
    classification: classifyTarget(target),
  }));
}

export function pickTarget(targets, wanted) {
  return targets.find(
    (target) => target.type === "page" && classifyTarget(target) === wanted,
  ) ?? null;
}
