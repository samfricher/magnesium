# Magnesium

A learning engine for Claude Code. It works with whatever model your Claude Code runs.

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

- **Practise for a test.** The tutor writes a study plan and timed practice papers for your test. It marks your answers and helps you fix each mistake.
- **Continue where you left off.** The tutor resumes a problem with its context and next step.
- **Remember how you reason.** Magnesium keeps your strengths, your difficulties, your original answers, and the help that you received.
- **Learn from your materials.** The tutor searches your local notes, PDFs, and source code during a conversation.
- **Correct a wrong memory.** Tell the tutor what really happened. The tutor checks the memory against your original work and corrects it.
- **Control what is recorded.** You can pause a lesson, turn recording off, or export your history.

Magnesium uses Claude Code's existing model connection. Your usual model limits and charges apply. Magnesium needs no separate API key, embedding model, or database server.

## Get started

Hand this repository to your coding agent and ask it to install Magnesium by following [docs/install.md](docs/install.md). The agent asks before it installs anything beyond the build.

On an Apple Silicon Mac you can instead download the study window, which installs Magnesium and sets up each study folder itself, with no terminal. [Download](#download) gives the steps.

In Claude Code, type `/magnesium:help` first. It shows a short guide and suggests a next step for your folder, such as adding your materials, studying a chapter, or continuing your study plan. To read about one topic, type `/magnesium:help papers`, `/magnesium:help recording`, `/magnesium:help setup`, or `/magnesium:help trouble`.

To start a lesson, ask for one. For example:

> Teach me how confidence intervals work using the notes in this folder.

Ordinary coding conversations do not start a lesson. Ask to learn, practise, or review when you want Magnesium involved.

To practise for a test in Claude Code, tell the tutor about the test, for example "I have a midterm Monday". You can also type `/magnesium:papers`. The tutor writes your plan and papers in `Study/<target>/`, for example `Study/midterm-1/`. Later conversations continue from that plan. The [user guide](docs/user-guide.md#practise-with-timed-papers) gives the steps.

Magnesium is built and tested for Claude Code. Its learner record, its files and its tools are open to any agent that can call them. The hooks that keep a lesson on track, the skills and the plan templates work only in Claude Code. An adapter for another agent is separate work.

Magnesium stores your learning history in `.magnesium/` inside the study folder. Your assistant's model receives the memories and file excerpts that a lesson needs. The [user guide](docs/user-guide.md#control-recording-and-keep-a-copy) explains how to control recording and export your history.

## The study window

One window, made for studying. It sits over Magnesium and the coding agent you already use, and adds no teaching rule of its own: a lesson behaves the same here and in a terminal. macOS on Apple Silicon. Claude Code is the agent.

### Download

1. From the [releases page](https://github.com/samfricher/magnesium/releases), download the newest `Magnesium-<version>-arm64.dmg` and open it.
2. Drag Magnesium to Applications, then open it from Applications.
3. macOS says it could not check the app for malware. Choose **Done**.
4. Open **System Settings → Privacy & Security**. Under Security, choose **Open Anyway** next to Magnesium, and confirm.

Step 4 is needed once for each new version. The app is signed ad hoc, which is free and needs no Apple account, but it is not signed by Apple. If macOS ever says the app is damaged, the copy did not come from the release page: download it again.

You need [Claude Code](https://code.claude.com/docs/en/setup), and a paid Claude plan or an Anthropic Console account. No terminal step is needed.

#### What the app sets up

The first screen shows each step, and moves on when they are done.

- **The `magnesium` program.** It is inside the app, built from the same commit. On first open the app puts it in `~/.local/bin`, where Claude Code's own installer puts `claude`. The window, Claude Code's hooks and a terminal then all run that one copy. If another version is already installed, the app asks before it replaces it, because a newer program upgrades each study folder's record when it opens the folder. If you keep the other version, lessons run on it, and the app says what that version cannot do. A program older than the window needs is refused, whatever its version number says: the app offers to replace it, or, run from source, names the install guide, and starts no lesson until it is fixed. "Keep" is not offered for it.
- **Claude Code.** The app looks for `claude` and asks it whether you are signed in (`claude auth status`). Signed out, a button starts Claude Code's own sign-in (`claude auth login`), which opens Anthropic's page in your browser. Without Claude Code, the app links to Anthropic's install page and offers Check again. The app never installs Claude Code, and never reads or stores a credential.
- **How lessons are paid for.** Choose your Claude plan, the default, or an API key, paid per use. The first screen and ⌘K show which one Claude Code would use. With the plan chosen, the app starts Claude Code without the `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` variables, because Claude Code started by another program uses such a key over the plan without asking. A key that Claude Code keeps itself, such as an API key helper in its settings, cannot be held back: the app then says the lesson is not paid with the plan.
- **Your study folders.** The library lists every study folder the window knows. **Open a folder** adds one, and **New folder** makes one. **Find folders on this Mac** looks through your home folder for folders that Magnesium was set up in from a terminal, and adds them. It runs only when you press it, it reads folder names only, and macOS may ask whether the app may read Documents or Desktop. A folder Magnesium has never been set up in is set up when you press Start. A folder an earlier version set up asks once, then is upgraded: Magnesium keeps a copy of its record in `.magnesium/backups` before it changes it. Say "Not now" and the folder stays as it was, and the window starts no lesson in it. **Remove** on a course takes it out of the library and leaves the folder as it is.

Setting a folder up also writes two things outside it: Magnesium's plugin for Claude Code in `~/Library/Application Support/magnesium`, and one entry in Claude Code's settings that names it. The [install guide](docs/install.md#5-set-up-a-study-folder) lists every file.

## Find your way around

| I want to… | Start here |
| --- | --- |
| Use Magnesium in a study session | [User guide](docs/user-guide.md) |
| Build, test, or change the project | [Contributing](CONTRIBUTING.md) |
| Download the study window | [The study window](#the-study-window) |

## License

Magnesium is licensed under either of the [MIT license](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option. Unless you say otherwise, any contribution you submit for inclusion is dual-licensed the same way, without extra terms.
