# Local Docling worker

An optional document converter for PDFs, DOCX, PPTX, and EPUB sources that need
more than Magnesium's native extraction. It processes an explicitly selected source
range through a separately packaged local Docling installation. Native
extraction remains the default.

## Enable a prepared worker

Start Magnesium with `MAGNESIUM_DOCS_WORKER` set to the packaged worker executable and
`MAGNESIUM_DOCS_WORKER_OPT_IN=1`. OCR also requires `MAGNESIUM_DOCS_WORKER_OCR=1` and
locally installed OCR models. Without OCR, scanned pages can return
`text_unavailable`.

The native runtime opens the authorized source and passes a read-only snapshot,
its SHA-256, and the requested range over the `MAGNESIUM_DOCS_V1` stdio protocol.
The worker returns anchored text blocks, extraction provenance, warnings, and
coverage. It validates the snapshot and hash before conversion. Conversion
never modifies the original source.
Extracted text is derived data, not learner evidence or proof of correctness.

## Prepare a development environment

From the repository root, create a dedicated environment rather than changing a
user's existing Python environment:

```sh
python3 -m venv /tmp/magnesium-docling
/tmp/magnesium-docling/bin/pip install -r workers/docling/requirements.txt
```

[requirements.txt](requirements.txt) pins direct dependencies.
[pins.json](pins.json) records source-distribution provenance; it isn't a
complete dependency lock. Distribution requires a per-target lock containing
all direct and transitive artifacts with verified hashes, installed using
`--require-hashes`, plus the interpreter, dependencies, licenses, and local
layout/table/OCR assets. That release package isn't provided here. Verify
offline conversion with bundled assets before distributing it.

## Run tests

The [self-tests](tests/README.md) need no Docling models. For conversion fidelity,
the sample generator requires headless Chrome/Chromium. Generate its 12-document
corpus, build the native baseline, and run the gate with the Docling environment:

```sh
cargo build --locked --bin magnesium
python3 workers/docling/sample/generate.py workers/docling/sample
/tmp/magnesium-docling/bin/python workers/docling/gate.py --out /tmp/magnesium-docling-results.json
```

The gate uses `MAGNESIUM_TEST_BINARY` when set, otherwise `target/debug/magnesium`, in a
private, credential-free disposable workspace. The manifest explicitly names
native readable, text-unavailable, and unsupported expectations. Native status
runs even with zero lexical hits. Each readable quotation must occur exactly in
its own physical-page excerpt; lexical retrieval is checked separately against
the expected path, hash, and eligible anchor. No text normalization removes
mathematical signs or negation. Office/EPUB native text fidelity is `NOT_RUN`
with a separately checked unsupported-format qualification.

`--sample DIR` selects the generated sample. `--out FILE` writes all 12 document
results and their native responses even when initialization, extraction, worker
conversion, or child cleanup fails. The default output is
`docling-gate-results.json`. Only explicit coordinator preparation states may
continue; `--preparation-seconds` (30), `--preparation-attempts` (20), and
`--request-seconds` (15) bound the operation. Unknown fields, wrong hashes,
exclusions, malformed responses, and wrong text never become retry-until-green
loops. The test-only asyncio client owns bounded stdio and child cleanup.

Missing Docling dependencies leave optional conversion and its unmeasured
budgets `NOT_RUN`; native branches must still qualify. The installed optional
stack reports conversion coverage, runtime, peak child RSS, and package size.
This is source-fidelity evidence only; no model inference is involved.

The ordinary core checks run an inexpensive actual-core smoke for a two-page
readable PDF, an image-only blank PDF, and an excluded source. Its fixtures use
standard-library PDF bytes and require neither Chrome nor Docling. The full
12-document conversion sample remains an explicit optional check.

## Implementation and limits

- [worker.py](worker.py): framing, snapshot validation, conversion, and block mapping
- [gate.py](gate.py): native/worker comparison and resource measurements
- [sample/generate.py](sample/generate.py): authored documents and anchor manifest
- [native integration](../../src/retrieval/manifest/convert.rs): authorization and worker invocation

The worker caps snapshots at 64 MiB, extracted text at 2 MiB, and returned blocks
at 512. It runs on demand; it doesn't reprocess whole workspaces or provide
background enrichment.
