# Docling worker self-tests

Run from the repository root:

```sh
python3 workers/docling/tests/test_worker.py
python3 workers/docling/tests/test_gate.py
```

`test_worker.py` launches the real worker with `stub_docling/` in place of the
conversion library. It checks framed requests, snapshot/hash/range validation,
anchor mapping, coverage, warnings, and bounded failures.

`test_gate.py` checks current explicit source references, zero-hit unavailable
text, per-page exact quotations, independent retrieval, unsupported formats,
bounded preparation, binary version identity, and all-document failure output.
Neither worker nor gate unit tests need an installed Docling stack or models.

`python3 -m unittest ci.test_native_mcp` exercises real adversarial child
processes: initialization rejection, malformed/partial/oversized JSON lines,
wrong response IDs, notification/stderr floods, request/write timeout, and EOF.
The owned child must be reaped in every case.

`python3 ci/check.py core` builds the native release executable and runs
`ci/check_native_sources.py` against authored standard-library readable/blank
PDFs and source exclusion, with a private environment and no provider discovery.
It writes machine-readable results under `target/validation/` on failure too.

Update worker stubs when its Docling API usage changes; use the
[conversion gate](../README.md#run-tests) for the separately installed optional
full conversion stack. Stub success is not conversion-quality evidence.
