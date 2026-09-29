# Shared vocabulary validation (KNEG-257)

Implemented on 17 September 2026. Vocabulary is a Settings page, not a top-level destination. The dictionary is the single source for local Whisper dictation and supported meeting integrations; legacy meeting keyterms migrate transactionally on database open. CLI/API dictionary routes retain their wire formats. Meeting keyterm commands now read/replace the shared dictionary for compatibility; the former setting is retained only as migration input.

## Request contract

Changes apply to the next dictation request or new meeting. A meeting's owned configuration retains its vocabulary across reconnects. Empty lists omit prompts. Local imports, sherpa dictation, local meetings, OpenAI Realtime and Tinfoil are not newly enabled. Vocabulary and prompt values are redacted from Debug output.

- Whisper and KB Whisper: comma-separated whole terms, application budget 200 UTF-8 bytes including separators. This is a conservative application limit, not a tokenizer measurement.
- AssemblyAI: 100 terms, 50 characters each, JSON keyterms_prompt query parameter.
- Corti: 1,000 terms, 50 characters and five words each; configuration.keyterms.terms contains objects with a term field. The config message includes type=config.
- Deepgram: vocabulary-enabled sessions explicitly use Nova-3; an explicit incompatible model is preserved without hints. Repeated keyterm parameters, application budget 400 UTF-8 bytes including separators and at most 100 terms. This is not an exact measure of Deepgram's documented 500-token ceiling.

The same preparation code supplies request payloads and usage review. Terms are normalized, sorted alphabetically and deduplicated with case preserved. Terms that do not fit are omitted whole, with reasons; later smaller terms remain eligible.

Sources checked 17 September 2026:
- https://www.assemblyai.com/docs/voice-agents/best-practices
- https://docs.corti.ai/stt/keyterms
- https://docs.corti.ai/api-reference/transcribe
- https://developers.deepgram.com/docs/keyterm

## Automated and native checks

Storage checks cover migration idempotency, malformed legacy input rollback, duplicate handling, Swedish case/spelling preservation, ID-preserving edits and delete/undo of over-limit legacy entries. Main-window Tauri IPC checks exercise CRUD, review and undo against shared storage and reject access from another window. Request tests cover decoder prompt forwarding, language detection without a prompt followed by inference with the prompt, provider caps and wire formats, incompatible Deepgram models, and unchanged silence gating.

The Vocabulary page was visually inspected in a separate native profile; a Swedish test term was saved and displayed. Native validation is separate from the mocked IPC tests.

## Local audio experiment and limitations

A bounded smoke test used locally synthesized English speech containing names and technical terms, plus three seconds of digital silence. The installed speech generator has only English voices; no representative Swedish recording was available. Standard base, KB Whisper tiny Q5 and KB Whisper base Q5 were exercised with and without the same short vocabulary. The KB model downloads matched registry-pinned size and SHA-256 values. Application decoder thresholds were used. These direct server requests bypass the dictation silence gate deliberately to inspect decoder behavior.

Results were mixed: standard base changed Dev to Dyve but did not spell every name correctly; KB tiny did not consistently improve; KB base recognized Axel Karlsson with the prompt but the rest of the English synthetic utterance remained inaccurate. These results do not establish an accuracy improvement, particularly for Swedish speech.

Direct silence requests generated non-speech output even without hints; standard base produced a prompt-related Kubernetes phrase with hints. Normal application dictation rejects silent capture before requesting inference, and its regression test passes. Low-level direct-server smoke results must not be presented as user dictation output.

Raw synthetic-only results are in ignored target/vocabulary-audio/results.json. Representative Swedish/English human-speech comparisons and live cloud sessions remain unverified. Do not close the issue's acoustic acceptance criteria on the basis of request tests or this synthetic sample alone.
