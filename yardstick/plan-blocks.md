# Marking a plan's schedule, block by block

A block is one unit of scheduled work: a row of the plan's schedule table (the table with a block or duration column). Where the plan has no such table, it is a heading that sets out one section's work, together with the rows under it, and each later line that schedules a paper or a test. Mark every block, in order, naming it by its number or heading as the plan writes it. Judge only from the plan's own words.

For each block give:

1. **done**: true if the plan says the block was already done before the plan was written ("done 2026-11-18"). A done block is marked but does not count toward the shape.
2. **sections**: the course sections, by the ids under SECTIONS, whose own material the block sets out to work: its slide pages, the topics it teaches or tests. Judge by its content, not by its title alone. A section whose idea one of the block's problems only uses or carries is not added. Write `all` for a paper across the whole test. Leave the list empty when the block works no course material (marking only, a sheet from memory, leaving for the test) or only material outside SECTIONS.
3. **kind**, one of:
   - `tutor_paper`: a paper across several sections that the tutor or the plan wrote (a Diagnostic, a mock, a "new mock"), sat in one go.
   - `instructor_paper`: the instructor's own past test, or questions from the course's own past papers, sat in one go.
   - `teach_first`: work on one section or a few that opens with teaching: an explanation, a lesson, a worked example or "teach me", before the learner works a problem. A check at its end does not change this.
   - `test_first`: work on one section or a few that opens with the learner doing problems from a blank page without help, with explanation only of what breaks. Reading the worked solutions of problems already sat, before the test, does not change this.
   - `marking`: marking, logging or repairing a paper or test just sat.
   - `chosen_by_marks`: work whose content is picked by what lost marks (the weakest tags, the rows with the most lost marks) rather than named in advance.
   - `redo`: cold redos or spaced review of earlier rows or checks.
   - `other`: anything else (a sheet from memory, rereading the plan, leaving for the test).

   When a block mixes kinds, take the one that decides what the learner does first.
4. **listed**: the ids of the exercises under LISTED EXERCISES that the block sets, as written or changed, or names as carried by one of its problems ("it carries 8.39"). An exercise only listed as what a section's slides hold, without being set or carried, is left out, and so is one named only as an optional extra or in a cut or drop line. Drills, mocks and a Diagnostic are not listed exercises. An empty list when there are none.
5. **unseen**: for `teach_first` and `test_first` blocks, true if the learner does, without help, at least one problem in that block that he has not seen earlier in the plan: not a question from a paper he already sat, and not a worked example he was shown. A block that names no problem for him to do is false. For every other kind, null.
6. **quote**: the words of the block that decide its kind, copied exactly, at most 25 words.

What the marks add up to, with done blocks left out:

- Sections with a block of their own work (`teach_first` or `test_first`).
- Sections in lecture order: the longest run, in lecture order, of the first such block of each section.
- Sections whose first such block is `test_first`.
- Sections with an unseen problem in one of their blocks.
- Listed exercises set or carried by some block.
- Whether a `tutor_paper` comes before the first block on a section.
- Whether the plan has an `instructor_paper`.
- How many blocks are `chosen_by_marks`.
