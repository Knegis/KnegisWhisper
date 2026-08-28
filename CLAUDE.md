# Working in this repository

## Keep Linear current — this repo's label is `knegiswhisper`

Every defect, gap, blocked item and unverified claim this project records in `docs/` should have a
matching Linear issue, and **keeping the two in step is part of the work, not a separate chore.**
The `docs/` files are maintained in place — when something in them becomes false it is edited, not
annotated — so an issue that still describes a state that has ended is the same defect one letter
later.

**Team:** `Knegis Fegis`. **Label:** `knegiswhisper`, plus exactly one type label — `Bug`,
`Improvement` or `Feature`.

> **Never use `axelalbertkarlsson/villaklint`.** That label routes agents to a different
> repository (`github.com/axelalbertkarlsson/villaklint`) and has nothing to do with this project.
> It exists in the same workspace and is easy to reach for by accident. When in doubt, list the
> labels and read the parent group: this repo's label sits under `axelalbertkarlsson`, villaklint's
> under `repo`.

### When to update it

After any pass that changes what is true — a bench run, a chunk landing, a merged fix, a doc
reconciliation:

- **File what is newly recorded.** A finding written down in `BENCH_CHECK.md` or
  `ENGINEERING_LOG.md` and nowhere else is a finding that will be rediscovered.
- **Close what actually landed**, citing the commits, and say in the closing note what did *not*
  close with it. Split the remainder out rather than leaving a half-true issue open.
- **Correct what changed.** If half a defect was fixed, retitle and re-scope to the half that
  remains, and say which half moved and why the priority moved with it. Appending to the
  description is preferred over rewriting it — the history of a claim is often the useful part.
- **Reframe a decision as a decision.** Something declined on principle (Tinfoil, the AEC helper)
  stays visible at no priority rather than sitting in the backlog reading as unfinished work.

### What an issue should say

The same standard the docs hold themselves to:

- **A probe verifies the link it exercised and nothing further.** "The hook fires" and "the hotkey
  works" are different claims. An issue that says a thing is verified names the evidence.
- **Distinguish *believed*, *reused from tested code*, and *invented*.** Three epistemic states,
  not one word. The meeting clients are the worked example.
- **Name the source.** End with the file and section the item came from, so the issue can be
  re-checked against the record rather than trusted.
- **Say what is *not* blocked by it.** Half the value of the blocked table in `COMPLETION_PLAN.md`
  is the right-hand column.

### Priorities as used so far

`Urgent` for a live exposure the application does not disclose. `High` for a disclosed exposure, a
gate that is off, or a by-hand check that would prove a core claim. `Medium` for most real defects
and unverified claims. `Low` for future capability with no consumer yet. `No priority` for
something declined rather than deferred.
