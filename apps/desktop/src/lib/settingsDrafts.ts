/** Only labels and operations are registered. Draft values stay inside their forms. */
export type SettingsDraft = {
  label: string;
  order?: number;
  dirty: () => boolean;
  settle: () => Promise<void>;
  save: () => Promise<void>;
  discard: () => void;
};
export class SettingsDraftRegistry {
  private entries = new Map<string, SettingsDraft>();
  private listeners = new Set<() => void>();
  subscribe(listener: () => void) { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; }
  notify() { for (const listener of this.listeners) listener(); }
  register(id: string, entry: SettingsDraft) { this.entries.set(id, entry); return () => { this.entries.delete(id); }; }
  dirty() { return [...this.entries.values()].filter(entry => entry.dirty()).sort((a, b) => (a.order ?? 0) - (b.order ?? 0)); }
  async settle() {
    await Promise.all([...this.entries.values()].map(entry => entry.settle()));
    // Form acknowledgments schedule React state. Observe them after its event batch commits.
    await new Promise<void>(resolve => setTimeout(resolve, 0));
  }
  async save() {
    await this.settle();
    for (const entry of this.dirty()) await entry.save();
    await this.settle();
    if (this.dirty().length) throw new Error("Some settings still have unsaved changes. Review them before leaving.");
  }
  async discard() { await this.settle(); for (const entry of this.dirty()) entry.discard(); }
}
