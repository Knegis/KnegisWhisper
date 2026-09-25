/** Escape literal speech before handing it to a Markdown editor. */
export function transcriptToMarkdown(rows: readonly { text: string; speaker: string | null }[]): string {
  const literal = (value: string) => value.replace(/([\\`*_{}\[\]()<>#+.!|~=&-])/g, "\\$1")
    .replace(/^[ \t]+/, spaces => Array.from(spaces, c => c === " " ? "&#32;" : "&#9;").join(""));
  return rows.filter(row => row.text.trim()).map(row => {
    const text = row.speaker ? `${row.speaker}: ${row.text}` : row.text;
    return text.split(/\r?\n/).map(literal).join("  \n");
  }).join("\n\n");
}
