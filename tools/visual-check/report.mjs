/** Renders data-first visual-check results into an operator-readable report. */
export function createReport(header = {}) {
  return { header, checks: [], events: [] };
}

export function addCheck(report, check) {
  report.checks.push({ screenshots: [], note: "", matches: null, ...check });
  return report;
}

export function addEvent(report, event) {
  report.events.push(event);
  return report;
}

function display(value) {
  if (value === undefined) return "";
  return typeof value === "string" ? value : JSON.stringify(value);
}

function checkDetails(check) {
  const lines = ["", `### ${check.id}`, "", "```json"];
  lines.push(JSON.stringify(check.measured, null, 2));
  lines.push("```");

  if (check.note) lines.push("", check.note);
  if (check.screenshots.length) {
    lines.push("", `Screenshots: ${check.screenshots.join(", ")}`);
  }
  return lines;
}

export function renderMarkdown(report) {
  const lines = ["# Visual check report", "", "## Header", ""];

  for (const [key, value] of Object.entries(report.header)) {
    lines.push(`- ${key}: ${display(value)}`);
  }

  lines.push(
    "",
    "## Checks",
    "",
    "| ID | Claim | Matches | Expectation |",
    "| --- | --- | --- | --- |",
  );

  for (const check of report.checks) {
    const matches = check.matches === null ? "human review" : check.matches;
    lines.push(
      `| ${check.id} | ${check.claim} | ${matches} | ${check.expectation
        .replaceAll("|", "\\|")} |`,
    );
  }

  for (const check of report.checks) lines.push(...checkDetails(check));

  if (report.events.length) {
    lines.push("", "## Observations", "");
    for (const event of report.events) lines.push(`- ${display(event)}`);
  }

  return `${lines.join("\n")}\n`;
}
