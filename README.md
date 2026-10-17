# Magnesium

A harness-neutral learning engine for Claude Code, Codex and OpenCode. It works with whatever model your assistant runs.

In the brain, a single magnesium ion (Mg²⁺) sits in the channel of a synapse's NMDA receptor and blocks it. Weak or one-sided signals leave it in place. It moves aside only when both neurons fire together, and that lets the connection strengthen. Magnesium brings the same rule to your learning. It finds the missing piece in your mental model through Socratic questions, diagrams when they help, and active recall. Then it remembers what you know across every session in your study folder.

Magnesium works in a study folder. A study folder is any course or project folder where you ran `magnesium init`. Each study folder keeps its own learning history: your reasoning, useful explanations, and unfinished work from earlier conversations.

## What it does

- **Practise for a test.** In Claude Code, the tutor writes a study plan and timed practice papers for your test. It marks your answers and helps you fix each mistake.
- **Continue where you left off.** The tutor resumes a problem with its context and next step.
- **Remember how you reason.** Magnesium keeps your strengths, your difficulties, your original answers, and the help that you received.
- **Learn from your materials.** The tutor searches your local notes, PDFs, and source code during a conversation.
- **Correct a wrong memory.** Tell the tutor what really happened. The tutor checks the memory against your original work and corrects it.
- **Control what is recorded.** You can pause a lesson, turn recording off, or export your history.

Magnesium uses your assistant's existing model connection. Your usual model limits and charges apply. Magnesium needs no separate API key, embedding model, or database server.

## Get started

Hand this repository to your coding agent and ask it to install Magnesium by following [docs/install.md](docs/install.md). The agent asks before it installs anything beyond the build.

In Claude Code, type `/magnesium:help` first. It shows a short guide and suggests a next step for your folder, such as adding your materials, studying a chapter, or continuing your study plan. To read about one topic, type `/magnesium:help papers`, `/magnesium:help recording`, `/magnesium:help setup`, or `/magnesium:help trouble`.

To start a lesson, ask for one. For example:

> Teach me how confidence intervals work using the notes in this folder.

Ordinary coding conversations do not start a lesson. Ask to learn, practise, or review when you want Magnesium involved.

To practise for a test in Claude Code, tell the tutor about the test, for example "I have a midterm Monday". You can also type `/magnesium:papers`. The tutor writes your plan and papers in `Study/<target>/`, for example `Study/midterm-1/`. Later conversations continue from that plan. The [user guide](docs/user-guide.md#practise-with-timed-papers) gives the steps.

Practice papers and `/magnesium:help` work only in Claude Code for now. Codex and OpenCode do not yet have the skills or the plan templates.

Magnesium stores your learning history in `.magnesium/` inside the study folder. Your assistant's model receives the memories and file excerpts that a lesson needs. The [user guide](docs/user-guide.md#control-recording-and-keep-a-copy) explains how to control recording and export your history.

## Find your way around

| I want to… | Start here |
| --- | --- |
| Use Magnesium in a study session | [User guide](docs/user-guide.md) |
| Build, test, or change the project | [Contributing](CONTRIBUTING.md) |
| Understand the design | [Architecture](docs/architecture/learner-model.md) |
| Work with the tools and data | [API reference](docs/architecture/learner-model-contracts.md) · [Database schema](src/learner/schema.sql) |
| Understand privacy and trust boundaries | [Security model](docs/security/threat-model.md) |

## License

Magnesium is licensed under either of the [MIT license](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option. Unless you say otherwise, any contribution you submit for inclusion is dual-licensed the same way, without extra terms.
