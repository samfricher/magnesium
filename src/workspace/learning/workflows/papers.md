# Papers

Each turn, first start or resume the Magnesium session for the target (a slug like `midterm-1`) before reading materials, since native capture expires within minutes. Stage papers_loop serves marking, repair and redos.

## The plan is built outside the lesson

The planning run builds the course map and the plan, in a run of its own, so this conversation never reads the folder file by file. Start it with the subagent tool (Agent, called Task in some hosts), subagent_type `magnesium:planner`, and a brief that names:

- the target slug;
- the learner's own words about the test or goal (what, when, how long, aids);
- the hours they have, and when and where they practise;
- anything they have already agreed or refused.

Before you start it, ask only for what the learner's message and the folder do not show, each with a default they can skip. Also ask when and where they will practise, and what they will do if they miss a session. Suggest a default that fits their hours and commitments. They may take it or skip the question. Write the answer with course_map write facts sessions. Each session is one line, "If it is <days> at <HH:MM>, then I <what> <where>.", with a 24-hour start time. Leave out "<days> at" for every day. Then add one backup line that starts "If I miss".

When no subagent tool is available, load stage planning yourself and follow it here.

While it runs, tell the learner in one line that the plan is being built from their files.

When it returns:

1. Ask the questions its report lists: each cut to agree, and each assumed fact.
2. Record each answer with course_map write: a cut agreed (scope state confirm), a cut refused (scope state in), and new facts.
3. Say the plan's shape in a few lines: where it starts, and how much it covers.
4. Begin block 1.

The runtime writes `Study/<target>/plan.md` and `map.md` from the map; never edit them. Start the planning run again when a sitting ends, when a block runs well past its minutes, or when the learner asks to re-plan.

## A practice paper samples the map

A practice paper or mock is a set of tasks over in-scope items, in the form the instructor's past papers use. Build one when a block of the plan names it, or when the learner asks for a new mock.

1. course_map read with the target states which items were never asked and which have no blank-page pass; reach counts, for each candidate, the items it uses.
2. Choose the questions from those counts, in this order: the instructor's own exercises and past questions, posed whole, as the source poses them or in a new setting (the source's form with one thing changed, as the traces on the map show); then a problem you write, for an item no listed one asks.
3. Never rank by marks on a paper you wrote.

Three question shapes resist pattern matching. Give the result, ask the reason: state a true result and ask why it holds, such as "Var(3X) is 9 Var(X), not 3 Var(X). Why?" Two look-alikes, one difference: set side by side two problems that look the same but need different methods, and ask which method each needs and what feature decides it. Break one assumption: drop one condition of a method and ask which step fails, with a counterexample. Every mock paper has at least one question of each shape. A given result earns full marks only for a correct reason. A stretch question goes beyond scope, is labelled, and is scored apart.

## Format and keys

Use the format that renders best: LaTeX for math, Markdown with runnable keys for code, or HTML.

Each paper gets a separate key:

- a mark scheme and half-credit rules;
- one line per question, not per part, such as `Items: lec05.4, w5-list.6.63`: the course map items it uses, the item it comes from first;
- a check of the key where possible, by an executable method the key names (compile, tests, numeric check).

Write each paper you add to the map as a task item (kind task, ref its name and question, such as mocka.q2) with its use links, and name it in its block.

When you make a figure or page as a file, render it to an image with what the machine has, such as a headless browser screenshot or an SVG-to-PNG converter, then look at the image, fix what is wrong and look again. Show it only after you have looked at the final version. Check that nothing overlaps or is cut off, every label is readable, the picture shows what its question or key says, and nothing in it is unlabelled decoration. When a browser is available, open a page with scripts once in a headless browser to confirm it runs without errors. If the machine cannot render the file, say so and use text or a Mermaid diagram instead. Never give the learner a paper or key with a line cut off at the edge of the page. When the build names such a line, rewrite it so it can break, and build again.

Each paper tells the learner to write `?` instead of looking anything up, and S (sure), U (unsure) or G (guess) after each answer.
