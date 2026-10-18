# Magnesium

A harness-neutral learning engine for Claude Code, Codex and OpenCode. It works with whatever model your assistant runs.

In the brain, a single magnesium ion (Mg²⁺) sits in the channel of a synapse's NMDA receptor and blocks it. Weak or one-sided signals leave it in place. It moves aside only when both neurons fire together, and that lets the connection strengthen. Magnesium brings the same rule to your learning. It finds the missing piece in your mental model through Socratic questions, diagrams when they help, and active recall. Then it remembers what you know across every session in your study folder.

## Philosophy of the system

Reading feels productive. You finish a chapter of a textbook, or the documentation for a new framework, and every step makes sense while you read. A week later you need to use what you read. You open an empty file to set up the framework, and you cannot remember the steps. The steps made sense a week ago. Now only the memory of understanding is left.

The feeling of understanding measures how smoothly the text went in. Getting the content back out is a different ability, and the feeling does not measure that ability. Only the second ability matters, for one reason: understanding that you cannot reproduce cannot be applied to anything. It cannot set up the framework. It cannot carry the meeting where you explain the paper. An exam is only the case where someone measures this gap.

Roediger and Karpicke measured the gap in 2006. One group of students read a passage twice. A second group read the passage once and then wrote down what they remembered. The group that read twice felt more confident. A week later, the group that had written from memory remembered more.

Magnesium therefore asks you to reproduce what you read. The first question comes soon after you read, and later questions come after a gap. The material can be a lecture, a framework, a paper or a book. The loop is the same.

Reproducing from memory is the step that strengthens the memory. Psychologists call this effect the testing effect. The testing effect is one of the better-replicated findings in learning research, and it does not depend on exam material; studies have shown it with word lists and with medical procedures. Dunlosky and colleagues reviewed ten study techniques in 2013. They rated practice testing high utility, and they rated re-reading and highlighting low. A failed attempt to reproduce has a second use: the attempt shows where your understanding stops. When you cannot finish a derivation, the step you stopped at is the step you do not know. The tutor therefore explains in short pieces and asks for the next step as soon as you could take that step yourself.

When you get a step wrong, the tutor finds the first step that broke and explains that step only. Then the tutor asks a new question that depends on the repaired step. The question is new rather than repeated for one reason: a repeated question lets you recognise the answer you were just shown, and recognition is weaker than recall. Research on feedback supports this shape. Feedback helps most when it names the specific error and is followed by another attempt (Shute, 2008).

Everything you learn starts to fade as soon as you learn it. Reproducing the material again after a gap slows the fading, and the slowing is greater when the gap has made the material harder to reach. Cepeda and colleagues pooled over three hundred experiments on spacing in 2006 and found this pattern throughout. The tutor therefore records what you answered and when, and asks again before the material fades. The later questions mix topics, so that you also practise choosing which method applies. Choosing the method is a separate skill from carrying the method out, and real work requires both skills.

Magnesium started with exams, because exams are where the gap between feeling ready and being ready gets measured. In the courses Magnesium was built for, the hours that raised the grade were the hours spent sitting a practice paper cold, marking the paper, and fixing what the marking exposed. The hours spent re-reading slides were pleasant and changed almost nothing. The same loop fits a framework you are learning for work or a book you want to keep. Read a piece, close the source, reproduce the piece, find the step that broke, repair that step, and return to the piece later.

Every answer you give goes into a learner record in your study folder, and the record persists across conversations. After a few weeks the record shows which skills keep failing for you, and which prerequisite keeps appearing under those failures. Practice then goes to those skills and prerequisites. The record is yours. You can read the record and export it, and if the record is wrong you can show the tutor your original work and have the record corrected.

The magnesium ion in the receptor moves aside only when both sides of the synapse fire together. A question you answer, followed by feedback on the step you missed, is the closest a tutor can come to firing both sides together.

## What it does

Magnesium works in a study folder. A study folder is any course or project folder where you ran `magnesium init`. Each study folder keeps its own learning history: your reasoning, useful explanations, and unfinished work from earlier conversations.

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
