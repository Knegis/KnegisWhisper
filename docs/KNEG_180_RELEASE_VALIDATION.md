# KNEG-180: release workflow validation

The release workflow referenced `secrets.APPLE_CERTIFICATE` and
`secrets.WINDOWS_CERTIFICATE` directly in step `if` conditions. GitHub does not
allow the secrets context there. The build job now exposes two boolean presence
flags as environment variables, and each certificate-import condition compares
the corresponding flag to `'true'`. Certificate values and passwords remain
step-scoped. The OS conditions and unsigned-build behavior are preserved.

## Evidence, 8 September 2026

- Authenticated GitHub API inspection confirmed run `34169942694` failed at
  `d1d3cf4727b73e051688f81d9b7022e77ff61fe2`, for `.github/workflows/release.yml`.
  Check suite `92562829272` has zero check runs, so the check-run annotations API
  provides no server-side error text. This report does not claim to quote a
  GitHub annotation.
- Official actionlint v1.7.12, verified against its release checksum, rejected
  the original workflow at lines 95 and 109 with `context "secrets" is not
  allowed here`.
- After the change, both the release workflow and all three repository
  workflows passed `actionlint -shellcheck= -pyflakes=`. This checks workflow
  syntax and expressions; shellcheck and pyflakes were disabled explicitly.
- `git diff --check` passed. No application code changed.

The workflow remains dispatch-only. No release was dispatched or published,
and no signing secrets were added. Hosted acceptance and a real bundle/signing
run remain unverified; release delivery is tracked by KNEG-43.

Sources: [GitHub secret conditions](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/use-secrets),
[actionlint usage](https://github.com/rhysd/actionlint/blob/v1.7.12/docs/usage.md),
[failed run](https://github.com/axelalbertkarlsson/KnegisWhisper/actions/runs/34169942694).
