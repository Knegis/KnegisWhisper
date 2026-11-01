/**
 * Split a confirmation question into its first sentence, ending at the first
 * "?", and the sentences after it: the dialog's title and its detail
 * (`components/ConfirmDialog.tsx`). Every question the application asks is
 * written that way round -- "Delete “Weekly sync” forever? Its recording …" --
 * so the title is what is being asked and the detail what it costs. A message
 * with no "?" is all title.
 */
export function splitQuestion(message: string): { title: string; detail: string } {
  const end = message.indexOf("?");
  if (end < 0) return { title: message, detail: "" };
  return { title: message.slice(0, end + 1), detail: message.slice(end + 1).trim() };
}
