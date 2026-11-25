# Selection AI: skills on a selection or a page

Selecting text shows the selection menu, and in it **Ask AI**. A press opens
the skills under the bar; a second press shuts them. The owner set the
behaviour on 2026-11-21.

## The skills

- **Built in.** Translate, Summarize and Simplify. Their prompts are fixed
  in code (`crates/assist/src/skill.rs`); Settings only picks their model.
- **In the menu.** The built-ins sit side by side in one row. The reader's
  own fold into one "Custom skills" row (shown only when one exists);
  hovered or pressed, a narrow list opens beside it and scrolls. Typing
  while it shows finds a skill: the search field slides in on top, the
  first match lights, ↑ ↓ move the light, and Enter runs it. The first Escape clears the search and folds
  the list; the next closes the menu. The page menu folds them the same
  way (`skill_menu.rs`, over the shared `side_list.rs`, which the
  Dictionary picker's AI languages use too).
- **Custom.** The reader's own, written in Settings › Skills: a name, a
  prompt and, if wanted, a model. Up to 24.
- **No chatter.** Every prompt, built in or custom, is joined to a fixed
  rule (`NO_CHATTER` in `crates/assist/src/prompt.rs`) that forbids
  greetings, "Okay, I got it", "I'm now going to…", preamble and closing
  remarks. The answer is the result and nothing else.

## Models

- **Selection AI** is one AI task with its own model and backups, in the
  global fallback list. It replaced the three page rows (Summarize page,
  Translate page, Simplify page) in settings v9; a v8 pick becomes that
  skill's own model.
- **A skill's own model** is off by default ("Same as Selection AI"). When
  set, it is asked first; if it is gone or fails, the run falls back to
  Selection AI's model, then its backups.
- The page tasks Summarize, Translate and Simplify ask the same route as
  their skill (`AiSettings::page_model`).

## The card

- **Plain text.** No tags, word classes or synonyms: the AI's answer as it
  streams, under the skill's name, with Copy and Ask again below and who
  answered in the small print. A failure says why; Try again sits in the
  footer's right corner.
- **Pointing.** An arrow points at the selection, and the card rides the
  page as it scrolls, as the word card does. A card from the page menu
  points where the menu opened.
- **Temporary.** Nothing is saved with highlights; closing the card, or
  the book leaving its pane, forgets it. At most eight are kept.
- **Size.** Wider than the word card: from 360 to 560 points, growing with
  the square root of the text's length (`skill_card::width_for`). The body
  scrolls past 460 points, held at the end while words stream in.

## Running in the background

- While a selection skill is running, a softly irregular rainbow outline follows its selected
  line shapes; it goes away on success, failure or cancellation. It is not a saved mark.
  Ask again brings it back. A run goes on when its place is scrolled away; Close stops it.
- While the selection outline is away, the run is a line in the pane's AI stack
  (`docs/study-mode.md`, The corner stack): *Simplify · page 1* with
  *Cancel*, which closes the card with a toast "Stopped Simplify".
- Landing in sight, the card just shows. Landing out of sight, the stack
  keeps *Simplify ready* with **View**, which scrolls back to the text and
  shows the card. A failure shows red there for about 3 s, and a toast
  says "Simplify didn’t finish". Page-menu skills work the same.

## The page menu

The right-click page menu keeps its built-in rows and behaviour (see
`docs/page-ai.md`). When custom skills exist it lists them under "Custom
skills"; one run there reads the whole page and answers in the same card.

## Code

- Settings: `crates/settings/src/skills.rs` (`Skill`, `CustomSkill`,
  `Skills`, routes).
- Engine: `crates/engine/src/skills.rs` (asks, anchors, answers) and
  `session/skills.rs`; command `ai.skill`.
- Assist: `crates/assist/src/skill.rs` (prompts), `service/skills.rs`.
- UI: `study/bar.rs` (Ask AI), `skill_menu.rs`, `skill_card.rs`, `app/skills.rs`,
  `settings_window/skills.rs`, `page_ai/menu/rows.rs`.
- Shots: `skill_answers=sample custom_skills=Quiz_me selection=0:quick
  skill=custom-1`, or `page_skill=0:120:300:custom-1`; `click=Ask_AI
  click=Custom_skills typed=gram` shows the search.
