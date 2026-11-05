# Contributing to KnegisWhisper

Bug reports, focused fixes, clearer documentation and thoughtful product feedback
are welcome. Start with a problem someone can reproduce or a concrete improvement
someone needs.

## Report a problem

Search open and closed [issues](https://github.com/Knegis/KnegisWhisper/issues) first,
then use the bug-report form. Include your app version, operating system,
provider/model, expected result and shortest reproduction. Redacted diagnostics
help; private audio, transcripts and credentials are never required.

Report vulnerabilities through [the security policy](SECURITY.md), not public issues.

## Propose a change

Use the feature-request form to explain the workflow and why it matters. Discuss
substantial features, UI changes, persistence changes and architecture work before
implementing them. A proposal does not guarantee acceptance or a release date.

## Submit a pull request

1. Fork the repository and branch from `dev`, the integration branch.
2. Follow [Development](docs/DEVELOPMENT.md) to build the native application.
3. Keep the change focused. Preserve unrelated edits and user data.
4. Run the relevant checks from that guide. Bug fixes should include a regression
   check when it meaningfully demonstrates the failure and correction.
5. Open your PR against `dev` and fill in the template. Explain the problem,
   behavior change, validation and any remaining limitations. Include before/after
   screenshots for visual changes.

Use descriptive conventional titles such as `fix(notes): preserve the selected folder`.
Update user documentation when behavior changes; add a concise Unreleased changelog
entry when the change matters to users. Maintainers promote reviewed work to `main`
and prepare releases there.

## AI-assisted work

AI assistance is welcome. You remain responsible for understanding the change,
checking its assumptions and explaining what the verification proves. Do not submit
generated audits or broad cleanup without establishing a useful, focused problem.

## Verification and privacy

Frontend fixtures contain invented data and do not exercise native capture, model
inference or credential storage. Identify browser, automated native and hands-on
checks separately. Never commit recordings, transcripts, tokens, downloaded models
or local profiles containing personal data. See [network purposes](docs/NETWORK_BOUNDARY.md)
and [security details](docs/SECURITY.md).

Be considerate in reports and reviews. Give maintainers enough context to assess a
change without asking them to reconstruct your investigation.
